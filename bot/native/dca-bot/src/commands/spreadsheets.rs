//! `/spreadsheets ...` (team-event spreadsheet sessions, reports and corrections).

use crate::app::App;
use crate::managers::spreadsheet::*;
use crate::opts;
use crate::responder::{ReplyData, Responder};
use crate::util::*;
use chrono::{DateTime, Utc};
use dca_core::metrics::session_date;
use dca_core::report::parse_anchor_date;
use dca_state::config::{DashboardConfig, SpreadsheetTeam};
use dca_state::models::{Correction, SpreadsheetSession};
use dca_state::stores::*;
use serenity::all::*;

fn fmt_local(value: &str) -> String {
    chrono::DateTime::parse_from_rfc3339(value).map(|d| d.format("%-m/%-d/%Y, %-I:%M:%S %p").to_string()).unwrap_or_else(|_| value.to_string())
}

async fn resolve_team(app: &App, r: &Responder, value: &str) -> BotResult<Option<(DashboardConfig, SpreadsheetTeam)>> {
    let config = app.config().await;
    let Some(team) = find_spreadsheet_team(&config, value).cloned() else {
        r.edit_reply(ReplyData::text(format!("Spreadsheet team **{value}** is not configured."))).await?;
        return Ok(None);
    };
    let roles = r.member().map(|m| m.roles.clone()).unwrap_or_default();
    if !can_access_team(&roles, r.permissions(), &team) {
        r.edit_reply(ReplyData::text("You do not have access to this team's spreadsheet data.")).await?;
        return Ok(None);
    }
    Ok(Some((config, team)))
}

async fn resolve_session(app: &App, team_id: &str, session_id: Option<&str>, statuses: &[&str]) -> Option<SpreadsheetSession> {
    if let Some(id) = session_id {
        let s = get_session(&app.store, id).await?;
        if s.team_id != team_id {
            return None;
        }
        if !statuses.is_empty() && !statuses.contains(&s.status.as_str()) {
            return None;
        }
        return Some(s);
    }
    latest_session(&app.store, team_id, statuses).await
}

fn preview_text(session: &SpreadsheetSession) -> String {
    let rows: Vec<String> = session
        .players
        .iter()
        .take(20)
        .map(|p| format!("#{} {} - {} - pts {} - score {}", p.rank, p.player_name, p.team_type, p.points.map(|v| v.to_string()).unwrap_or_default(), p.score.map(|v| v.to_string()).unwrap_or_default()))
        .collect();
    truncate(
        &[
            format!("Session: `{}`", session.id),
            format!("Event: **{}**", if !session.metadata.title.is_empty() { &session.metadata.title } else if !session.team_event_name.is_empty() { &session.team_event_name } else { "Team Event" }),
            format!("Players: **{}** ({} own, {} opponents)", session.players.len(), session.stats.own_players, session.stats.opponents),
            format!("Missing own players scored 0: **{}**", session.attendance.missing_players.len()),
            String::new(),
            if rows.is_empty() { "No rows parsed.".to_string() } else { rows.join("\n") },
        ]
        .join("\n"),
        1900,
    )
}

