//! `/membercount`, `/teamcount`, `/updatecount`: dashboard-managed team player counts.

use crate::app::App;
use crate::logs::{log_action, LogEntry};
use crate::managers::member_count::{find_team, normalize_team_name, sync_member_count_message};
use crate::opts;
use crate::responder::{ReplyData, Responder};
use crate::util::*;
use dca_state::config::update_config;
use serde_json::json;
use serenity::all::*;

fn guard(r: &Responder) -> bool {
    r.permissions().contains(Permissions::MANAGE_GUILD) || r.permissions().contains(Permissions::ADMINISTRATOR)
}

async fn announce_sync(app: &App, r: &Responder, saved_prefix: bool) -> BotResult<()> {
    let sync = sync_member_count_message(app, false, "").await;
    let content = if sync.skipped {
        if saved_prefix {
            format!("Saved, but sync skipped: {}", sync.reason)
        } else {
            format!("Skipped: {}", sync.reason)
        }
    } else if saved_prefix {
        format!("Saved and updated <#{}>.", sync.channel_id)
    } else {
        format!("Member count message {} in <#{}>.", if sync.created { "created" } else { "updated" }, sync.channel_id)
    };
    r.edit_reply(ReplyData::text(content)).await
}

pub async fn slash_membercount(app: &App, r: &Responder, cmd: &CommandInteraction) -> BotResult<()> {
    if !guard(r) {
        return r.reply(ReplyData::text("You don't have permission to use this command.").ephemeral()).await;
    }
    let options = cmd.data.options();
    let Some((sub, sub_opts)) = opts::subcommand(&options) else { return Ok(()) };
    r.defer(true).await?;
    match sub {
        "sync" => announce_sync(app, r, false).await,
        "set" => {
            let team_name = opts::string(sub_opts, "team").unwrap_or("");
            let players = opts::integer(sub_opts, "players").unwrap_or(0).clamp(0, 999) as u32;
            let status = opts::string(sub_opts, "status");
            let config = app.config().await;
            let Some(team) = find_team(&config.member_counts, team_name).cloned() else {
                return r.edit_reply(ReplyData::text(format!("Team **{team_name}** is not configured. Add it from the dashboard first."))).await;
            };
            let status_owned = status.map(str::to_string);
            let tid = team.id.clone();
            update_config(&app.store, |c| {
                if let Some(t) = c.member_counts.teams.iter_mut().find(|t| t.id == tid) {
                    t.players = players;
                    if let Some(s) = &status_owned {
                        if !s.is_empty() {
                            t.recruitment_status = s.clone();
                        }
                    }
                }
            })
            .await?;
            log_action(
                app,
                LogEntry::new(
                    "memberCount",
                    "Member Count Edited",
                    format!("<@{}> set **{}** to **{players}** players{}.", r.user().id, team.name, status.map(|s| format!(", {s}")).unwrap_or_default()),
                )
                .guild(cmd.guild_id.map(|g| g.to_string()).unwrap_or_default())
                .actor(r.user().id, display_tag(r.user()))
                .meta(json!({ "teamId": team.id, "players": players, "status": status.unwrap_or("") })),
            )
            .await;
            announce_sync(app, r, true).await
        }
        "list" => {
            let config = app.config().await;
            let lines: Vec<String> = config.member_counts.teams.iter().map(|t| format!("**{}** - {} players, {}", t.name, t.players, t.recruitment_status)).collect();
            r.edit_reply(ReplyData::text(if lines.is_empty() { "No teams are configured.".to_string() } else { lines.join("\n") })).await
        }
        _ => Ok(()),
    }
}

pub async fn slash_teamcount(app: &App, r: &Responder, cmd: &CommandInteraction) -> BotResult<()> {
    if !guard(r) {
        return r.reply(ReplyData::text("You don't have permission to use this command.").ephemeral()).await;
    }
    let options = cmd.data.options();
    r.defer(true).await?;
    let team_name = opts::string(&options, "team").unwrap_or("");
    let players = opts::integer(&options, "players").unwrap_or(0).clamp(0, 999) as u32;
    let status = opts::string(&options, "status");
    let config = app.config().await;
    let Some(team) = find_team(&config.member_counts, team_name).cloned() else {
        return r.edit_reply(ReplyData::text(format!("Team **{team_name}** is not configured."))).await;
    };
    let status_owned = status.map(str::to_string);
    let tid = team.id.clone();
    update_config(&app.store, |c| {
        if let Some(t) = c.member_counts.teams.iter_mut().find(|t| t.id == tid) {
            t.players = players;
            if let Some(s) = &status_owned {
                if !s.is_empty() {
                    t.recruitment_status = s.clone();
                }
            }
        }
    })
    .await?;
    log_action(
        app,
        LogEntry::new("memberCount", "Team Count Updated", format!("<@{}> set **{}** to **{players}** players.", r.user().id, team.name))
            .guild(cmd.guild_id.map(|g| g.to_string()).unwrap_or_default())
            .actor(r.user().id, display_tag(r.user()))
            .meta(json!({ "teamId": team.id, "players": players, "status": status.unwrap_or("") })),
    )
    .await;
    announce_sync(app, r, true).await
}

pub async fn slash_updatecount(app: &App, r: &Responder, cmd: &CommandInteraction) -> BotResult<()> {
    if !guard(r) {
        return r.reply(ReplyData::text("You don't have permission to use this command.").ephemeral()).await;
    }
    let options = cmd.data.options();
    r.defer(true).await?;
    let team_name = opts::string(&options, "team").unwrap_or("");
    let field = opts::string(&options, "field").unwrap_or("players");
    let value = opts::string(&options, "value").unwrap_or("");
    let config = app.config().await;
    let target = normalize_team_name(team_name);
    let found = config.member_counts.teams.iter().find(|t| std::iter::once(&t.name).chain(t.aliases.iter()).any(|n| normalize_team_name(n) == target)).cloned();
    let Some(team) = found else {
        return r.edit_reply(ReplyData::text(format!("Team **{team_name}** is not configured. Add it from the dashboard first."))).await;
    };
    let mut players_value: Option<u32> = None;
    if field == "players" {
        match value.trim().parse::<i64>() {
            Ok(n) if n >= 0 => players_value = Some(n as u32),
            _ => return r.edit_reply(ReplyData::text("Please provide a valid non-negative player count.")).await,
        }
    }
    let (tid, value_owned) = (team.id.clone(), value.to_string());
    update_config(&app.store, |c| {
        if let Some(t) = c.member_counts.teams.iter_mut().find(|t| t.id == tid) {
            match players_value {
                Some(p) => t.players = p,
                None => t.recruitment_status = value_owned.clone(),
            }
        }
    })
    .await?;
    log_action(
        app,
        LogEntry::new("memberCount", "Member Count Edited", format!("<@{}> updated **{}** {field} to **{value}**.", r.user().id, team.name))
            .guild(cmd.guild_id.map(|g| g.to_string()).unwrap_or_default())
            .actor(r.user().id, display_tag(r.user()))
            .meta(json!({ "teamId": team.id, "field": field, "value": value })),
    )
    .await;
    announce_sync(app, r, true).await
}
