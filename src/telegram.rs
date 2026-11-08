use std::sync::Arc;

use teloxide::prelude::*;
use teloxide::types::{ChatId, InputFile};
use teloxide::utils::command::BotCommands;
use tokio::sync::broadcast;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use tracing::{error, warn};

use crate::events::{ApiEvent, Notification, RecorderCommand};
use crate::state::SharedState;

/// Bot commands the user can send. `/start` is how a chat registers itself.
#[derive(BotCommands, Clone)]
#[command(rename_rule = "lowercase", description = "Dogitor commands:")]
pub enum Command {
    #[command(description = "register this chat and show its id")]
    Start,
    #[command(description = "show monitor status")]
    Status,
    #[command(description = "enable monitoring")]
    Arm,
    #[command(description = "disable monitoring")]
    Disarm,
    #[command(description = "set trigger threshold in dBFS, e.g. /threshold -30")]
    Threshold(f32),
    #[command(description = "capture a snapshot now")]
    Snapshot,
    #[command(description = "resend the most recent clip")]
    Last,
}

/// Send outbound notifications to the registered chat.
pub async fn run_outbound(
    bot: Bot,
    state: Arc<SharedState>,
    mut rx: UnboundedReceiver<Notification>,
) {
    while let Some(n) = rx.recv().await {
        let id = state.chat_id();
        if id == 0 {
            warn!("no chat id set yet; send /start to the bot. dropping notification");
            continue;
        }
        let chat = ChatId(id);
        let result = match &n {
            Notification::Text(t) => bot.send_message(chat, t).await.map(|_| ()),
            Notification::SoundDetected(db) => bot
                .send_message(
                    chat,
                    format!("🐕 Sound detected ({db:.1} dBFS) — recording…"),
                )
                .await
                .map(|_| ()),
            Notification::Clip(p) => bot.send_video(chat, InputFile::file(p)).await.map(|_| ()),
            Notification::Photo(p) => bot.send_photo(chat, InputFile::file(p)).await.map(|_| ()),
        };
        if let Err(e) = result {
            error!("telegram send failed: {e}");
            if matches!(n, Notification::Clip(_)) {
                let _ = bot
                    .send_message(chat, format!("⚠️ Couldn't send clip: {e}"))
                    .await;
            }
        }
    }
}

/// Run the inbound command dispatcher (long polling). Blocks until shutdown.
pub async fn run_commands(
    bot: Bot,
    state: Arc<SharedState>,
    rec_tx: UnboundedSender<RecorderCommand>,
    events_tx: broadcast::Sender<ApiEvent>,
) {
    let handler = Update::filter_message()
        .filter_command::<Command>()
        .endpoint(handle);

    Dispatcher::builder(bot, handler)
        .dependencies(dptree::deps![state, rec_tx, events_tx])
        .enable_ctrlc_handler()
        .build()
        .dispatch()
        .await;
}

/// Push a `State` event so the app reflects control changes made via Telegram.
fn broadcast_state(events_tx: &broadcast::Sender<ApiEvent>, state: &SharedState) {
    let _ = events_tx.send(ApiEvent::State {
        armed: state.armed(),
        recording: state.recording(),
    });
}

async fn handle(
    bot: Bot,
    msg: Message,
    cmd: Command,
    state: Arc<SharedState>,
    rec_tx: UnboundedSender<RecorderCommand>,
    events_tx: broadcast::Sender<ApiEvent>,
) -> ResponseResult<()> {
    // Bot usernames are public, so anyone can message the bot. Only the
    // registered chat may control it; `/start` registers a chat only while
    // none is set (first claim wins, and a configured `chat_id` is never
    // replaced). Everyone else is ignored without a reply, so strangers learn
    // nothing — not even that the bot is running.
    let chat = msg.chat.id.0;
    let authorized = match cmd {
        Command::Start => state.claim_chat_id(chat),
        _ => state.chat_id() == chat,
    };
    if !authorized {
        warn!(
            chat,
            user = ?msg.from.as_ref().map(|u| u.id.0),
            text = msg.text().unwrap_or_default(),
            "telegram: ignoring command from unregistered chat"
        );
        return Ok(());
    }

    match cmd {
        Command::Start => {
            bot.send_message(
                msg.chat.id,
                format!(
                    "Dogitor connected ✅\nChat id: {chat}\nThis is now the only chat the bot will answer. \
                     Add the id to config.toml ([telegram] chat_id) to keep it across restarts."
                ),
            )
            .await?;
        }
        Command::Status => {
            let armed = if state.armed() { "ARMED" } else { "disarmed" };
            let last = state
                .last_trigger()
                .map(|t| t.format("%Y-%m-%d %H:%M:%S").to_string())
                .unwrap_or_else(|| "never".into());
            bot.send_message(
                msg.chat.id,
                format!(
                    "Status: {armed}\nCurrent level: {:.1} dBFS\nTrigger: {:.1} dBFS\nLast trigger: {last}",
                    state.current_db(),
                    state.trigger_db()
                ),
            )
            .await?;
        }
        Command::Arm => {
            state.set_armed(true);
            broadcast_state(&events_tx, &state);
            bot.send_message(msg.chat.id, "Monitoring ARMED 🔔").await?;
        }
        Command::Disarm => {
            state.set_armed(false);
            broadcast_state(&events_tx, &state);
            bot.send_message(msg.chat.id, "Monitoring disarmed 🔕")
                .await?;
        }
        Command::Threshold(v) if !SharedState::valid_trigger_db(v) => {
            bot.send_message(
                msg.chat.id,
                "Threshold must be a number between -120 and 0 dBFS, e.g. /threshold -30",
            )
            .await?;
        }
        Command::Threshold(v) => {
            state.set_trigger_db(v);
            broadcast_state(&events_tx, &state);
            bot.send_message(msg.chat.id, format!("Trigger threshold set to {v:.1} dBFS"))
                .await?;
        }
        Command::Snapshot => {
            let _ = rec_tx.send(RecorderCommand::Snapshot);
            bot.send_message(msg.chat.id, "📸 Capturing…").await?;
        }
        Command::Last => match state.last_clip() {
            Some(p) => {
                bot.send_video(msg.chat.id, InputFile::file(p)).await?;
            }
            None => {
                bot.send_message(msg.chat.id, "No clips yet.").await?;
            }
        },
    }
    Ok(())
}