async fn resolve_anchor(app: &App, o: &[ResolvedOption<'_>], team: &SpreadsheetTeam) -> BotResult<DateTime<Utc>> {
    if let Some(value) = opts::string(o, "anchor_date") {
        if !value.is_empty() {
            return parse_anchor_date(value).map_err(|e| e.into());
        }
    }
    let latest = list_sessions(&app.store, &SessionFilter { team_id: Some(team.id.clone()), status: Some("processed".into()) }).await;
    Ok(latest.first().map(session_date).unwrap_or_else(Utc::now))
}

async fn reply_with_period_report(app: &App, r: &Responder, o: &[ResolvedOption<'_>], team: &SpreadsheetTeam, period: &str) -> BotResult<()> {
    let anchor = resolve_anchor(app, o, team).await?;
    let Some(report) = generate_period_report(app, team, period, anchor, None, None).await? else {
        return r.edit_reply(ReplyData::text(format!("No processed spreadsheet sessions were found for this {}.", if period == "weekly" { "week" } else { "month" }))).await;
    };
    let mut data = ReplyData::text(format!("{} report for **{}**.", if period == "weekly" { "Weekly" } else { "Monthly" }, team.name)).embed(build_period_report_embed(&report.model));
    data.files = report_attachments(&report).await;
    r.edit_reply(data).await?;
    cleanup_report_images(app, &report).await;
    Ok(())
}

async fn post_report_and_reply(app: &App, r: &Responder, o: &[ResolvedOption<'_>], team: &SpreadsheetTeam, period: &str, reason: &str) -> BotResult<()> {
    let anchor = resolve_anchor(app, o, team).await?;
    let result = send_period_report(app, team, period, ReportOptions { anchor, force: true, reason: reason.into() }).await;
    if let Some(reason) = result.skipped {
        return r.edit_reply(ReplyData::text(format!("No {period} report was posted: {reason}."))).await;
    }
    r.edit_reply(ReplyData::text(format!("{} report regenerated and posted to the configured output channel.", if period == "weekly" { "Weekly" } else { "Monthly" }))).await
}

async fn rebuild_if_missing(app: &App, session: SpreadsheetSession, paths: &[&str]) -> Result<SpreadsheetSession, String> {
    if paths.is_empty() || paths.iter().any(|p| p.is_empty() || !std::path::Path::new(p).exists()) {
        return rebuild_spreadsheet_session(app, &session.id).await;
    }
    Ok(session)
}

async fn summary_reply(r: &Responder, content: String, session: &SpreadsheetSession) -> BotResult<()> {
    let mut data = ReplyData::text(content).embed(build_summary_embed(session));
    data.files = session_files(session, true).await;
    r.edit_reply(data).await
}

pub async fn slash_spreadsheets(app: &App, r: &Responder, cmd: &CommandInteraction) -> BotResult<()> {
    let options = cmd.data.options();
    let Some((sub, o)) = opts::subcommand(&options) else { return Ok(()) };
    r.defer(true).await?;
    let team_value = opts::string(o, "team").unwrap_or("");
    let Some((config, team)) = resolve_team(app, r, team_value).await? else { return Ok(()) };
    let session_id = opts::string(o, "session_id");
    let actor_id = r.user().id.to_string();
    let actor_tag = display_tag(r.user());

    match sub {
        "status" => {
            let sessions = list_sessions(&app.store, &SessionFilter { team_id: Some(team.id.clone()), status: None }).await;
            let pending = sessions.iter().filter(|s| s.status == "pending").count();
            let processed = sessions.iter().filter(|s| s.status == "processed").count();
            let sp = &config.spreadsheets;
            r.edit_reply(ReplyData::text(
                [
                    format!("Team: **{}**", team.name),
                    format!("Enabled: **{}**", if team.enabled { "yes" } else { "no" }),
                    format!("Monitored channel: {}", if team.monitored_channel_id.is_empty() { "not set".to_string() } else { format!("<#{}>", team.monitored_channel_id) }),
                    format!("Output channel: {}", if team.output_channel_id.is_empty() { "submission channel".to_string() } else { format!("<#{}>", team.output_channel_id) }),
                    format!("Access role: {}", if team.access_role_id.is_empty() { "admins only".to_string() } else { format!("<@&{}>", team.access_role_id) }),
                    format!("Grouping window: **{} minute(s)**", sp.session_window_minutes),
                    "OCR engine: **local PaddleOCR (PP-OCRv5, no API keys)**".to_string(),
                    format!("Raw data retention: **{} day(s)**", sp.raw_data_retention_days),
                    format!("Local image retention: **{} day(s)**", sp.image_retention_days),
                    format!("Output format: **{}**", sp.output_format),
                    format!("Sessions: **{}** total, **{pending}** pending, **{processed}** processed", sessions.len()),
                ]
                .join("\n"),
            ))
            .await
        }
        "sessions" => {
            let sessions = list_sessions(&app.store, &SessionFilter { team_id: Some(team.id.clone()), status: None }).await;
            if sessions.is_empty() {
                return r.edit_reply(ReplyData::text(format!("No spreadsheet sessions have been captured for **{}** yet.", team.name))).await;
            }
            let lines: Vec<String> = sessions
                .iter()
                .take(10)
                .map(|s| format!("`{}` - **{}** - {} image(s) - {}", s.id, s.status, s.images.len(), fmt_local(if s.updated_at.is_empty() { &s.created_at } else { &s.updated_at })))
                .collect();
            r.edit_reply(ReplyData::text(lines.join("\n"))).await
        }
        "generate" => {
            let Some(session) = resolve_session(app, &team.id, session_id, &[]).await else {
                return r.edit_reply(ReplyData::text(format!("No spreadsheet session found for **{}**.", team.name))).await;
            };
            let processed = process_spreadsheet_session(app, &session.id, ProcessOptions { rerun_ocr: opts::boolean(o, "rerun_ocr") == Some(true) }).await?;
            let mut data = ReplyData::default().embed(build_summary_embed(&processed));
            data.files = session_files(&processed, true).await;
            r.edit_reply(data).await?;
            if processed.status == "processed" {
                send_session_output(app, &processed).await;
            }
            cleanup_generated_image_outputs(app, &processed.outputs).await;
            Ok(())
        }
        "summary" => {
            let Some(session) = resolve_session(app, &team.id, session_id, &["processed", "failed", "pending"]).await else {
                return r.edit_reply(ReplyData::text(format!("No spreadsheet session found for **{}**.", team.name))).await;
            };
            r.edit_reply(ReplyData::text(build_summary_text(&session))).await
        }
        "weekly" => reply_with_period_report(app, r, o, &team, "weekly").await,
        "monthly" => reply_with_period_report(app, r, o, &team, "monthly").await,
        "file" => {
            let Some(session) = resolve_session(app, &team.id, session_id, &["processed"]).await else {
                return r.edit_reply(ReplyData::text(format!("No processed spreadsheet session found for **{}**.", team.name))).await;
            };
            let path = session.outputs.spreadsheet_path.clone();
            let ready = rebuild_if_missing(app, session, &[&path]).await?;
            let files = session_files(&ready, false).await;
            if files.is_empty() {
                r.edit_reply(ReplyData::text(format!("No spreadsheet file exists for `{}`. Rebuild it first.", ready.id))).await?;
            } else {
                let mut d = ReplyData::text(format!("Spreadsheet for `{}`.", ready.id));
                d.files = files;
                r.edit_reply(d).await?;
            }
            cleanup_generated_image_outputs(app, &ready.outputs).await;
            Ok(())
        }
        "chart" => {
            let Some(session) = resolve_session(app, &team.id, session_id, &["processed"]).await else {
                return r.edit_reply(ReplyData::text(format!("No processed spreadsheet session found for **{}**.", team.name))).await;
            };
            let path = session.outputs.chart_path.clone();
            let ready = rebuild_if_missing(app, session, &[&path]).await?;
            match attachment_for(&ready.outputs.chart_path).await {
                Some(chart) => r.edit_reply(ReplyData::text(format!("Chart for `{}`.", ready.id)).file(chart)).await?,
                None => r.edit_reply(ReplyData::text(format!("No chart exists for `{}`. Rebuild it first.", ready.id))).await?,
            }
            cleanup_generated_image_outputs(app, &ready.outputs).await;
            Ok(())
        }
        "correct" | "correct-name" | "correct-team" | "correct-placement" | "correct-points" | "correct-event-name" => {
            let sid = opts::string(o, "session_id").unwrap_or("");
            let Some(session) = resolve_session(app, &team.id, Some(sid), &["processed"]).await else {
                return r.edit_reply(ReplyData::text(format!("No processed session `{sid}` exists for **{}**.", team.name))).await;
            };
            let row = opts::integer(o, "row").unwrap_or(1);
            let value = opts::string(o, "value").unwrap_or("").to_string();
            let mk = |field: &str, value: String, row: i64| Correction { row, field: field.into(), value, actor_id: actor_id.clone(), actor_tag: actor_tag.clone(), ..Default::default() };
            let (content, corrected) = match sub {
                "correct" => ("Correction applied to `{id}` and outputs were rebuilt.", correct_spreadsheet_session(app, &session.id, mk(opts::string(o, "field").unwrap_or(""), value, row)).await?),
                "correct-name" => ("Player name correction applied to `{id}`.", correct_spreadsheet_session(app, &session.id, mk("player_name", value, row)).await?),
                "correct-team" => {
                    let mut s = correct_spreadsheet_session(app, &session.id, mk("team_type", opts::string(o, "team_type").unwrap_or("").to_string(), row)).await?;
                    if let Some(label) = opts::string(o, "value").filter(|v| !v.is_empty()) {
                        s = correct_spreadsheet_session(app, &session.id, mk("team_name", label.to_string(), row)).await?;
                    }
                    ("Team correction applied to `{id}`.", s)
                }
                "correct-placement" => ("Placement correction applied to `{id}`.", correct_spreadsheet_session(app, &session.id, mk("placement", opts::integer(o, "placement").unwrap_or(1).to_string(), row)).await?),
                "correct-points" => ("Points correction applied to `{id}`.", correct_spreadsheet_session(app, &session.id, mk(opts::string(o, "field").unwrap_or("points"), value, row)).await?),
                _ => ("Event name correction applied to `{id}`.", correct_spreadsheet_session(app, &session.id, mk("event_name", value, 1)).await?),
            };
            summary_reply(r, content.replace("{id}", &corrected.id), &corrected).await?;
            cleanup_generated_image_outputs(app, &corrected.outputs).await;
            Ok(())
        }
        "regenerate-weekly" => post_report_and_reply(app, r, o, &team, "weekly", "staff-regenerate-weekly").await,
        "regenerate-monthly" => post_report_and_reply(app, r, o, &team, "monthly", "staff-regenerate-monthly").await,
        "test-ocr" => {
            let Some(session) = resolve_session(app, &team.id, session_id, &["pending", "processed", "failed"]).await else {
                return r.edit_reply(ReplyData::text(format!("No spreadsheet session found for **{}**.", team.name))).await;
            };
            let preview = preview_spreadsheet_session(app, &session.id, true).await?;
            r.edit_reply(ReplyData::text(format!("TEMP OCR test complete.\n{}", preview_text(&preview)))).await
        }
        "test-grouping" => {
            let sessions = list_sessions(&app.store, &SessionFilter { team_id: Some(team.id.clone()), status: Some("pending".into()) }).await;
            let pending: Vec<&SpreadsheetSession> = sessions.iter().take(10).collect();
            r.edit_reply(ReplyData::text(
                [
                    "TEMP grouping inspection.".to_string(),
                    format!("Grouping window: **{} minute(s)**", config.spreadsheets.session_window_minutes),
                    format!("Pending sessions: **{}**", pending.len()),
                    if pending.is_empty() {
                        "No pending sessions.".to_string()
                    } else {
                        pending
                            .iter()
                            .map(|s| format!("`{}` - {} image(s), author {}, last image {}", s.id, s.images.len(), if s.author_tag.is_empty() { &s.author_id } else { &s.author_tag }, fmt_local(if !s.last_image_at.is_empty() { &s.last_image_at } else { &s.created_at })))
                            .collect::<Vec<_>>()
                            .join("\n")
                    },
                ]
                .join("\n"),
            ))
            .await
        }
        "preview" => {
            let Some(session) = resolve_session(app, &team.id, session_id, &["pending", "processed", "failed"]).await else {
                return r.edit_reply(ReplyData::text(format!("No spreadsheet session found for **{}**.", team.name))).await;
            };
            let preview = preview_spreadsheet_session(app, &session.id, false).await?;
            r.edit_reply(ReplyData::text(format!("TEMP parsed preview.\n{}", preview_text(&preview)))).await
        }
        "force-weekly" => post_report_and_reply(app, r, o, &team, "weekly", "temp-force-weekly").await,
        "force-monthly" => post_report_and_reply(app, r, o, &team, "monthly", "temp-force-monthly").await,
        "rebuild" | "rebuild-event" => {
            let sid = opts::string(o, "session_id").unwrap_or("");
            let Some(session) = resolve_session(app, &team.id, Some(sid), &["processed"]).await else {
                return r.edit_reply(ReplyData::text(format!("No processed session `{sid}` exists for **{}**.", team.name))).await;
            };
            let rebuilt = rebuild_spreadsheet_session(app, &session.id).await?;
            summary_reply(r, format!("Rebuilt `{}`.", rebuilt.id), &rebuilt).await?;
            cleanup_generated_image_outputs(app, &rebuilt.outputs).await;
            Ok(())
        }
        _ => Ok(()),
    }
}
