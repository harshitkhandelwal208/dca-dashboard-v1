//! Delayed team-role assignment queue (`teamRoleScheduler.js`).

use crate::app::App;
use crate::logs::{log_action, LogEntry};
use crate::managers::member_count::find_team;
use crate::util::*;
use dca_state::config::DashboardConfig;
use dca_state::models::TeamRoleAssignment;
use dca_state::stores::{list_assignments, queue_assignments, save_assignments};
use dca_state::util::{now_iso, now_ms, parse_ms};
use serde_json::json;
use serenity::all::*;
use std::sync::OnceLock;
use std::time::Duration;
use tokio::sync::Mutex;

const CHECK_INTERVAL: Duration = Duration::from_secs(60);
const RETRY_MS: i64 = 10 * 60 * 1000;
const MAX_ATTEMPTS: u32 = 2016;

pub fn community_guild_id(config: &DashboardConfig) -> String {
    [&config.bot.community_guild_id, &config.bot.guild_id, &std::env::var("COMMUNITY_GUILD_ID").unwrap_or_default(), &std::env::var("DISCORD_GUILD_ID").unwrap_or_default()]
        .iter()
        .find(|v| !v.is_empty())
        .map(|v| v.to_string())
        .unwrap_or_default()
}

pub fn recruitment_guild_id(config: &DashboardConfig) -> String {
    [&config.bot.recruitment_guild_id, &std::env::var("RECRUITMENT_GUILD_ID").unwrap_or_default(), &config.bot.guild_id, &std::env::var("DISCORD_GUILD_ID").unwrap_or_default()]
        .iter()
        .find(|v| !v.is_empty())
        .map(|v| v.to_string())
        .unwrap_or_default()
}

fn lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

pub async fn queue_team_role_assignment(app: &App, user: UserId, team_name: &str, ticket_id: &str, actor: Option<&User>) -> Result<usize, String> {
    if team_name.is_empty() {
        return Err("Missing user or team.".into());
    }
    let config = app.config().await;
    let team = find_team(&config.member_counts, team_name).ok_or_else(|| format!("Team {team_name} is not configured."))?.clone();

    let mut wanted: Vec<(&str, String, String, u32, String)> = Vec::new(); // type, guild, role, delay, requires
    if team.recruitment_role_auto_assign_enabled && !team.recruitment_role_id.is_empty() {
        wanted.push(("recruitment", recruitment_guild_id(&config), team.recruitment_role_id.clone(), team.recruitment_role_delay_minutes, String::new()));
    }
    let community_role = if team.community_role_id.is_empty() { team.role_id.clone() } else { team.community_role_id.clone() };
    if (team.community_role_auto_assign_enabled || team.auto_assign_enabled) && !community_role.is_empty() {
        let delay = if team.community_role_delay_minutes > 0 { team.community_role_delay_minutes } else { team.auto_assign_delay_minutes };
        wanted.push(("community", community_guild_id(&config), community_role, delay, config.recruitment.community_rules_role_id.clone()));
    }
    wanted.retain(|w| !w.1.is_empty() && !w.2.is_empty());
    if wanted.is_empty() {
        return Err(format!("Auto role assignment is not configured for {}.", team.name));
    }

    let now = now_ms();
    let mut items = Vec::new();
    for (kind, guild, role, delay, requires) in wanted {
        let stamp = if ticket_id.is_empty() { now.to_string() } else { ticket_id.to_string() };
        let run_at = chrono::DateTime::from_timestamp_millis(now + delay as i64 * 60_000).map(|d| d.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)).unwrap_or_else(now_iso);
        items.push(TeamRoleAssignment {
            id: format!("{kind}-{stamp}-{user}-{}", team.id),
            kind: kind.into(),
            status: "pending".into(),
            user_id: user.to_string(),
            team_id: team.id.clone(),
            team_name: team.name.clone(),
            role_id: role,
            guild_id: guild,
            requires_role_id: requires,
            ticket_id: ticket_id.into(),
            attempts: 0,
            created_at: now_iso(),
            not_before_at: run_at.clone(),
            run_at,
            ..Default::default()
        });
    }
    let count = items.len();
    let due_now = items.iter().any(|i| parse_ms(&i.run_at).unwrap_or(0) <= now);
    queue_assignments(&app.store, items.clone()).await?;

    let mut entry = LogEntry::new("ticket", "Team Role Assignments Queued", format!("<@{user}> has {count} pending **{}** role assignment(s).", team.name))
        .guild(recruitment_guild_id(&config))
        .target(user, "")
        .meta(json!({ "assignmentIds": items.iter().map(|i| i.id.clone()).collect::<Vec<_>>(), "teamId": team.id }));
    if let Some(a) = actor {
        entry = entry.actor(a.id, display_tag(a));
    }
    log_action(app, entry).await;

    if due_now {
        process_due_assignments(app, None, false).await;
    }
    Ok(count)
}

