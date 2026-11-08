//! Single-source camera fan-out via MediaMTX.
//!
//! Exactly one FFmpeg **publisher** captures the camera (video) plus the cpal
//! PCM (audio, piped on stdin) and publishes H.264/Opus to a local MediaMTX
//! path over RTSP. MediaMTX then fans that one stream out to live viewers
//! (WebRTC/WHEP) and to the clip/snapshot readers — so the macOS camera is
//! opened exactly once and the mic stays owned solely by cpal.
//!
//! The publisher is brought up on demand and torn down once nobody needs it,
//! tracked by reference "holds":
//! - **recording** — an in-process hold the recorder takes for a clip's duration.
//! - **live** — a TTL lease taken by `POST /api/live/start`, refreshed by the app.
//! - **snapshot** — a momentary hold while a still is grabbed.

use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{anyhow, Context, Result};
use tokio::io::AsyncWriteExt;
use tokio::process::{Child, ChildStdin, Command};
use tokio::sync::mpsc::UnboundedReceiver;
use tokio::sync::Mutex;
use tracing::{info, warn};

use crate::audio::AudioFormat;
use crate::config::{LiveConfig, RecordingConfig};

/// Fallback warmup, used only when `ffprobe` isn't available to actively probe
/// readiness. MediaMTX only exposes the path once the publisher has connected,
/// so readers that race the cold start would 404.
const PUBLISHER_WARMUP: Duration = Duration::from_millis(1500);

/// Upper bound on how long [`LiveHub::await_ready`] waits for the publisher's
/// RTSP path to become readable. A cold macOS camera can take several seconds to
/// open before FFmpeg even connects to MediaMTX, so this is deliberately generous.
const PUBLISHER_READY_TIMEOUT: Duration = Duration::from_secs(12);
/// Max time a single readiness probe may run before it's killed and retried.
const READY_PROBE_TIMEOUT: Duration = Duration::from_secs(3);
/// Delay between readiness probes while the path is still 404ing.
const READY_PROBE_INTERVAL: Duration = Duration::from_millis(250);

struct Inner {
    child: Option<Child>,
    started_at: Option<Instant>,
    recording_hold: bool,
    snapshot_holds: u32,
    live_lease_until: Option<Instant>,
    idle_since: Option<Instant>,
}

/// Owns the publisher process and arbitrates who keeps the camera alive.
pub struct LiveHub {
    inner: Mutex<Inner>,
    /// Separate lock so PCM writes never block on (or are blocked by) hold
    /// bookkeeping. `None` whenever the publisher is down.
    stdin: Mutex<Option<ChildStdin>>,
    /// Mirrors "publisher is up"; read by the cpal callback to gate PCM output.
    publishing: Arc<AtomicBool>,
    fmt: AudioFormat,
    rec_cfg: RecordingConfig,
    live_cfg: LiveConfig,
}

impl LiveHub {
    pub fn new(
        fmt: AudioFormat,
        rec_cfg: RecordingConfig,
        live_cfg: LiveConfig,
        publishing: Arc<AtomicBool>,
    ) -> Arc<Self> {
        Arc::new(Self {
            inner: Mutex::new(Inner {
                child: None,
                started_at: None,
                recording_hold: false,
                snapshot_holds: 0,
                live_lease_until: None,
                idle_since: None,
            }),
            stdin: Mutex::new(None),
            publishing,
            fmt,
            rec_cfg,
            live_cfg,
        })
    }

    /// RTSP URL that readers (clip recorder, snapshot) pull from.
    pub fn rtsp_url(&self) -> &str {
        &self.live_cfg.mediamtx_rtsp
    }

    /// WHEP URL advertised to the app for live view.
    pub fn whep_url(&self) -> &str {
        &self.live_cfg.whep_url
    }

    // --- holds -------------------------------------------------------------

    /// Hold the publisher up for a clip recording. Returns `true` if this call
    /// started the publisher (so the caller should wait for warmup).
    pub async fn acquire_recording(&self) -> Result<bool> {
        let mut inner = self.inner.lock().await;
        inner.recording_hold = true;
        inner.idle_since = None;
        self.ensure_up_locked(&mut inner).await
    }

    pub async fn release_recording(&self) {
        let mut inner = self.inner.lock().await;
        inner.recording_hold = false;
        self.mark_idle_if_unheld(&mut inner);
    }

    /// Take/refresh the live lease (`POST /api/live/start`).
    pub async fn acquire_live(&self) -> Result<()> {
        let mut inner = self.inner.lock().await;
        inner.live_lease_until =
            Some(Instant::now() + Duration::from_secs(self.live_cfg.lease_ttl_secs));
        inner.idle_since = None;
        self.ensure_up_locked(&mut inner).await?;
        Ok(())
    }

