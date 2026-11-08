use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// Top-level configuration, loaded from a TOML file (default `config.toml`).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Config {
    #[serde(default)]
    pub telegram: TelegramConfig,
    #[serde(default)]
    pub audio: AudioConfig,
    #[serde(default)]
    pub recording: RecordingConfig,
    #[serde(default)]
    pub api: ApiConfig,
    #[serde(default)]
    pub live: LiveConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TelegramConfig {
    /// Bot token. The env var `DOGITOR_BOT_TOKEN` takes precedence over this.
    #[serde(default)]
    pub token: Option<String>,
    /// Chat id to send notifications to. Discover it by sending `/start`.
    #[serde(default)]
    pub chat_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AudioConfig {
    /// Start recording when the level rises above this dBFS.
    #[serde(serialize_with = "serialize_f32_as_f64")]
    pub trigger_db: f32,
    /// Consider the room "quiet" when the level is below this dBFS.
    #[serde(serialize_with = "serialize_f32_as_f64")]
    pub quiet_db: f32,
    /// Sound must stay above `trigger_db` this long before triggering.
    #[serde(serialize_with = "serialize_f32_as_f64")]
    pub trigger_hold_secs: f32,
    /// Room must stay below `quiet_db` this long before recording stops.
    #[serde(serialize_with = "serialize_f32_as_f64")]
    pub quiet_timeout_secs: f32,
    /// Ignore new triggers for this long after a clip finishes.
    pub cooldown_secs: u64,
}

/// `toml`'s float serializer upcasts `f32 -> f64` with a plain `as` cast,
/// which prints values like `0.3` as `0.30000001192092896` (the exact f64
/// widening of the f32 bit pattern). Round-tripping through f32's own
/// (shortest, precise-at-f32) `Display` first keeps hand-edited config files
/// readable after the GUI's Settings tab saves them.
fn serialize_f32_as_f64<S: serde::Serializer>(v: &f32, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_f64(v.to_string().parse().unwrap())
}

impl Default for AudioConfig {
    fn default() -> Self {
        Self {
            trigger_db: -30.0,
            quiet_db: -40.0,
            trigger_hold_secs: 0.3,
            quiet_timeout_secs: 3.0,
            cooldown_secs: 5,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct RecordingConfig {
    pub output_dir: PathBuf,
    /// macOS: avfoundation device index (e.g. "0"). Linux: device path (e.g. "/dev/video0").
    pub video_device: String,
    pub width: u32,
    pub height: u32,
    pub framerate: u32,
    /// Hard cap on a single clip's length, in seconds.
    pub max_clip_secs: u64,
}

impl Default for RecordingConfig {
    fn default() -> Self {
        Self {
            output_dir: PathBuf::from("clips"),
            video_device: "0".to_string(),
            width: 640,
            height: 480,
            framerate: 30,
            max_clip_secs: 60,
        }
    }
}

/// HTTP + WebSocket API exposed to the companion app over the tailnet.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ApiConfig {
    /// Port for the axum server (bound on `0.0.0.0`).
    pub port: u16,
    /// Shared bearer token required on every request. The env var
    /// `DOGITOR_API_TOKEN` takes precedence over this.
    pub token: Option<String>,
}

impl Default for ApiConfig {
    fn default() -> Self {
        Self {
            port: 8088,
            token: None,
        }
    }
}

/// Live-streaming / MediaMTX settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct LiveConfig {
    /// RTSP path the publisher writes to and clip/snapshot readers read from.
    pub mediamtx_rtsp: String,
    /// WHEP (WebRTC) URL advertised to the app, e.g.
    /// `http://dogitor.<tailnet>.ts.net:8889/cam/whep`.
    pub whep_url: String,
    /// Keep the publisher alive this long after the last hold drops (avoids
    /// thrashing the camera between back-to-back consumers).
    pub live_idle_secs: u64,
    /// A `live/start` lease expires after this long unless the app refreshes it.
    pub lease_ttl_secs: u64,
}

impl Default for LiveConfig {
    fn default() -> Self {
        Self {
            mediamtx_rtsp: "rtsp://127.0.0.1:8554/cam".to_string(),
            whep_url: "http://127.0.0.1:8889/cam/whep".to_string(),
            live_idle_secs: 10,
            lease_ttl_secs: 30,
        }
    }
}

impl Config {
    /// Load config from `path`, then override secrets from env vars if set:
    /// `DOGITOR_BOT_TOKEN` (Telegram) and `DOGITOR_API_TOKEN` (HTTP API).
    pub fn load(path: &str) -> Result<Self> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("reading config file `{path}` (copy config.toml.example?)"))?;
        let mut cfg: Config = toml::from_str(&text).context("parsing config TOML")?;
        if let Ok(token) = std::env::var("DOGITOR_BOT_TOKEN") {
            if !token.is_empty() {
                cfg.telegram.token = Some(token);
            }
        }
        if let Ok(token) = std::env::var("DOGITOR_API_TOKEN") {
            if !token.is_empty() {
                cfg.api.token = Some(token);
            }
        }
        Ok(cfg)
    }

    /// Resolve the bot token or fail with a helpful message.
    pub fn token(&self) -> Result<String> {
        self.telegram
            .token
            .clone()
            .filter(|t| !t.is_empty())
            .context("no bot token: set DOGITOR_BOT_TOKEN env var or [telegram].token in config")
    }

    /// Resolve the API bearer token or fail with a helpful message.
    pub fn api_token(&self) -> Result<String> {
        self.api
            .token
            .clone()
            .filter(|t| !t.is_empty())
            .context("no API token: set DOGITOR_API_TOKEN env var or [api].token in config")
    }
}
