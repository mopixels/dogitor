use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;

use anyhow::{anyhow, Context, Result};
use chrono::Local;
use tokio::io::AsyncWriteExt;
use tokio::process::{Child, ChildStdin, Command};
use tokio::sync::broadcast;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use tracing::{error, info};

use crate::config::RecordingConfig;
use crate::events::{ApiEvent, Notification, RecorderCommand};
use crate::live::LiveHub;
use crate::state::SharedState;

/// Records clips and grabs snapshots by reading from the shared MediaMTX
/// stream — never by opening the camera directly. The publisher (see
/// [`LiveHub`]) is the only camera consumer; recording is just another reader,
/// so live view and triggered recording can run at the same time.
pub async fn run(
    mut cmd_rx: UnboundedReceiver<RecorderCommand>,
    live: Arc<LiveHub>,
    cfg: RecordingConfig,
    state: Arc<SharedState>,
    notif_tx: UnboundedSender<Notification>,
    events_tx: broadcast::Sender<ApiEvent>,
) {
    if let Err(e) = std::fs::create_dir_all(&cfg.output_dir) {
        error!(
            "could not create output dir {}: {e}",
            cfg.output_dir.display()
        );
    }

    let mut child: Option<Child> = None;
    let mut stdin: Option<ChildStdin> = None;
    let mut current_path: Option<PathBuf> = None;

    while let Some(cmd) = cmd_rx.recv().await {
        match cmd {
            RecorderCommand::Start => {
                if child.is_some() {
                    continue;
                }
                let just_started = match live.acquire_recording().await {
                    Ok(v) => v,
                    Err(e) => {
                        error!("could not bring up publisher for recording: {e:#}");
                        let _ =
                            notif_tx.send(Notification::Text(format!("⚠️ Recording failed: {e}")));
                        continue;
                    }
                };
                if just_started {
                    live.await_ready().await;
                }

                let path = cfg
                    .output_dir
                    .join(format!("clip-{}.mp4", Local::now().format("%Y%m%d-%H%M%S")));
                match spawn_clip_reader(live.rtsp_url(), cfg.max_clip_secs, &path) {
                    Ok((c, si)) => {
                        info!("recording -> {}", path.display());
                        child = Some(c);
                        stdin = Some(si);
                        current_path = Some(path);
                        state.set_recording(true);
                        broadcast_state(&events_tx, &state);
                    }
                    Err(e) => {
                        error!("failed to start clip reader: {e:#}");
                        live.release_recording().await;
                        let _ =
                            notif_tx.send(Notification::Text(format!("⚠️ Recording failed: {e}")));
                    }
                }
            }
            RecorderCommand::Stop => {
                // Ask FFmpeg to quit cleanly so the MP4 is finalized (moov atom
                // written), then release the publisher hold.
                if let Some(mut si) = stdin.take() {
                    let _ = si.write_all(b"q").await;
                    let _ = si.flush().await;
                }
                if let Some(mut c) = child.take() {
                    match c.wait().await {
                        Ok(status) => info!("clip reader finished: {status}"),
                        Err(e) => error!("waiting on clip reader: {e}"),
                    }
                }
                state.set_recording(false);
                live.release_recording().await;
                broadcast_state(&events_tx, &state);

                if let Some(path) = current_path.take() {
                    state.set_last_clip(path.clone());
                    if let Some(name) = file_name(&path) {
                        let _ = events_tx.send(ApiEvent::Clip { name });
                    }
                    let _ = notif_tx.send(Notification::Clip(path));
                }
            }
            RecorderCommand::Snapshot => match capture_snapshot(&live, &cfg).await {
                Ok(path) => {
                    let _ = notif_tx.send(Notification::Photo(path));
                }
                Err(e) => {
                    let _ = notif_tx.send(Notification::Text(format!("⚠️ Snapshot failed: {e}")));
                }
            },
        }
    }
}

fn broadcast_state(events_tx: &broadcast::Sender<ApiEvent>, state: &SharedState) {
    let _ = events_tx.send(ApiEvent::State {
        armed: state.armed(),
        recording: state.recording(),
    });
}

fn file_name(path: &Path) -> Option<String> {
    path.file_name().map(|n| n.to_string_lossy().into_owned())
}

/// Spawn the clip recorder: read the shared stream from MediaMTX, copy the
/// H.264 video, transcode Opus audio to AAC (for broad MP4 playback / Telegram),
/// cap the length, and write a faststart MP4. stdin is piped so we can send `q`.
fn spawn_clip_reader(
    rtsp_url: &str,
    max_clip_secs: u64,
    path: &Path,
) -> Result<(Child, ChildStdin)> {
    let max_clip = max_clip_secs.to_string();

    let mut cmd = Command::new("ffmpeg");
    cmd.arg("-y")
        .args(["-hide_banner", "-loglevel", "warning"])
        .args(["-rtsp_transport", "tcp", "-i", rtsp_url])
        .args([
            "-c:v",
            "copy",
            "-c:a",
            "aac",
            "-b:a",
            "128k",
            "-t",
            &max_clip,
            "-movflags",
            "+faststart",
        ])
        .arg(path);

    cmd.stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .kill_on_drop(true);

    let mut child = cmd
        .spawn()
        .context("spawning ffmpeg clip reader (is it installed and on PATH?)")?;
    let stdin = child
        .stdin
        .take()
        .ok_or_else(|| anyhow!("clip reader stdin was not piped"))?;
    Ok((child, stdin))
}

/// Capture a single still frame from the shared stream. Brings the publisher up
/// momentarily if needed. Shared by the Telegram `/snapshot` command and the
/// `POST /api/snapshot` handler.
pub async fn capture_snapshot(live: &LiveHub, cfg: &RecordingConfig) -> Result<PathBuf> {
    let just_started = live.acquire_snapshot().await?;
    if just_started {
        live.await_ready().await;
    }
    let result = grab_frame(live.rtsp_url(), &cfg.output_dir).await;
    live.release_snapshot().await;
    result
}

async fn grab_frame(rtsp_url: &str, output_dir: &Path) -> Result<PathBuf> {
    let path = output_dir.join(format!("snap-{}.jpg", Local::now().format("%Y%m%d-%H%M%S")));

    let mut cmd = Command::new("ffmpeg");
    cmd.arg("-y")
        .args(["-hide_banner", "-loglevel", "warning"])
        .args(["-rtsp_transport", "tcp", "-i", rtsp_url])
        .args(["-frames:v", "1", "-update", "1", "-q:v", "2"])
        .arg(&path);
    cmd.stdout(Stdio::null());

    let status = cmd.status().await.context("spawning ffmpeg snapshot")?;
    if !status.success() {
        return Err(anyhow!("ffmpeg snapshot exited with {status}"));
    }
    Ok(path)
}
