use std::path::PathBuf;

use chrono::{DateTime, Local};
use serde::Serialize;

/// Commands sent to the recorder task.
#[derive(Debug)]
pub enum RecorderCommand {
    /// Begin recording a clip (spawn FFmpeg, open the PCM pipe).
    Start,
    /// Stop the current recording and finalize the file.
    Stop,
    /// Capture a single still frame, independent of recording state.
    Snapshot,
}

/// Real-time events pushed to API clients over the `/api/events` WebSocket.
///
/// Serializes to a tagged object, e.g. `{"type":"level","db":-42.1}`, matching
/// the shared TypeScript `ApiEvent` union the app imports.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum ApiEvent {
    /// Throttled live audio level (dBFS).
    Level { db: f32 },
    /// Armed / recording flags changed.
    State { armed: bool, recording: bool },
    /// A sound crossed the threshold and recording began.
    Trigger { db: f32, at: DateTime<Local> },
    /// A finished clip is available (file name within the clips dir).
    Clip { name: String },
}

/// Outbound messages delivered to the user via Telegram.
#[derive(Debug)]
pub enum Notification {
    /// Plain status/info text.
    Text(String),
    /// A loud sound crossed the threshold (carries the measured dBFS).
    SoundDetected(f32),
    /// A finished video clip is ready to send.
    Clip(PathBuf),
    /// A still photo is ready to send.
    Photo(PathBuf),
}
