//! Editable copy of `Config` for the Settings tab.
//!
//! Most fields are applied the simple way: "Save & Start" / "Save & Restart"
//! writes them to disk and relaunches the app. The two exceptions are values
//! the running server changes on its own or exposes live, which are kept in
//! sync and persisted immediately (see `app.rs`):
//! - the trigger threshold (also adjustable from the Status tab), and
//! - the Telegram chat ID (filled in when someone sends `/start` to the bot).
//!
//! `values` vs `saved` gives the form a dirty state, so the UI can show
//! "unsaved changes", offer Discard, and disable Save when nothing changed.

use std::io::Read;
use std::ops::RangeInclusive;
use std::path::PathBuf;
use std::process::Command;
use std::sync::mpsc::{self, Receiver, TryRecvError};

use dogitor::config::{
    ApiConfig, AudioConfig, Config, LiveConfig, RecordingConfig, TelegramConfig,
};

/// The one range every trigger-threshold control uses (Status slider,
/// Settings sensitivity slider, level meter), so they always agree.
pub const TRIGGER_DB_RANGE: RangeInclusive<f32> = -70.0..=-10.0;
/// "Quiet" follows the trigger at this distance whenever the trigger moves.
const QUIET_GAP_DB: f32 = 10.0;

type CameraList = Vec<(String, String)>;

/// Every persisted field, as the form edits it.
#[derive(Clone, PartialEq)]
pub struct FormValues {
    pub telegram_token: String,
    pub telegram_chat_id: String,
    pub trigger_db: f32,
    pub quiet_db: f32,
    pub trigger_hold_secs: f32,
    pub quiet_timeout_secs: f32,
    pub cooldown_secs: u64,
    pub output_dir: PathBuf,
    pub video_device: String,
    pub width: u32,
    pub height: u32,
    pub framerate: u32,
    pub max_clip_secs: u64,
    pub api_port: u16,
    pub api_token: String,
    pub mediamtx_rtsp: String,
    pub whep_url: String,
    pub live_idle_secs: u64,
    pub lease_ttl_secs: u64,
}

impl FormValues {
    fn from_config(cfg: &Config) -> Self {
        Self {
            telegram_token: cfg.telegram.token.clone().unwrap_or_default(),
            telegram_chat_id: cfg
                .telegram
                .chat_id
                .filter(|id| *id != 0)
                .map(|id| id.to_string())
                .unwrap_or_default(),
            trigger_db: cfg.audio.trigger_db,
            quiet_db: cfg.audio.quiet_db,
            trigger_hold_secs: cfg.audio.trigger_hold_secs,
            quiet_timeout_secs: cfg.audio.quiet_timeout_secs,
            cooldown_secs: cfg.audio.cooldown_secs,
            output_dir: cfg.recording.output_dir.clone(),
            video_device: cfg.recording.video_device.clone(),
            width: cfg.recording.width,
            height: cfg.recording.height,
            framerate: cfg.recording.framerate,
            max_clip_secs: cfg.recording.max_clip_secs,
            api_port: cfg.api.port,
            api_token: cfg.api.token.clone().unwrap_or_default(),
            mediamtx_rtsp: cfg.live.mediamtx_rtsp.clone(),
            whep_url: cfg.live.whep_url.clone(),
            live_idle_secs: cfg.live.live_idle_secs,
            lease_ttl_secs: cfg.live.lease_ttl_secs,
        }
    }
}

pub struct SettingsForm {
    pub values: FormValues,
    /// What's on disk; `values != saved` means unsaved edits.
    saved: FormValues,

    pub cameras: CameraList,
    pub camera_error: Option<String>,
    camera_job: Option<Receiver<Result<CameraList, String>>>,
    cameras_requested: bool,

    pub error: Option<String>,
    pub show_telegram_token: bool,
    pub show_api_token: bool,
    pub show_advanced: bool,
}

impl SettingsForm {
    pub fn from_config(cfg: &Config) -> Self {
        let values = FormValues::from_config(cfg);
        Self {
            saved: values.clone(),
            values,
            cameras: Vec::new(),
            camera_error: None,
            camera_job: None,
            cameras_requested: false,
            error: None,
            show_telegram_token: false,
            show_api_token: false,
            show_advanced: false,
        }
    }

    pub fn is_dirty(&self) -> bool {
        self.values != self.saved
    }

    pub fn discard(&mut self) {
        self.values = self.saved.clone();
        self.error = None;
    }

    /// Set the trigger threshold (quiet level follows it). Both are treated
    /// as already saved — the caller applies them live and persists them.
    pub fn set_trigger_db(&mut self, db: f32) {
        let quiet = (db - QUIET_GAP_DB).max(-80.0);
        for v in [&mut self.values, &mut self.saved] {
            v.trigger_db = db;
            v.quiet_db = quiet;
        }
    }

    /// Adopt a chat ID the bot discovered via `/start`. Returns `true` if it
    /// differs from what's saved (i.e. the caller should persist it). An
    /// in-progress edit of the field is left alone.
    pub fn sync_chat_id(&mut self, id: i64) -> bool {
        let id = id.to_string();
        if self.saved.telegram_chat_id == id {
            return false;
        }
        if self.values.telegram_chat_id == self.saved.telegram_chat_id {
            self.values.telegram_chat_id = id.clone();
        }
        self.saved.telegram_chat_id = id;
        true
    }

