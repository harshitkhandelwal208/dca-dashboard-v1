//! Recruitment "team join ban list" embeds (`recruitmentBanPanel.js`).

use crate::app::App;
use crate::logs::{append_only, LogEntry};
use crate::managers::team_roles::{community_guild_id, recruitment_guild_id};
use crate::util::*;
use dca_state::config::{update_config, DashboardConfig};
use dca_state::models::RecruitmentBan;
use dca_state::stores::list_recruitment_bans;
use dca_state::util::parse_ms;
use serde_json::json;
use serenity::all::*;

fn build_embeds(bans: &[RecruitmentBan], guild_name: &str) -> Vec<CreateEmbed> {
    if bans.is_empty() {
        return vec![CreateEmbed::new().title("Team Join Ban List").description("No users are currently blocked from joining teams.").colour(Colour::new(0x2f855a)).timestamp(Timestamp::now())];
    }
    let groups: Vec<&[RecruitmentBan]> = bans.chunks(25).collect();
    let total = groups.len();
    groups
        .iter()
        .enumerate()
        .map(|(index, group)| {
            let mut embed = CreateEmbed::new()
                .title(if total > 1 { format!("Team Join Ban List ({}/{total})", index + 1) } else { "Team Join Ban List".to_string() })
                .description(format!("Recruitment bans for {}.", if guild_name.is_empty() { "the recruitment server" } else { guild_name }))
                .colour(Colour::new(0xb42318))
                .timestamp(Timestamp::now());
            for ban in group.iter() {
                let mut lines =
                    vec![format!("User: <@{}>", ban.user_id), format!("Discord ID: {}", ban.user_id), format!("Reason: {}", if ban.reason.is_empty() { "No reason provided." } else { &ban.reason })];
                if !ban.banned_by_id.is_empty() {
                    lines.push(format!("Banned by: <@{}> ({})", ban.banned_by_id, if ban.banned_by_tag.is_empty() { &ban.banned_by_id } else { &ban.banned_by_tag }));
                }
                if !ban.guild_id.is_empty() {
                    lines.push(format!("Server ID: {}", ban.guild_id));
                }
                if let Some(ms) = parse_ms(&ban.updated_at) {
                    lines.push(format!("Listed: <t:{}:R>", ms / 1000));
                }
                embed = embed.field(if ban.user_tag.is_empty() { "Unknown user".to_string() } else { ban.user_tag.clone() }, truncate(&lines.join("\n"), 1024), false);
            }
            embed
        })
        .collect()
}

pub struct BanListSync {
    pub skipped: bool,
    pub reason: String,
    pub count: usize,
    pub messages: usize,
    pub channel_id: String,
    pub config: Option<DashboardConfig>,
}

fn skipped(reason: &str) -> BanListSync {
    BanListSync { skipped: true, reason: reason.into(), count: 0, messages: 0, channel_id: String::new(), config: None }
}

pub async fn sync_recruitment_ban_list(app: &App, from_dashboard: bool) -> BanListSync {
    let config = app.config().await;
    let Some(channel) = channel_id(&config.recruitment.ban_list_channel_id) else {
        return skipped("Recruitment ban list channel is not configured.");
    };
    let Ok(info) = channel.to_channel(&app.http).await else { return skipped("Recruitment ban list channel is not text based.") };
    let Some(guild_channel) = info.guild() else { return skipped("Recruitment ban list channel is not text based.") };
    // The list is about bans made in the recruitment server, but it may be posted in the recruitment server or in the
    // community (staff) server; a channel anywhere else is a configuration mistake.
    let here = guild_channel.guild_id.to_string();
    let allowed = [recruitment_guild_id(&config), community_guild_id(&config)];
    if allowed.iter().any(|g| !g.is_empty()) && !allowed.contains(&here) {
        return skipped("Ban list channel is neither in the recruitment server nor in the community server.");
    }
    let Ok(bot) = app.ensure_bot_id().await else { return skipped("Bot user is unavailable.") };
    let guild_name = app.http.get_guild(guild_channel.guild_id).await.map(|g| g.name).unwrap_or_default();
    let bans = list_recruitment_bans(&app.store).await;
    let embeds = build_embeds(&bans, &guild_name);

    let mut old_ids: std::collections::VecDeque<String> = config.recruitment.ban_list_message_ids.iter().cloned().collect();
    let mut next_ids: Vec<String> = Vec::new();
    for embed in embeds {
        let existing = old_ids.pop_front();
        let mut message: Option<Message> = None;
        if let Some(id) = existing.as_deref().and_then(snowflake) {
            message = channel.message(&app.http, MessageId::new(id)).await.ok().filter(|m| m.author.id == bot);
        }
        let sent = match message {
            Some(m) => channel.edit_message(&app.http, m.id, EditMessage::new().embeds(vec![embed]).allowed_mentions(CreateAllowedMentions::new())).await,
            None => channel.send_message(&app.http, CreateMessage::new().embed(embed).allowed_mentions(CreateAllowedMentions::new())).await,
        };
        match sent {
            Ok(m) => next_ids.push(m.id.to_string()),
            Err(error) => return skipped(&format!("Could not update the ban list: {error}")),
        }
    }
    for stale in old_ids {
        if let Some(id) = snowflake(&stale) {
            if let Ok(m) = channel.message(&app.http, MessageId::new(id)).await {
                if m.author.id == bot {
                    let _ = m.delete(&app.http).await;
                }
            }
        }
    }
    let ids = next_ids.clone();
    let saved = update_config(&app.store, |c| c.recruitment.ban_list_message_ids = ids).await.ok().map(|(c, _)| c);
    if from_dashboard {
        append_only(
            app,
            LogEntry::new("system", "Recruitment Ban List Synced From Dashboard", format!("{} banned user(s) are listed in <#{}>.", bans.len(), guild_channel.id))
                .guild(guild_channel.guild_id)
                .meta(json!({ "channelId": guild_channel.id.to_string(), "messageIds": next_ids })),
        )
        .await;
    }
    BanListSync { skipped: false, reason: String::new(), count: bans.len(), messages: next_ids.len(), channel_id: guild_channel.id.to_string(), config: saved }
}
