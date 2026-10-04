//! `logAction`: store an audit entry and mirror it into the configured log channel.

use crate::app::App;
use crate::util::*;
use dca_state::config::DashboardConfig;
use dca_state::models::BotLog;
use dca_state::stores::append_bot_log;
use serde_json::{json, Value};
use serenity::all::*;

#[derive(Default, Clone)]
pub struct LogEntry {
    pub kind: &'static str,
    pub title: String,
    pub message: String,
    pub guild_id: String,
    pub actor_id: String,
    pub actor_tag: String,
    pub target_id: String,
    pub target_tag: String,
    pub metadata: Value,
}

impl LogEntry {
    pub fn new(kind: &'static str, title: impl Into<String>, message: impl Into<String>) -> LogEntry {
        LogEntry { kind, title: title.into(), message: message.into(), metadata: json!({}), ..Default::default() }
    }
    pub fn guild(mut self, id: impl ToString) -> Self {
        self.guild_id = id.to_string();
        self
    }
    pub fn actor(mut self, id: impl ToString, tag: impl ToString) -> Self {
        self.actor_id = id.to_string();
        self.actor_tag = tag.to_string();
        self
    }
    pub fn target(mut self, id: impl ToString, tag: impl ToString) -> Self {
        self.target_id = id.to_string();
        self.target_tag = tag.to_string();
        self
    }
    pub fn meta(mut self, metadata: Value) -> Self {
        self.metadata = metadata;
        self
    }
}

fn event_enabled(config: &DashboardConfig, kind: &str) -> bool {
    if !config.logging.enabled {
        return false;
    }
    let e = &config.logging.events;
    match kind {
        "ticket" | "tickets" => e.tickets,
        "memberCount" | "memberCounts" => e.member_counts,
        "youtube" => e.youtube,
        "reactionRole" | "reactionRoles" => e.reaction_roles,
        "system" => e.system,
        _ => true,
    }
}

async fn send_to_discord(app: &App, log: &BotLog, config: &DashboardConfig) {
    let Some(channel) = channel_id(&config.logging.channel_id) else { return };
    if !event_enabled(config, &log.kind) {
        return;
    }
    let mut embed = CreateEmbed::new()
        .title(&log.title)
        .description(if log.message.is_empty() { "No details provided." } else { &log.message })
        .colour(Colour::new(0x0f766e));
    if let Ok(ts) = Timestamp::parse(&log.created_at) {
        embed = embed.timestamp(ts);
    }
    if !log.actor_id.is_empty() {
        embed = embed.field("Actor", format!("<@{}>", log.actor_id), true);
    }
    if !log.target_id.is_empty() {
        embed = embed.field("Target", format!("<@{}>", log.target_id), true);
    }
    if let Some(thread) = log.metadata.get("threadId").and_then(Value::as_str).filter(|s| !s.is_empty()) {
        embed = embed.field("Thread", format!("<#{thread}>"), true);
    }
    let content = if log.title == "Member Left" && !log.target_id.is_empty() { format!("<@{}>", log.target_id) } else { String::new() };
    let mentions = if content.is_empty() {
        CreateAllowedMentions::new()
    } else {
        user_id(&log.target_id).map(|u| CreateAllowedMentions::new().users(vec![u])).unwrap_or_default()
    };
    let mut message = CreateMessage::new().embed(embed).allowed_mentions(mentions);
    if !content.is_empty() {
        message = message.content(content);
    }
    if let Err(error) = channel.send_message(&app.http, message).await {
        tracing::warn!("Failed to send log entry to Discord: {error}");
    }
}

pub async fn log_action(app: &App, entry: LogEntry) -> BotLog {
    let log = BotLog {
        kind: entry.kind.to_string(),
        title: entry.title,
        message: entry.message,
        guild_id: entry.guild_id,
        actor_id: entry.actor_id,
        actor_tag: entry.actor_tag,
        target_id: entry.target_id,
        target_tag: entry.target_tag,
        metadata: entry.metadata,
        ..Default::default()
    };
    let saved = match append_bot_log(&app.store, log.clone()).await {
        Ok(saved) => saved,
        Err(error) => {
            tracing::warn!("Failed to store log entry: {error}");
            log
        }
    };
    let config = app.config().await;
    send_to_discord(app, &saved, &config).await;
    saved
}

/// Store only (used by the dashboard sync helpers that never mirrored to Discord).
pub async fn append_only(app: &App, entry: LogEntry) {
    let log = BotLog {
        kind: entry.kind.to_string(),
        title: entry.title,
        message: entry.message,
        guild_id: entry.guild_id,
        metadata: entry.metadata,
        ..Default::default()
    };
    let _ = append_bot_log(&app.store, log).await;
}
