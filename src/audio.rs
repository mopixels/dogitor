use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc as std_mpsc;
use std::sync::Arc;

use anyhow::{anyhow, Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::SampleFormat;
use tokio::sync::mpsc::UnboundedSender;
use tracing::{error, info};

/// The audio format chosen by the input device, needed to tell FFmpeg how to
/// interpret the raw PCM we pipe into it.
#[derive(Debug, Clone, Copy)]
pub struct AudioFormat {
    pub sample_rate: u32,
    pub channels: u16,
}

/// Start capturing the default input device.
///
/// The cpal stream is `!Send` on macOS, so it lives on a dedicated thread that
/// stays parked to keep the stream alive. Every audio buffer produces a dBFS
/// level on `level_tx`; while `publishing` is true, the same buffer is also
/// forwarded as interleaved little-endian s16 PCM on `pcm_tx` (consumed by the
/// MediaMTX publisher's stdin so the mic is opened exactly once, by cpal).
pub fn start(
    level_tx: UnboundedSender<f32>,
    pcm_tx: UnboundedSender<Vec<u8>>,
    publishing: Arc<AtomicBool>,
) -> Result<AudioFormat> {
    let (fmt_tx, fmt_rx) = std_mpsc::channel::<Result<AudioFormat, String>>();

    std::thread::Builder::new()
        .name("audio".into())
        .spawn(move || run_thread(level_tx, pcm_tx, publishing, fmt_tx))
        .context("spawning audio thread")?;

    match fmt_rx.recv() {
        Ok(Ok(fmt)) => Ok(fmt),
        Ok(Err(e)) => Err(anyhow!("audio init failed: {e}")),
        Err(_) => Err(anyhow!("audio thread exited before initialization")),
    }
}

fn run_thread(
    level_tx: UnboundedSender<f32>,
    pcm_tx: UnboundedSender<Vec<u8>>,
    publishing: Arc<AtomicBool>,
    fmt_tx: std_mpsc::Sender<Result<AudioFormat, String>>,
) {
    match build_stream(level_tx, pcm_tx, publishing) {
        Ok((stream, fmt)) => {
            if let Err(e) = stream.play() {
                let _ = fmt_tx.send(Err(format!("stream play: {e}")));
                return;
            }
            let _ = fmt_tx.send(Ok(fmt));
            info!(
                "audio capture running: {} Hz, {} ch",
                fmt.sample_rate, fmt.channels
            );
            // Keep the stream alive for the lifetime of the process.
            loop {
                std::thread::park();
            }
        }
        Err(e) => {
            let _ = fmt_tx.send(Err(format!("{e:#}")));
        }
    }
}

fn build_stream(
    level_tx: UnboundedSender<f32>,
    pcm_tx: UnboundedSender<Vec<u8>>,
    publishing: Arc<AtomicBool>,
) -> Result<(cpal::Stream, AudioFormat)> {
    let host = cpal::default_host();
    let device = host
        .default_input_device()
        .ok_or_else(|| anyhow!("no default audio input device"))?;
    info!(
        "audio input device: {}",
        device.name().unwrap_or_else(|_| "<unknown>".into())
    );

    let supported = device
        .default_input_config()
        .context("querying default input config")?;
    let sample_format = supported.sample_format();
    let config: cpal::StreamConfig = supported.into();
    let fmt = AudioFormat {
        sample_rate: config.sample_rate.0,
        channels: config.channels,
    };

    let err_fn = |e| error!("audio stream error: {e}");

    let stream = match sample_format {
        SampleFormat::F32 => device.build_input_stream(
            &config,
            move |data: &[f32], _| {
                let _ = level_tx.send(rms_dbfs_f32(data));
                if publishing.load(Ordering::Relaxed) {
                    let mut bytes = Vec::with_capacity(data.len() * 2);
                    for &s in data {
                        let v = (s.clamp(-1.0, 1.0) * 32767.0) as i16;
                        bytes.extend_from_slice(&v.to_le_bytes());
                    }
                    let _ = pcm_tx.send(bytes);
                }
            },
            err_fn,
            None,
        )?,
        SampleFormat::I16 => device.build_input_stream(
            &config,
            move |data: &[i16], _| {
                let _ = level_tx.send(rms_dbfs_i16(data));
                if publishing.load(Ordering::Relaxed) {
                    let mut bytes = Vec::with_capacity(data.len() * 2);
                    for &s in data {
                        bytes.extend_from_slice(&s.to_le_bytes());
                    }
                    let _ = pcm_tx.send(bytes);
                }
            },
            err_fn,
            None,
        )?,
        other => {
            return Err(anyhow!(
                "unsupported sample format {other:?} (expected f32 or i16)"
            ))
        }
    };

    Ok((stream, fmt))
}

fn rms_dbfs_f32(data: &[f32]) -> f32 {
    if data.is_empty() {
        return -120.0;
    }
    let sum: f64 = data.iter().map(|&s| (s as f64) * (s as f64)).sum();
    dbfs((sum / data.len() as f64).sqrt())
}

fn rms_dbfs_i16(data: &[i16]) -> f32 {
    if data.is_empty() {
        return -120.0;
    }
    let sum: f64 = data
        .iter()
        .map(|&s| {
            let v = s as f64 / 32768.0;
            v * v
        })
        .sum();
    dbfs((sum / data.len() as f64).sqrt())
}

fn dbfs(rms: f64) -> f32 {
    if rms <= 1e-10 {
        -120.0
    } else {
        (20.0 * rms.log10()) as f32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_scale_is_near_zero_dbfs() {
        // A constant full-scale signal has RMS 1.0 -> 0 dBFS.
        assert!((rms_dbfs_f32(&[1.0, 1.0, 1.0, 1.0]) - 0.0).abs() < 0.01);
        // i16 max is one LSB below 1.0, so just under 0 dBFS.
        assert!(rms_dbfs_i16(&[i16::MAX; 4]) > -0.01);
    }

    #[test]
    fn half_scale_is_about_minus_six_db() {
        assert!((rms_dbfs_f32(&[0.5, -0.5, 0.5, -0.5]) - (-6.02)).abs() < 0.05);
    }

    #[test]
    fn silence_is_floored() {
        assert_eq!(rms_dbfs_f32(&[0.0; 8]), -120.0);
        assert_eq!(rms_dbfs_i16(&[0; 8]), -120.0);
        assert_eq!(rms_dbfs_f32(&[]), -120.0);
    }
}