    /// Validate required fields and build a full `Config` ready to serialize.
    pub fn build_config(&self) -> Result<Config, String> {
        let v = &self.values;
        if v.telegram_token.trim().is_empty() {
            return Err("A Telegram bot token is required — get one from @BotFather.".into());
        }
        if v.api_token.trim().is_empty() {
            return Err("An access code is required for the phone app — click New code.".into());
        }
        let chat_id = if v.telegram_chat_id.trim().is_empty() {
            None
        } else {
            Some(
                v.telegram_chat_id
                    .trim()
                    .parse::<i64>()
                    .map_err(|_| "Chat ID must be a number (or left blank).".to_string())?,
            )
        };

        Ok(Config {
            telegram: TelegramConfig {
                token: Some(v.telegram_token.trim().to_string()),
                chat_id,
            },
            audio: AudioConfig {
                trigger_db: v.trigger_db,
                quiet_db: v.quiet_db,
                trigger_hold_secs: v.trigger_hold_secs,
                quiet_timeout_secs: v.quiet_timeout_secs,
                cooldown_secs: v.cooldown_secs,
            },
            recording: RecordingConfig {
                output_dir: v.output_dir.clone(),
                video_device: v.video_device.trim().to_string(),
                width: v.width,
                height: v.height,
                framerate: v.framerate,
                max_clip_secs: v.max_clip_secs,
            },
            api: ApiConfig {
                port: v.api_port,
                token: Some(v.api_token.trim().to_string()),
            },
            live: LiveConfig {
                mediamtx_rtsp: v.mediamtx_rtsp.trim().to_string(),
                whep_url: v.whep_url.trim().to_string(),
                live_idle_secs: v.live_idle_secs,
                lease_ttl_secs: v.lease_ttl_secs,
            },
        })
    }

    /// List cameras the first time Settings is shown, so the picker shows
    /// the configured camera's name without the user having to ask.
    pub fn ensure_cameras_listed(&mut self) {
        if !self.cameras_requested {
            self.refresh_cameras();
        }
    }

    /// Ask ffmpeg for the camera list on a background thread — it takes
    /// around a second, which would otherwise freeze the whole window.
    pub fn refresh_cameras(&mut self) {
        if self.camera_job.is_some() {
            return;
        }
        self.cameras_requested = true;
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(list_camera_devices());
        });
        self.camera_job = Some(rx);
    }

    pub fn listing_cameras(&self) -> bool {
        self.camera_job.is_some()
    }

    /// Display name for the selected camera, falling back to its index.
    pub fn selected_camera_name(&self) -> String {
        self.cameras
            .iter()
            .find(|(idx, _)| *idx == self.values.video_device)
            .map(|(_, name)| name.clone())
            .unwrap_or_else(|| format!("Camera {}", self.values.video_device))
    }

    /// Native folder picker. On macOS it must run on the main thread; it's
    /// modal, so blocking the UI loop while it's open is expected.
    pub fn pick_output_dir(&mut self) {
        if let Some(dir) = rfd::FileDialog::new()
            .set_title("Choose where Dogitor saves clips")
            .set_directory(&self.values.output_dir)
            .pick_folder()
        {
            self.values.output_dir = dir;
        }
    }

    /// Collect results from background jobs. Call once per frame.
    pub fn poll(&mut self) {
        if let Some(rx) = &self.camera_job {
            match rx.try_recv() {
                Ok(Ok(devices)) => {
                    self.cameras = devices;
                    self.camera_error = None;
                    self.camera_job = None;
                }
                Ok(Err(e)) => {
                    self.camera_error = Some(e);
                    self.camera_job = None;
                }
                Err(TryRecvError::Disconnected) => self.camera_job = None,
                Err(TryRecvError::Empty) => {}
            }
        }
    }
}

fn list_camera_devices() -> Result<CameraList, String> {
    let output = Command::new("ffmpeg")
        .args(["-f", "avfoundation", "-list_devices", "true", "-i", ""])
        .output()
        .map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => {
                "FFmpeg isn't installed. Install it with `brew install ffmpeg mediamtx`, then reopen Dogitor."
                    .to_string()
            }
            _ => format!("Couldn't run ffmpeg to look for cameras: {e}"),
        })?;
    let stderr = String::from_utf8_lossy(&output.stderr);

    let mut devices = Vec::new();
    let mut in_video = false;
    for line in stderr.lines() {
        if line.contains("video devices") {
            in_video = true;
            continue;
        }
        if line.contains("audio devices") {
            break;
        }
        if in_video {
            if let Some((idx, name)) = parse_device_line(line) {
                devices.push((idx, name));
            }
        }
    }
    if devices.is_empty() {
        return Err("No cameras found.".to_string());
    }
    Ok(devices)
}

/// Parses a line like `[AVFoundation indev @ 0x...] [0] FaceTime HD Camera`
/// into `("0", "FaceTime HD Camera")`.
fn parse_device_line(line: &str) -> Option<(String, String)> {
    let pos = line.find("] [")?;
    let rest = line[pos + 2..].strip_prefix('[')?;
    let (idx, after) = rest.split_once(']')?;
    Some((idx.to_string(), after.trim_start().to_string()))
}

/// A random 48-hex-char bearer token, read from `/dev/urandom` (no `rand`
/// dependency needed for something this simple).
pub fn random_token() -> String {
    let mut buf = [0u8; 24];
    if std::fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut buf))
        .is_err()
    {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        for (i, b) in buf.iter_mut().enumerate() {
            *b = ((nanos >> ((i % 16) * 4)) & 0xff) as u8;
        }
    }
    buf.iter()
        .fold(String::with_capacity(buf.len() * 2), |mut s, b| {
            use std::fmt::Write;
            let _ = write!(s, "{b:02x}");
            s
        })
}
