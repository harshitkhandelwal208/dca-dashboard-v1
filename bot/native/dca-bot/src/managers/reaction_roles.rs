//! Dashboard-managed reaction-role messages (`reactionRoleManager.js` + the dashboard sync twin).

use crate::app::App;
use crate::logs::{append_only, log_action, LogEntry};
use crate::util::*;
use dca_state::config::{update_config, DashboardConfig, ReactionRoleGroup};
use serde::Serialize;
use serde_json::json;
use serenity::all::*;
use std::sync::OnceLock;
use tokio::sync::Mutex;

#[derive(Serialize, Default, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct GroupResult {
    pub id: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skipped: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub channel_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reaction_errors: Option<Vec<String>>,
}

fn skipped(group: &ReactionRoleGroup, reason: impl Into<String>) -> GroupResult {
    GroupResult { id: group.id.clone(), name: group.name.clone(), skipped: Some(true), reason: Some(reason.into()), ..Default::default() }
}

fn normalize_for_search(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub fn reaction_from_config(emoji: &str) -> ReactionType {
    ReactionType::try_from(emoji.to_string()).unwrap_or_else(|_| ReactionType::Unicode(emoji.to_string()))
}

async fn find_existing_bot_message(app: &App, channel: ChannelId, bot: UserId, content: &str) -> Option<Message> {
    let messages = channel.messages(&app.http, GetMessages::new().limit(25)).await.ok()?;
    let expected = normalize_for_search(content);
    let first_line = normalize_for_search(content.split('\n').next().unwrap_or(""));
    messages
        .iter()
        .find(|m| m.author.id == bot && normalize_for_search(&m.content) == expected)
        .or_else(|| messages.iter().find(|m| m.author.id == bot && first_line.chars().count() >= 16 && normalize_for_search(&m.content).contains(&first_line)))
        .cloned()
}

async fn ensure_group(app: &App, bot: UserId, group: &ReactionRoleGroup) -> GroupResult {
    if !group.enabled {
        return skipped(group, "disabled");
    }
    let Some(channel) = channel_id(&group.channel_id) else { return skipped(group, "missing channel") };
    if group.options.is_empty() {
        return skipped(group, "missing options");
    }
    match channel.to_channel(&app.http).await {
        Ok(Channel::Guild(_)) => {}
        Ok(_) => return skipped(group, "channel is not text based"),
        Err(error) => return skipped(group, error.to_string()),
    }

    let mut message: Option<Message> = None;
    if let Some(id) = snowflake(&group.message_id) {
        message = channel.message(&app.http, MessageId::new(id)).await.ok().filter(|m| m.author.id == bot);
    }
    if message.is_none() {
        message = find_existing_bot_message(app, channel, bot, &group.message).await;
    }
    let created = message.is_none();
    let message = match message {
        Some(m) if m.content != group.message => match channel.edit_message(&app.http, m.id, EditMessage::new().content(&group.message)).await {
            Ok(m) => m,
            Err(error) => return skipped(group, error.to_string()),
        },
        Some(m) => m,
        None => match channel.send_message(&app.http, CreateMessage::new().content(&group.message)).await {
            Ok(m) => m,
            Err(error) => return skipped(group, error.to_string()),
        },
    };

    let mut errors = Vec::new();
    for option in &group.options {
        if let Err(error) = message.react(&app.http, reaction_from_config(&option.emoji)).await {
            errors.push(format!("{}: {error}", option.emoji));
        }
    }
    GroupResult {
        id: group.id.clone(),
        name: group.name.clone(),
        message_id: Some(message.id.to_string()),
        channel_id: Some(group.channel_id.clone()),
        created: Some(created),
        reaction_errors: Some(errors),
        ..Default::default()
    }
}

fn sync_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

/// Post/refresh every reaction-role message. Only one sync runs at a time.
pub async fn sync_reaction_roles(app: &App, from_dashboard: bool) -> Result<(DashboardConfig, Vec<GroupResult>), String> {
    let _guard = sync_lock().lock().await;
    let bot = app.ensure_bot_id().await.map_err(|e| e.to_string())?;
    let config = app.config().await;
    let mut results = Vec::new();
    let mut updates: Vec<(String, String)> = Vec::new();
    for group in &config.reaction_roles {
        let result = ensure_group(app, bot, group).await;
        if let Some(id) = &result.message_id {
            if *id != group.message_id {
                updates.push((group.id.clone(), id.clone()));
            }
        }
        results.push(result);
    }
    let next = if updates.is_empty() {
        config
    } else {
        update_config(&app.store, |c| {
            for (gid, mid) in &updates {
                if let Some(g) = c.reaction_roles.iter_mut().find(|g| &g.id == gid) {
                    g.message_id = mid.clone();
                }
            }
        })
        .await?
        .0
    };
    if from_dashboard {
        append_only(
            app,
            LogEntry::new("reactionRole", "Reaction Roles Synced From Dashboard", format!("{} reaction role message(s) were synced.", results.iter().filter(|r| r.skipped != Some(true)).count()))
                .guild(&next.bot.guild_id)
                .meta(json!({ "results": results })),
        )
        .await;
    }
    Ok((next, results))
}

fn emoji_matches(expected: &str, emoji: &ReactionType) -> bool {
    let expected = expected.trim();
    if expected.is_empty() {
        return false;
    }
    match emoji {
        ReactionType::Unicode(s) => s == expected || s.trim_end_matches('\u{fe0f}') == expected.trim_end_matches('\u{fe0f}'),
        ReactionType::Custom { id, name, animated } => {
            let name = name.clone().unwrap_or_default();
            let keys = [name.clone(), id.to_string(), format!("{name}:{id}"), format!("<{}:{name}:{id}>", if *animated { "a" } else { "" })];
            keys.iter().any(|k| k == expected)
        }
        _ => false,
    }
}

pub async fn handle_reaction(app: &App, reaction: &Reaction, add: bool) {
    let Some(user_id) = reaction.user_id else { return };
    let Some(guild_id) = reaction.guild_id else { return };
    if Some(user_id) == app.bot_user_id() {
        return;
    }
    if let Some(member) = &reaction.member {
        if member.user.bot {
            return;
        }
    }
    let config = app.config().await;
    let Some(group) = config.reaction_roles.iter().find(|g| g.enabled && g.message_id == reaction.message_id.to_string()) else { return };
    let Some(option) = group.options.iter().find(|o| emoji_matches(&o.emoji, &reaction.emoji)) else { return };
    let Some(role) = role_id(&option.role_id) else { return };
    let reason = format!("Reaction role: {}", group.name);

    let result = if add { app.http.add_member_role(guild_id, user_id, role, Some(&reason)).await } else { app.http.remove_member_role(guild_id, user_id, role, Some(&reason)).await };
    if let Err(error) = result {
        tracing::warn!("Reaction role update failed: {error}");
        return;
    }
    if add && !config.recruitment.community_rules_role_id.is_empty() && option.role_id == config.recruitment.community_rules_role_id {
        super::team_roles::process_due_assignments(app, Some(user_id), true).await;
    }
    let tag = match &reaction.member {
        Some(m) => display_tag(&m.user),
        None => user_id.to_string(),
    };
    log_action(
        app,
        LogEntry::new(
            "reactionRole",
            if add { "Reaction Role Added" } else { "Reaction Role Removed" },
            format!("<@{}> {} <@&{}> from **{}**.", user_id, if add { "received" } else { "lost" }, option.role_id, group.name),
        )
        .guild(guild_id)
        .actor(user_id, tag)
        .meta(json!({ "groupId": group.id, "roleId": option.role_id, "emoji": option.emoji, "messageId": reaction.message_id.to_string() })),
    )
    .await;
}
