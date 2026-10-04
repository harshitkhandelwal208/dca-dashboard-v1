//! `/remindme`, `/reminders`, `/cancelreminder`.

use crate::app::App;
use crate::opts;
use crate::responder::{ReplyData, Responder};
use crate::util::*;
use dca_state::stores::*;
use serenity::all::*;

pub async fn slash_remindme(app: &App, r: &Responder, cmd: &CommandInteraction) -> BotResult<()> {
    let options = cmd.data.options();
    let duration_input = opts::string(&options, "duration").unwrap_or("");
    let message = opts::string(&options, "message").unwrap_or("");
    let ms = parse_duration_ms(duration_input);
    let Some(ms) = ms.filter(|ms| *ms >= 5000) else {
        return r.reply(ReplyData::text("\u{26a0}\u{fe0f} Please enter a valid time (min 5s). Try `10m`, `1h`, etc.").ephemeral()).await;
    };
    let reminder = add_reminder(
        &app.store,
        &r.user().id.to_string(),
        &r.channel_id().to_string(),
        &r.guild_id().map(|g| g.to_string()).unwrap_or_default(),
        message,
        unix_ms() + ms as i64,
    )
    .await?;
    r.reply(ReplyData::text(format!("\u{2705} Reminder set! I\u{2019}ll remind you in **{duration_input}** (ID: {})", reminder.id))).await
}

pub async fn slash_reminders(app: &App, r: &Responder) -> BotResult<()> {
    let list = list_reminders(&app.store, Some(&r.user().id.to_string())).await;
    if list.is_empty() {
        return r.reply(ReplyData::text("\u{1f4ed} You have no active reminders!")).await;
    }
    let lines = list.iter().map(|x| format!("\u{2022} ID: {} \u{2013} \"{}\"", x.id, x.text)).collect::<Vec<_>>().join("\n");
    r.reply(ReplyData::text(format!("\u{1f4cb} Your reminders:\n{lines}"))).await
}

pub async fn slash_cancel(app: &App, r: &Responder, cmd: &CommandInteraction) -> BotResult<()> {
    let options = cmd.data.options();
    let id = opts::integer(&options, "id").unwrap_or(0).max(0) as u64;
    if remove_reminder(&app.store, &r.user().id.to_string(), id).await? {
        r.reply(ReplyData::text(format!("\u{2705} Reminder ID {id} cancelled!"))).await
    } else {
        r.reply(ReplyData::text(format!("\u{26a0}\u{fe0f} No reminder found with ID {id}."))).await
    }
}
