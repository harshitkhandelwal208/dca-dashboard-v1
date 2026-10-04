//! Team member-count message (`memberCountManager.js` + the dashboard's sync twin).

use crate::app::App;
use crate::logs::{log_action, LogEntry};
use crate::util::*;
use dca_state::config::{update_config, DashboardConfig, MemberCountsConfig, MemberTeam};
use serde_json::json;
use serenity::all::*;
use unicode_normalization::UnicodeNormalization;

pub fn normalize_team_name(value: &str) -> String {
    value.nfkd().filter(|c| !('\u{0300}'..='\u{036f}').contains(c)).filter(|c| c.is_ascii_alphanumeric()).flat_map(|c| c.to_lowercase()).collect()
}

pub fn find_team<'a>(counts: &'a MemberCountsConfig, name: &str) -> Option<&'a MemberTeam> {
    let target = normalize_team_name(name);
    counts.teams.iter().find(|team| std::iter::once(&team.name).chain(team.aliases.iter()).any(|n| normalize_team_name(n) == target))
}

pub fn build_embed(counts: &MemberCountsConfig) -> CreateEmbed {
    let teams: Vec<&MemberTeam> = counts.teams.iter().take(25).collect();
    let total: u32 = teams.iter().map(|t| t.players).sum();
    let mut embed = CreateEmbed::new()
        .title(if counts.title.is_empty() { "Member Count" } else { &counts.title })
        .description(format!("**Tracked teams:** {}\n**Total players:** {}", teams.len(), total))
        .colour(Colour::new(0x0f766e))
        .footer(CreateEmbedFooter::new("Use /teamcount or the dashboard to update these numbers."))
        .timestamp(Timestamp::now());
    for team in teams {
        let mut value = vec![format!("**Players:** {}", team.players), format!("**Recruitment:** {}", team.recruitment_status)];
        if !team.aliases.is_empty() {
            value.push(format!("**Aliases:** {}", team.aliases.join(", ")));
        }
        embed = embed.field(format!("{}{}", team.name, if team.division.is_empty() { String::new() } else { format!(" - {}", team.division) }), value.join("\n"), true);
    }
    embed
}

pub struct MemberCountSync {
    pub skipped: bool,
    pub reason: String,
    pub created: bool,
    pub channel_id: String,
    pub message_id: String,
    pub config: Option<DashboardConfig>,
}

fn skipped(reason: &str) -> MemberCountSync {
    MemberCountSync { skipped: true, reason: reason.into(), created: false, channel_id: String::new(), message_id: String::new(), config: None }
}

/// Create or update the member-count message. `origin` is "" for the bot and " From Dashboard" for the dashboard.
pub async fn sync_member_count_message(app: &App, silent: bool, origin: &str) -> MemberCountSync {
    let config = app.config().await;
    let counts = &config.member_counts;
    if !counts.enabled {
        return skipped("Member counts are disabled.");
    }
    let Some(channel) = channel_id(&counts.channel_id) else { return skipped("Member count channel is not configured.") };
    let Ok(channel_info) = channel.to_channel(&app.http).await else { return skipped("Member count channel is not text based.") };
    let Some(guild_channel) = channel_info.guild() else { return skipped("Member count channel is not text based.") };
    let Ok(bot_id) = app.ensure_bot_id().await else { return skipped("Bot user is unavailable.") };

    let mut message: Option<Message> = None;
    if let Some(id) = snowflake(&counts.message_id) {
        message = channel.message(&app.http, MessageId::new(id)).await.ok().filter(|m| m.author.id == bot_id);
    }
    let created = message.is_none();
    let message = match message {
        Some(m) => match channel.edit_message(&app.http, m.id, EditMessage::new().content("").embeds(vec![build_embed(counts)])).await {
            Ok(m) => m,
            Err(error) => return skipped(&format!("Could not edit the member count message: {error}")),
        },
        None => match channel.send_message(&app.http, CreateMessage::new().embed(build_embed(counts))).await {
            Ok(m) => m,
            Err(error) => return skipped(&format!("Could not post the member count message: {error}")),
        },
    };

    let mut saved = None;
    if message.id.to_string() != counts.message_id {
        let new_id = message.id.to_string();
        saved = update_config(&app.store, |c| c.member_counts.message_id = new_id).await.ok().map(|(c, _)| c);
    }
    if !silent {
        log_action(
            app,
            LogEntry::new(
                "memberCount",
                if created {
                    format!("Member Count Message Created{origin}")
                } else if origin.is_empty() {
                    "Member Count Message Updated".to_string()
                } else {
                    "Member Count Message Synced From Dashboard".to_string()
                },
                format!("{} teams are listed in <#{}>.", counts.teams.len(), guild_channel.id),
            )
            .guild(guild_channel.guild_id)
            .meta(json!({ "channelId": guild_channel.id.to_string(), "messageId": message.id.to_string() })),
        )
        .await;
    }
    MemberCountSync { skipped: false, reason: String::new(), created, channel_id: guild_channel.id.to_string(), message_id: message.id.to_string(), config: saved.or(Some(config)) }
}

/// Add `delta` to a team's player count (recruitment accept) and refresh the message.
pub async fn increment_team_count(app: &App, team_name: &str, delta: i32, actor: Option<&User>) -> Option<String> {
    if team_name.is_empty() {
        return None;
    }
    let config = app.config().await;
    if !config.member_counts.update_on_recruitment_close {
        return None;
    }
    let team = find_team(&config.member_counts, team_name)?.clone();
    let id = team.id.clone();
    let _ = update_config(&app.store, |c| {
        if let Some(t) = c.member_counts.teams.iter_mut().find(|t| t.id == id) {
            t.players = (t.players as i64 + delta as i64).max(0) as u32;
        }
    })
    .await;
    let mut entry = LogEntry::new("memberCount", "Member Count Updated", format!("{} changed by {}.", team.name, if delta > 0 { format!("+{delta}") } else { delta.to_string() }))
        .guild(&config.bot.guild_id)
        .meta(json!({ "teamId": team.id, "teamName": team.name, "delta": delta }));
    if let Some(a) = actor {
        entry = entry.actor(a.id, display_tag(a));
    }
    log_action(app, entry).await;
    if app.is_ready() {
        sync_member_count_message(app, true, "").await;
    }
    Some(team.name)
}
