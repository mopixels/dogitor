//! Clip listing/thumbnailing shared by the HTTP API (`src/api.rs`) and the
//! macOS GUI's Clips tab, so both surfaces see identical metadata and reuse
//! (rather than regenerate) the same cached thumbnails.

use std::path::{Path, PathBuf};

use chrono::{DateTime, Local, NaiveDateTime, TimeZone};
use tokio::process::Command;

/// One recorded clip on disk.
#[derive(Debug, Clone)]
pub struct ClipInfo {
    pub name: String,
    pub path: PathBuf,
    pub started_at: DateTime<Local>,
    pub duration_s: f32,
    pub size_bytes: u64,
    /// Cached JPEG thumbnail beside the clip (`<stem>.thumb.jpg`), generated
    /// on first request.
    pub thumb_path: Option<PathBuf>,
}

/// List `*.mp4` clips in `dir`, newest first.
pub async fn list(dir: &Path) -> anyhow::Result<Vec<ClipInfo>> {
    let mut rd = match tokio::fs::read_dir(dir).await {
        Ok(rd) => rd,
        Err(_) => return Ok(vec![]), // dir may not exist until the first clip
    };

    let mut clips = Vec::new();
    while let Some(entry) = rd.next_entry().await? {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("mp4") {
            continue;
        }
        let Some(name) = path.file_name().map(|n| n.to_string_lossy().into_owned()) else {
            continue;
        };
        let meta = entry.metadata().await?;
        clips.push(ClipInfo {
            started_at: parse_started_at(&name, &meta),
            duration_s: probe_duration(&path).await.unwrap_or(0.0),
            thumb_path: ensure_thumb(&path).await,
            size_bytes: meta.len(),
            name,
            path,
        });
    }
    clips.sort_by(|a, b| b.started_at.cmp(&a.started_at));
    Ok(clips)
}

/// Prefer the timestamp encoded in the clip name (`clip-YYYYMMDD-HHMMSS.mp4`),
/// falling back to the file's modified time.
fn parse_started_at(name: &str, meta: &std::fs::Metadata) -> DateTime<Local> {
    name.strip_prefix("clip-")
        .and_then(|s| s.split('.').next())
        .and_then(|s| NaiveDateTime::parse_from_str(s, "%Y%m%d-%H%M%S").ok())
        .and_then(|ndt| Local.from_local_datetime(&ndt).single())
        .or_else(|| meta.modified().ok().map(DateTime::<Local>::from))
        .unwrap_or_else(Local::now)
}

async fn probe_duration(path: &Path) -> Option<f32> {
    let out = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-show_entries",
            "format=duration",
            "-of",
            "default=nw=1:nk=1",
        ])
        .arg(path)
        .output()
        .await
        .ok()?;
    if !out.status.success() {
        return None;
    }
    String::from_utf8_lossy(&out.stdout)
        .trim()
        .parse::<f32>()
        .ok()
}

/// Return the path to a cached JPEG thumbnail, generating it from the clip's
/// first frame on first request. Thumbnails live beside the clip as
/// `<stem>.thumb.jpg` (excluded from `list`, which only matches `*.mp4`).
async fn ensure_thumb(clip: &Path) -> Option<PathBuf> {
    let stem = clip.file_stem()?.to_string_lossy().into_owned();
    let thumb_path = clip.with_file_name(format!("{stem}.thumb.jpg"));
    if !thumb_path.exists() {
        let status = Command::new("ffmpeg")
            .args(["-y", "-hide_banner", "-loglevel", "error", "-i"])
            .arg(clip)
            .args(["-frames:v", "1", "-update", "1", "-q:v", "5"])
            .arg(&thumb_path)
            .status()
            .await
            .ok()?;
        if !status.success() {
            return None;
        }
    }
    Some(thumb_path)
}