    /// Drop the live lease (`POST /api/live/stop`).
    pub async fn release_live(&self) {
        let mut inner = self.inner.lock().await;
        inner.live_lease_until = None;
        self.mark_idle_if_unheld(&mut inner);
    }

    /// Hold the publisher up while a still is grabbed. Returns `just_started`.
    pub async fn acquire_snapshot(&self) -> Result<bool> {
        let mut inner = self.inner.lock().await;
        inner.snapshot_holds += 1;
        inner.idle_since = None;
        self.ensure_up_locked(&mut inner).await
    }

    pub async fn release_snapshot(&self) {
        let mut inner = self.inner.lock().await;
        inner.snapshot_holds = inner.snapshot_holds.saturating_sub(1);
        self.mark_idle_if_unheld(&mut inner);
    }

    /// Sleep until the freshly started publisher's stream should be readable.
    pub async fn await_warmup(&self) {
        let remaining = {
            let inner = self.inner.lock().await;
            inner
                .started_at
                .map(|t| PUBLISHER_WARMUP.saturating_sub(t.elapsed()))
                .unwrap_or_default()
        };
        if !remaining.is_zero() {
            tokio::time::sleep(remaining).await;
        }
    }

    /// Wait until the freshly started publisher's RTSP path is actually readable,
    /// instead of guessing with a fixed warmup. MediaMTX returns 404 on the path
    /// until the publisher has connected and begun sending; on a cold camera that
    /// can take several seconds, so a clip/snapshot reader started on a fixed
    /// timer races the publisher and dies with "404 Not Found". Poll the path
    /// with `ffprobe` until it answers (or `PUBLISHER_READY_TIMEOUT` elapses).
    pub async fn await_ready(&self) {
        let url = self.live_cfg.mediamtx_rtsp.as_str();
        let deadline = Instant::now() + PUBLISHER_READY_TIMEOUT;
        loop {
            match rtsp_ready(url).await {
                Some(true) => return,
                // ffprobe isn't on PATH — fall back to the fixed warmup.
                None => return self.await_warmup().await,
                Some(false) => {}
            }
            if Instant::now() >= deadline {
                warn!(
                    "live: RTSP path {url} still not ready after {PUBLISHER_READY_TIMEOUT:?}; \
                     starting reader anyway"
                );
                return;
            }
            tokio::time::sleep(READY_PROBE_INTERVAL).await;
        }
    }

    fn mark_idle_if_unheld(&self, inner: &mut Inner) {
        if !held(inner) && inner.idle_since.is_none() {
            inner.idle_since = Some(Instant::now());
        }
    }

    // --- publisher lifecycle ----------------------------------------------

    /// Start the publisher if it isn't already running. Returns `true` when it
    /// was just started (caller should `await_warmup`).
    async fn ensure_up_locked(&self, inner: &mut Inner) -> Result<bool> {
        if inner.child.is_some() {
            return Ok(false);
        }
        let (child, stdin) = self
            .spawn_publisher()
            .context("starting MediaMTX publisher")?;
        *self.stdin.lock().await = Some(stdin);
        inner.child = Some(child);
        inner.started_at = Some(Instant::now());
        self.publishing.store(true, Ordering::Relaxed);
        info!("live: publisher up -> {}", self.live_cfg.mediamtx_rtsp);
        Ok(true)
    }

    async fn stop_locked(&self, inner: &mut Inner) {
        self.publishing.store(false, Ordering::Relaxed);
        self.stdin.lock().await.take(); // closes the PCM pipe
        if let Some(mut child) = inner.child.take() {
            let _ = child.start_kill();
            let _ = child.wait().await;
        }
        inner.started_at = None;
        inner.idle_since = None;
        info!("live: publisher down");
    }

