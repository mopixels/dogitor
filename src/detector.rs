use std::sync::Arc;
use std::time::{Duration, Instant};

use chrono::Local;
use tokio::sync::broadcast;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use tracing::info;

use crate::events::{ApiEvent, Notification, RecorderCommand};
use crate::state::SharedState;

/// Timing/threshold parameters for the trigger state machine.
pub struct Timings {
    pub quiet_db: f32,
    pub trigger_hold: Duration,
    pub quiet_timeout: Duration,
    pub cooldown: Duration,
    pub max_clip: Duration,
}

#[derive(PartialEq)]
enum State {
    Idle,
    Recording,
}

/// Consume dBFS levels and drive recording with hysteresis:
/// - Idle -> Recording once the level stays above the (live-adjustable) trigger
///   threshold for `trigger_hold`.
/// - Recording -> Idle once it stays below `quiet_db` for `quiet_timeout`, or
///   `max_clip` is reached, or monitoring is disarmed. Then a `cooldown` window
///   suppresses immediate re-triggering.
pub async fn run(
    mut level_rx: UnboundedReceiver<f32>,
    state: Arc<SharedState>,
    t: Timings,
    rec_tx: UnboundedSender<RecorderCommand>,
    notif_tx: UnboundedSender<Notification>,
    events_tx: broadcast::Sender<ApiEvent>,
) {
    let mut st = State::Idle;
    let mut loud_since: Option<Instant> = None;
    let mut quiet_since: Option<Instant> = None;
    let mut record_start: Option<Instant> = None;
    let mut cooldown_until: Option<Instant> = None;

    while let Some(db) = level_rx.recv().await {
        state.set_current_db(db);
        let now = Instant::now();
        let trigger_db = state.trigger_db();

        match st {
            State::Idle => {
                if let Some(until) = cooldown_until {
                    if now < until {
                        continue;
                    }
                    cooldown_until = None;
                }
                if !state.armed() {
                    loud_since = None;
                    continue;
                }
                if db >= trigger_db {
                    let since = *loud_since.get_or_insert(now);
                    if now.duration_since(since) >= t.trigger_hold {
                        st = State::Recording;
                        record_start = Some(now);
                        quiet_since = None;
                        loud_since = None;
                        let at = Local::now();
                        state.set_last_trigger(at);
                        info!("TRIGGER {db:.1} dBFS (>= {trigger_db:.1})");
                        let _ = rec_tx.send(RecorderCommand::Start);
                        let _ = notif_tx.send(Notification::SoundDetected(db));
                        let _ = events_tx.send(ApiEvent::Trigger { db, at });
                    }
                } else {
                    loud_since = None;
                }
            }
            State::Recording => {
                let started = record_start.unwrap_or(now);
                let over_max = now.duration_since(started) >= t.max_clip;
                let disarmed = !state.armed();
                let quiet_done = if db < t.quiet_db {
                    let since = *quiet_since.get_or_insert(now);
                    now.duration_since(since) >= t.quiet_timeout
                } else {
                    quiet_since = None;
                    false
                };

                if quiet_done || over_max || disarmed {
                    let reason = if disarmed {
                        "disarmed"
                    } else if over_max {
                        "max length"
                    } else {
                        "quiet"
                    };
                    info!("STOP recording ({reason})");
                    let _ = rec_tx.send(RecorderCommand::Stop);
                    st = State::Idle;
                    record_start = None;
                    quiet_since = None;
                    loud_since = None;
                    cooldown_until = Some(now + t.cooldown);
                }
            }
        }
    }
}
