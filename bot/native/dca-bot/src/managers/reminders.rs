//! Reminder delivery. Reminders are persisted now (the old bot kept them in memory and lost them on restart).

use crate::app::App;
use crate::util::*;
use dca_state::stores::take_due_reminders;
use serenity::all::*;
use std::sync::Arc;
use std::time::Duration;

async fn deliver(app: &App, user: UserId, channel: Option<ChannelId>, text: &str) {
    let dm = async {
        let dm = user.create_dm_channel(&app.http).await?;
        dm.say(&app.http, format!("\u{23f0} Reminder: {text}")).await
    }
    .await;
    if dm.is_err() {
        if let Some(channel) = channel {
            let _ =
                channel.send_message(&app.http, CreateMessage::new().content(format!("<@{user}> \u{23f0} Reminder: {text}")).allowed_mentions(CreateAllowedMentions::new().users(vec![user]))).await;
        }
    }
}

pub fn start_scheduler(app: Arc<App>) {
    tokio::spawn(async move {
        loop {
            let due = take_due_reminders(&app.store, unix_ms()).await;
            for reminder in due {
                if let Some(user) = user_id(&reminder.user_id) {
                    deliver(&app, user, channel_id(&reminder.channel_id), &reminder.text).await;
                }
            }
            tokio::time::sleep(Duration::from_secs(5)).await;
        }
    });
}