    fn spawn_publisher(&self) -> Result<(Child, ChildStdin)> {
        let video_size = format!("{}x{}", self.rec_cfg.width, self.rec_cfg.height);
        let framerate = self.rec_cfg.framerate.to_string();
        let gop = (self.rec_cfg.framerate * 2).to_string();
        let sample_rate = self.fmt.sample_rate.to_string();
        let channels = self.fmt.channels.to_string();

        let mut cmd = Command::new("ffmpeg");
        cmd.arg("-y").args(["-hide_banner", "-loglevel", "warning"]);

        // Video input: the camera only (no audio from the device).
        #[cfg(target_os = "macos")]
        {
            let input = format!("{}:none", self.rec_cfg.video_device);
            cmd.args([
                "-thread_queue_size",
                "512",
                "-f",
                "avfoundation",
                "-framerate",
                &framerate,
                "-video_size",
                &video_size,
                "-i",
                &input,
            ]);
        }
        #[cfg(target_os = "linux")]
        {
            cmd.args([
                "-thread_queue_size",
                "512",
                "-f",
                "v4l2",
                "-framerate",
                &framerate,
                "-video_size",
                &video_size,
                "-i",
                &self.rec_cfg.video_device,
            ]);
        }

        // Audio input: raw PCM piped from cpal on stdin. The s16le demuxer
        // generates clean, monotonic timestamps from the sample count, which is
        // exactly what we want. We must NOT use `-use_wallclock_as_timestamps`
        // here: on a raw PCM pipe it stamps each read with the wall clock, which
        // comes out non-monotonic ("Queue input is backward in time"). libopus
        // and the RTSP muxer then reject those packets, so the published stream
        // ends up with no usable audio and recorded clips have no sound.
        cmd.args([
            "-thread_queue_size",
            "512",
            "-f",
            "s16le",
            "-ar",
            &sample_rate,
            "-ac",
            &channels,
            "-i",
            "pipe:0",
        ]);

        // Encode for WebRTC: H.264 video + Opus audio, low-latency.
        cmd.args([
            "-c:v",
            "libx264",
            "-preset",
            "veryfast",
            "-tune",
            "zerolatency",
            "-pix_fmt",
            "yuv420p",
            "-g",
            &gop,
            "-c:a",
            "libopus",
            "-b:a",
            "64k",
            "-f",
            "rtsp",
            "-rtsp_transport",
            "tcp",
            self.live_cfg.mediamtx_rtsp.as_str(),
        ]);

        cmd.stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .kill_on_drop(true);

        let mut child = cmd
            .spawn()
            .context("spawning ffmpeg publisher (is it installed and on PATH?)")?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| anyhow!("ffmpeg publisher stdin was not piped"))?;
        Ok((child, stdin))
    }

    /// Background task: tears the publisher down once it has been unheld for
    /// `live_idle_secs`, and expires stale live leases.
    pub async fn run_reaper(self: Arc<Self>) {
        let mut tick = tokio::time::interval(Duration::from_secs(1));
        let grace = Duration::from_secs(self.live_cfg.live_idle_secs);
        loop {
            tick.tick().await;
            let mut inner = self.inner.lock().await;

            if let Some(until) = inner.live_lease_until {
                if Instant::now() >= until {
                    inner.live_lease_until = None;
                }
            }

            if held(&inner) {
                inner.idle_since = None;
            } else if inner.child.is_some() {
                let idle = *inner.idle_since.get_or_insert_with(Instant::now);
                if idle.elapsed() >= grace {
                    self.stop_locked(&mut inner).await;
                }
            }
        }
    }
}

fn held(inner: &Inner) -> bool {
    inner.recording_hold || inner.snapshot_holds > 0 || inner.live_lease_until.is_some()
}

/// Probe whether MediaMTX will serve the publisher's RTSP path yet. `Some(true)`
/// when a reader can open it, `Some(false)` while it still 404s or stalls, and
/// `None` when `ffprobe` can't be run at all (caller should fall back).
async fn rtsp_ready(url: &str) -> Option<bool> {
    let mut cmd = Command::new("ffprobe");
    cmd.args([
        "-rtsp_transport",
        "tcp",
        "-loglevel",
        "error",
        "-i",
        url,
        "-show_entries",
        "format=start_time",
        "-of",
        "csv=p=0",
    ]);
    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true);

    let mut child = match cmd.spawn() {
        Ok(child) => child,
        Err(_) => return None, // ffprobe not installed / not on PATH
    };
    match tokio::time::timeout(READY_PROBE_TIMEOUT, child.wait()).await {
        Ok(Ok(status)) => Some(status.success()),
        Ok(Err(_)) => Some(false),
        Err(_) => {
            // Probe hung (connected but no data yet); kill it and retry.
            let _ = child.start_kill();
            Some(false)
        }
    }
}

/// Pump cpal PCM into the publisher's stdin while it's up. Mirrors the v1
/// recorder's PCM branch, but the target is now the single shared publisher.
pub async fn pump_pcm(hub: Arc<LiveHub>, mut pcm_rx: UnboundedReceiver<Vec<u8>>) {
    while let Some(bytes) = pcm_rx.recv().await {
        let mut guard = hub.stdin.lock().await;
        if let Some(stdin) = guard.as_mut() {
            if let Err(e) = stdin.write_all(&bytes).await {
                warn!("writing PCM to publisher stdin: {e}");
                // Pipe is gone; stop trying until the next publisher comes up.
                *guard = None;
            }
        }
    }
}
