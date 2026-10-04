//! Reminder delivery background job.

use dca_state::reminder_store::ReminderStore;
use serenity::all::{ChannelId, Http};
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;

pub async fn start_reminder_scheduler(http: Arc<Http>) {
    tokio::spawn(async move {
        loop {
            sleep(Duration::from_secs(10)).await;
            let now = chrono::Utc::now().timestamp();
            let due_reminders = ReminderStore::pop_due_reminders(now);

            for reminder in due_reminders {
                if let Ok(channel_id) = reminder.channel_id.parse::<u64>() {
                    let ch = ChannelId::new(channel_id);
                    let content = format!(
                        "⏰ <@{}> Reminder: {}",
                        reminder.user_id, reminder.message
                    );
                    let _ = ch.say(&http, content).await;
                }
            }
        }
    });
}
