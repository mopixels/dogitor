use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU32, Ordering};
use std::sync::Mutex;

use chrono::{DateTime, Local};

/// Shared, concurrently-accessed monitor state.
///
/// Hot values (armed flag, thresholds, current level) are atomics so the audio
/// callback / detector can read and write them without locking; the rarely
/// touched values (timestamps, last clip path) use a mutex.
pub struct SharedState {
    armed: AtomicBool,
    /// Telegram chat id to notify. `0` means "not set yet" (send /start).
    chat_id: AtomicI64,
    /// f32 bits — see `trigger_db` / `set_trigger_db`.
    trigger_db: AtomicU32,
    current_db: AtomicU32,
    /// True while a clip is being recorded. Surfaced in `/api/status` and the
    /// `State` event; the detector owns the actual transitions.
    recording: AtomicBool,
    last_trigger: Mutex<Option<DateTime<Local>>>,
    last_clip: Mutex<Option<PathBuf>>,
}

impl SharedState {
    pub fn new(armed: bool, chat_id: i64, trigger_db: f32) -> Self {
        Self {
            armed: AtomicBool::new(armed),
            chat_id: AtomicI64::new(chat_id),
            trigger_db: AtomicU32::new(trigger_db.to_bits()),
            current_db: AtomicU32::new((-120.0f32).to_bits()),
            recording: AtomicBool::new(false),
            last_trigger: Mutex::new(None),
            last_clip: Mutex::new(None),
        }
    }

    pub fn armed(&self) -> bool {
        self.armed.load(Ordering::Relaxed)
    }
    pub fn set_armed(&self, v: bool) {
        self.armed.store(v, Ordering::Relaxed);
    }

    pub fn chat_id(&self) -> i64 {
        self.chat_id.load(Ordering::Relaxed)
    }
    pub fn set_chat_id(&self, v: i64) {
        self.chat_id.store(v, Ordering::Relaxed);
    }
    /// Make `v` the owning chat if none is registered yet. Returns `true` if
    /// `v` is the owner afterwards (it just claimed it, or already was), and
    /// `false` if a different chat already owns the bot.
    pub fn claim_chat_id(&self, v: i64) -> bool {
        match self
            .chat_id
            .compare_exchange(0, v, Ordering::SeqCst, Ordering::SeqCst)
        {
            Ok(_) => true,
            Err(owner) => owner == v,
        }
    }

    pub fn trigger_db(&self) -> f32 {
        f32::from_bits(self.trigger_db.load(Ordering::Relaxed))
    }
    /// Whether `v` is acceptable as a trigger threshold from a remote control
    /// surface (API / Telegram): a real number in the dBFS range. Rejects NaN,
    /// which would make every comparison false and silently stop detection.
    pub fn valid_trigger_db(v: f32) -> bool {
        v.is_finite() && (-120.0..=0.0).contains(&v)
    }
    pub fn set_trigger_db(&self, v: f32) {
        self.trigger_db.store(v.to_bits(), Ordering::Relaxed);
    }

    pub fn current_db(&self) -> f32 {
        f32::from_bits(self.current_db.load(Ordering::Relaxed))
    }
    pub fn set_current_db(&self, v: f32) {
        self.current_db.store(v.to_bits(), Ordering::Relaxed);
    }

    pub fn recording(&self) -> bool {
        self.recording.load(Ordering::Relaxed)
    }
    pub fn set_recording(&self, v: bool) {
        self.recording.store(v, Ordering::Relaxed);
    }

    pub fn last_trigger(&self) -> Option<DateTime<Local>> {
        *self.last_trigger.lock().unwrap()
    }
    pub fn set_last_trigger(&self, t: DateTime<Local>) {
        *self.last_trigger.lock().unwrap() = Some(t);
    }

    pub fn last_clip(&self) -> Option<PathBuf> {
        self.last_clip.lock().unwrap().clone()
    }
    pub fn set_last_clip(&self, p: PathBuf) {
        *self.last_clip.lock().unwrap() = Some(p);
    }
}