async fn try_assignment(app: &App, a: &TeamRoleAssignment) -> Result<(), String> {
    let guild = guild_id(&a.guild_id).ok_or("guild unavailable")?;
    app.http.get_guild(guild).await.map_err(|_| "guild unavailable".to_string())?;
    let user = user_id(&a.user_id).ok_or("bad user")?;
    let member = app.http.get_member(guild, user).await.map_err(|_| format!("member not in {} guild", if a.kind.is_empty() { "target" } else { &a.kind }))?;
    if let Some(req) = role_id(&a.requires_role_id) {
        if !member.roles.contains(&req) {
            return Err("waiting for configured rules role".into());
        }
    }
    let role = role_id(&a.role_id).ok_or("bad role")?;
    if !member.roles.contains(&role) {
        app.http.add_member_role(guild, user, role, Some(&format!("Recruitment accepted for {}", a.team_name))).await.map_err(|e| e.to_string())?;
    }
    Ok(())
}

pub async fn process_due_assignments(app: &App, only_user: Option<UserId>, ignore_run_at: bool) {
    if !app.is_ready() {
        return;
    }
    let _guard = lock().lock().await;
    let now = now_ms();
    let mut list = list_assignments(&app.store).await;
    let mut changed: Vec<TeamRoleAssignment> = Vec::new();
    for a in list.iter_mut() {
        if a.status != "pending" {
            continue;
        }
        if let Some(u) = only_user {
            if a.user_id != u.to_string() {
                continue;
            }
        }
        let not_before = parse_ms(if a.not_before_at.is_empty() { &a.run_at } else { &a.not_before_at }).unwrap_or(0);
        if not_before > now {
            continue;
        }
        if !ignore_run_at && parse_ms(&a.run_at).unwrap_or(0) > now {
            continue;
        }
        let result = try_assignment(app, a).await;
        a.attempts += 1;
        a.last_attempt_at = now_iso();
        match result {
            Ok(()) => {
                a.status = "complete".into();
                a.completed_at = now_iso();
            }
            Err(reason) => {
                if a.attempts >= MAX_ATTEMPTS {
                    a.status = "expired".into();
                    a.failed_reason = reason;
                } else {
                    a.run_at = chrono::DateTime::from_timestamp_millis(now + RETRY_MS).map(|d| d.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)).unwrap_or_default();
                    a.last_reason = reason;
                }
            }
        }
        changed.push(a.clone());
    }
    if !changed.is_empty() {
        let _ = save_assignments(&app.store, changed).await;
    }
}

pub fn start_scheduler(app: std::sync::Arc<App>) {
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(5)).await;
        loop {
            process_due_assignments(&app, None, false).await;
            tokio::time::sleep(CHECK_INTERVAL).await;
        }
    });
}

pub async fn handle_community_member_join(app: &App, member: &Member) {
    let config = app.config().await;
    if member.guild_id.to_string() != community_guild_id(&config) {
        return;
    }
    process_due_assignments(app, Some(member.user.id), false).await;
}
