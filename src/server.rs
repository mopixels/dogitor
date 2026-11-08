//! Starts every background task (audio capture, detector, recorder, live
//! media fan-out, HTTP API, Telegram bot) and hands back the shared handles a
//! caller needs to observe/drive the running monitor. Used identically by the
//! headless CLI (`src/main.rs`) and the macOS GUI (`src/bin/dogitor_gui/`).

use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use teloxide::Bot;
use tokio::sync::broadcast;
use tokio::sync::mpsc::{unbounded_channel, UnboundedSender};
use tracing::{error, info};

use crate::api::{self, AppState};
use crate::config::Config;
use crate::detector::{self, Timings};
use crate::events::{ApiEvent, Notification, RecorderCommand};
use crate::live::{self, LiveHub};
use crate::recorder;
use crate::state::SharedState;
use crate::{audio, telegram};

/// Shared handles into a running monitor, returned by [`start`].
pub struct ServerHandle {
    pub state: Arc<SharedState>,
    pub rec_tx: UnboundedSender<RecorderCommand>,
    pub events_tx: broadcast::Sender<ApiEvent>,
    pub cfg: Config,
}

/// Spin up the whole monitor (audio capture, detector, recorder, live media,
/// HTTP API, Telegram bot) as background tasks and return handles to observe
/// and drive it. Never blocks waiting for shutdown — the caller decides how
/// to keep the process alive (e.g. `ctrl_c()` for the CLI, the GUI event loop
/// for the Mac app).
pub async fn start(cfg: Config) -> Result<ServerHandle> {
    let token = cfg.token()?;
    let api_token = Arc::new(cfg.api_token()?);

    let state = Arc::new(SharedState::new(
        true,
        cfg.telegram.chat_id.unwrap_or(0),
        cfg.audio.trigger_db,
    ));

    let (level_tx, level_rx) = unbounded_channel::<f32>();
    let (pcm_tx, pcm_rx) = unbounded_channel::<Vec<u8>>();
    let (rec_tx, rec_rx) = unbounded_channel::<RecorderCommand>();
    let (notif_tx, notif_rx) = unbounded_channel::<Notification>();
    let (events_tx, _events_rx) = broadcast::channel::<ApiEvent>(256);

    // True whenever the MediaMTX publisher is up; gates cpal's PCM output so the
    // publisher (and only the publisher) receives mic audio.
    let publishing = Arc::new(AtomicBool::new(false));

    // Audio capture spawns its own (non-Send) thread and reports the format.
    let fmt = audio::start(level_tx, pcm_tx, publishing.clone())?;
    info!("audio format: {} Hz, {} ch", fmt.sample_rate, fmt.channels);

    // The single camera fan-out: one on-demand publisher, shared by live + recording.
    let live = LiveHub::new(fmt, cfg.recording.clone(), cfg.live.clone(), publishing);

    let timings = Timings {
        quiet_db: cfg.audio.quiet_db,
        trigger_hold: Duration::from_secs_f32(cfg.audio.trigger_hold_secs),
        quiet_timeout: Duration::from_secs_f32(cfg.audio.quiet_timeout_secs),
        cooldown: Duration::from_secs(cfg.audio.cooldown_secs),
        max_clip: Duration::from_secs(cfg.recording.max_clip_secs),
    };
    tokio::spawn(detector::run(
        level_rx,
        state.clone(),
        timings,
        rec_tx.clone(),
        notif_tx.clone(),
        events_tx.clone(),
    ));

    tokio::spawn(recorder::run(
        rec_rx,
        live.clone(),
        cfg.recording.clone(),
        state.clone(),
        notif_tx.clone(),
        events_tx.clone(),
    ));

    // Feed cpal PCM to the publisher, reap the idle publisher, and push the live
    // dB meter to API clients.
    tokio::spawn(live::pump_pcm(live.clone(), pcm_rx));
    tokio::spawn(live.clone().run_reaper());
    tokio::spawn(api::run_level_broadcaster(state.clone(), events_tx.clone()));

    // HTTP + WebSocket API for the companion app.
    let app_state = AppState {
        state: state.clone(),
        live: live.clone(),
        events_tx: events_tx.clone(),
        rec_cfg: cfg.recording.clone(),
        token: api_token,
    };
    let api_port = cfg.api.port;
    tokio::spawn(async move {
        if let Err(e) = api::run(app_state, api_port).await {
            error!("api server exited: {e:#}");
        }
    });

    let bot = Bot::new(token);
    tokio::spawn(telegram::run_outbound(bot.clone(), state.clone(), notif_rx));
    tokio::spawn(telegram::run_commands(
        bot,
        state.clone(),
        rec_tx.clone(),
        events_tx.clone(),
    ));

    let _ = notif_tx.send(Notification::Text("Dogitor online \u{2705}".into()));
    info!("dogitor running \u{2014} send /start to your bot if the chat id is unset");

    Ok(ServerHandle {
        state,
        rec_tx,
        events_tx,
        cfg,
    })
}
