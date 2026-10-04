//! Team-event spreadsheets: screenshot intake and grouping, local OCR processing, outputs, corrections and the
//! weekly/monthly reports. Port of `spreadsheetManager.js` with the Gemini step replaced by the local readers.

use crate::app::App;
use crate::logs::{log_action, LogEntry};
use crate::managers::recruitment::vision::{download, known_team_names, read_event_bytes};
use crate::util::*;
use chrono::{DateTime, Datelike, TimeZone, Utc};
use dca_core::calc::*;
use dca_core::metrics::*;
use dca_core::report::*;
use dca_core::session::*;
use dca_state::config::{DashboardConfig, SpreadsheetTeam};
use dca_state::models::*;
use dca_state::stores::*;
use dca_state::util::{now_iso, parse_ms};
use serde_json::json;
use serenity::all::*;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

fn normalize(value: &str) -> String {
    normalize_key(value)
}

pub fn spreadsheet_data_dir(app: &App) -> PathBuf {
    app.dirs.data.join("spreadsheets")
}

fn output_dir(app: &App, team_id: &str, session_id: &str) -> PathBuf {
    spreadsheet_data_dir(app).join(safe_file_name(team_id, "team")).join(safe_file_name(session_id, "session"))
}

pub fn find_spreadsheet_team<'a>(config: &'a DashboardConfig, value: &str) -> Option<&'a SpreadsheetTeam> {
    let text = normalize(value);
    config.spreadsheets.teams.iter().find(|t| t.id == value || normalize(&t.id) == text || normalize(&t.name) == text || t.own_team_aliases.iter().any(|a| normalize(a) == text))
}

fn enabled_teams(config: &DashboardConfig) -> Vec<&SpreadsheetTeam> {
    if !config.spreadsheets.enabled {
        return Vec::new();
    }
    config.spreadsheets.teams.iter().filter(|t| t.enabled && !t.monitored_channel_id.is_empty()).collect()
}

pub fn session_window_ms(config: &DashboardConfig) -> i64 {
    (config.spreadsheets.session_window_minutes.max(1) as i64) * 60_000
}

// ----------------------------------------------------------------------------------------- intake

fn image_attachments(message: &Message) -> Vec<dca_state::models::Attachment> {
    message
        .attachments
        .iter()
        .filter(|a| is_image_attachment(a))
        .take(20)
        .map(|a| dca_state::models::Attachment {
            id: a.id.to_string(),
            message_id: message.id.to_string(),
            name: if a.filename.is_empty() { "screenshot".into() } else { a.filename.clone() },
            url: a.url.clone(),
            proxy_url: a.proxy_url.clone(),
            content_type: a.content_type.clone().unwrap_or_default(),
            size: a.size as u64,
            created_at: message.timestamp.to_string(),
            ..Default::default()
        })
        .collect()
}

/// A screenshot posted in a monitored channel joins (or starts) that person's pending session.
pub async fn handle_message(app: &App, message: &Message) -> bool {
    let Some(guild) = message.guild_id else { return false };
    if message.author.bot {
        return false;
    }
    let config = app.config().await;
    let channel = message.channel_id.to_string();
    let Some(team) = enabled_teams(&config).into_iter().find(|t| t.monitored_channel_id == channel) else { return false };
    let attachments = image_attachments(message);
    if attachments.is_empty() {
        return false;
    }
    let now = now_iso();
    let window = session_window_ms(&config);
    let pending = list_sessions(&app.store, &SessionFilter { team_id: Some(team.id.clone()), status: Some("pending".into()) }).await;
    let author = message.author.id.to_string();
    let latest = pending
        .into_iter()
        .find(|s| s.channel_id == channel && s.author_id == author && unix_ms() - parse_ms(if s.last_image_at.is_empty() { &s.created_at } else { &s.last_image_at }).unwrap_or(0) <= window);
    let mut session = latest.unwrap_or_else(|| SpreadsheetSession {
        team_id: team.id.clone(),
        team_name: team.name.clone(),
        guild_id: guild.to_string(),
        channel_id: channel.clone(),
        author_id: author.clone(),
        author_tag: display_tag(&message.author),
        status: "pending".into(),
        created_at: now.clone(),
        ..Default::default()
    });
    session.team_name = team.name.clone();
    let mid = message.id.to_string();
    if !session.message_ids.contains(&mid) {
        session.message_ids.push(mid);
    }
    session.images.extend(attachments);
    session.last_image_at = now;
    session.status = "pending".into();
    save_session(&app.store, session).await.is_ok()
}

// --------------------------------------------------------------------------------------- roster

async fn learned_roster(app: &App, team_id: &str, exclude: &str) -> Vec<String> {
    let sessions = list_sessions(&app.store, &SessionFilter { team_id: Some(team_id.into()), status: None }).await;
    let mut names = Vec::new();
    for s in sessions.iter().filter(|s| s.id != exclude && s.status == "processed") {
        names.extend(s.attendance.roster.iter().cloned());
        names.extend(s.players.iter().filter(|p| p.team_type == "own").map(|p| p.player_name.clone()));
    }
    unique_names(names)
}

async fn attach_attendance(app: &App, mut session: SpreadsheetSession) -> SpreadsheetSession {
    let sessions = list_sessions(&app.store, &SessionFilter { team_id: Some(session.team_id.clone()), status: None }).await;
    let mut roster: Vec<(String, String)> = Vec::new();
    let add = |name: &str, roster: &mut Vec<(String, String)>| {
        let key = player_key(name);
        if !key.is_empty() && !roster.iter().any(|(k, _)| *k == key) {
            roster.push((key, name.to_string()));
        }
    };
    for h in sessions.iter().filter(|h| h.status == "processed" && h.id != session.id) {
        for p in h.players.iter().filter(|p| p.team_type == "own") {
            add(&p.player_name, &mut roster);
        }
    }
    for p in session.players.iter().filter(|p| p.team_type == "own") {
        add(&p.player_name, &mut roster);
    }
    let current: std::collections::HashSet<String> = session.players.iter().filter(|p| p.team_type == "own").map(|p| player_key(&p.player_name)).filter(|k| !k.is_empty()).collect();
    let mut missing: Vec<String> = roster.iter().filter(|(k, _)| !current.contains(k)).map(|(_, n)| n.clone()).collect();
    missing.sort_by_key(|a| a.to_lowercase());
    let mut all: Vec<String> = roster.into_iter().map(|(_, n)| n).collect();
    all.sort_by_key(|a| a.to_lowercase());
    session.attendance = Attendance {
        roster: all,
        attended_players: session.players.iter().filter(|p| p.team_type == "own").map(|p| p.player_name.clone()).collect(),
        missing_players: missing,
        zero_score_policy: "Players missing this event are recorded as 0 in event summaries and period rollups.".into(),
    };
    session
}

fn team_context(team: &SpreadsheetTeam, learned: &[String]) -> TeamContext {
    TeamContext { name: team.name.clone(), own_team_aliases: team.own_team_aliases.clone(), own_player_aliases: unique_names(team.own_player_aliases.iter().cloned().chain(learned.iter().cloned())) }
}

// ----------------------------------------------------------------------------------- reading images

/// Read every screenshot of a session with the local OCR.
async fn read_session_images(app: &App, session: &SpreadsheetSession, config: &DashboardConfig) -> Result<Vec<PageReading>, String> {
    let reader = app.reader().ok_or_else(|| {
        let why = app.reader_error.read().unwrap().clone();
        format!("The local OCR models are not loaded{}.", if why.is_empty() { String::new() } else { format!(" ({why})") })
    })?;
    let known = known_team_names(config);
    let mut pages = Vec::new();
    for (index, image) in session.images.iter().enumerate() {
        let name = if image.name.is_empty() { format!("image-{}", index + 1) } else { image.name.clone() };
        let url = if image.proxy_url.is_empty() { image.url.clone() } else { image.proxy_url.clone() };
        let bytes = match download(app, &url).await {
            Ok(b) => b,
            Err(e) => {
                // Fall back to the direct URL once (proxy URLs can expire independently).
                match download(app, &image.url).await {
                    Ok(b) => b,
                    Err(_) => {
                        pages.push(PageReading { image: name, error: e, ..Default::default() });
                        continue;
                    }
                }
            }
        };
        match read_event_bytes(app, reader.clone(), bytes, known.clone()).await {
            Ok((page, method)) => pages.push(to_reading(&page, &name, &method)),
            Err(e) => pages.push(PageReading { image: name, error: e, ..Default::default() }),
        }
    }
    Ok(pages)
}

fn parsed_from_readings(readings: &[PageReading], team: &TeamContext, known: &[String]) -> Parsed {
    let pages: Vec<_> = readings.iter().filter(|r| !r.rows.is_empty()).map(from_reading).collect();
    parse_pages(&pages, team, known)
}

// ------------------------------------------------------------------------------------------ outputs

struct Files {
    chart: Vec<u8>,
    image: Vec<u8>,
    spreadsheet: Result<Vec<u8>, String>,
    fods: String,
}

/// Write the workbook + images into the session folder and return their paths.
pub async fn rebuild_artifacts(app: &App, session: &SpreadsheetSession, config: &DashboardConfig) -> Result<Outputs, String> {
    let dir = output_dir(app, &session.team_id, &session.id);
    tokio::fs::create_dir_all(&dir).await.map_err(|e| e.to_string())?;
    let base = format!("{}-{}", safe_file_name(&session.team_id, "team"), safe_file_name(&session.id, "session"));
    let s2 = session.clone();
    let renderer_app = app.renderer.clone();
    let files = tokio::task::spawn_blocking(move || -> Result<Files, String> {
        Ok(Files { chart: renderer_app.svg_to_png(&build_chart_svg(&s2))?, image: renderer_app.svg_to_png(&build_spreadsheet_image_svg(&s2))?, spreadsheet: build_xlsx(&s2), fods: build_fods(&s2) })
    })
    .await
    .map_err(|e| e.to_string())??;

    let chart_path = dir.join(format!("{base}-summary-chart.png"));
    let image_path = dir.join(format!("{base}-spreadsheet.png"));
    let fods_path = dir.join(format!("{base}.fods"));
    tokio::fs::write(&chart_path, &files.chart).await.map_err(|e| e.to_string())?;
    tokio::fs::write(&image_path, &files.image).await.map_err(|e| e.to_string())?;
    tokio::fs::write(&fods_path, files.fods.as_bytes()).await.map_err(|e| e.to_string())?;

    let mut spreadsheet_path = fods_path.clone();
    let mut conversion_error = String::new();
    if config.spreadsheets.output_format != "fods" {
        match files.spreadsheet {
            Ok(bytes) => {
                let xlsx = dir.join(format!("{base}.xlsx"));
                tokio::fs::write(&xlsx, bytes).await.map_err(|e| e.to_string())?;
                spreadsheet_path = xlsx;
            }
            Err(e) => conversion_error = format!("Direct XLSX generation failed, so the flat ODS file is used instead: {e}"),
        }
    }
    let p = |path: &Path| path.to_string_lossy().to_string();
    Ok(Outputs {
        fods_path: p(&fods_path),
        spreadsheet_path: p(&spreadsheet_path),
        spreadsheet_image_path: p(&image_path),
        chart_path: p(&chart_path),
        conversion_error,
        generated_at: now_iso(),
        ..Default::default()
    })
}

pub async fn cleanup_generated_image_outputs(app: &App, outputs: &Outputs) -> usize {
    let root = spreadsheet_data_dir(app);
    let mut deleted = 0;
    for path in [&outputs.spreadsheet_image_path, &outputs.chart_path, &outputs.table_image_path] {
        if path.is_empty() {
            continue;
        }
        let pb = PathBuf::from(path);
        if !pb.starts_with(&root) {
            continue;
        }
        if tokio::fs::remove_file(&pb).await.is_ok() {
            deleted += 1;
            remove_empty_parents(&pb, &root).await;
        }
    }
    deleted
}

async fn remove_empty_parents(path: &Path, stop: &Path) {
    let mut current = path.parent().map(Path::to_path_buf);
    while let Some(dir) = current {
        if !dir.starts_with(stop) || dir == stop {
            break;
        }
        match tokio::fs::read_dir(&dir).await {
            Ok(mut rd) => {
                if rd.next_entry().await.ok().flatten().is_some() {
                    break;
                }
            }
            Err(_) => break,
        }
        if tokio::fs::remove_dir(&dir).await.is_err() {
            break;
        }
        current = dir.parent().map(Path::to_path_buf);
    }
}

/// Delete generated images / old source folders past their retention.
pub async fn cleanup_local_spreadsheet_images(app: &App, retention_days: u32) -> (usize, usize) {
    let root = spreadsheet_data_dir(app);
    let cutoff = std::time::SystemTime::now() - Duration::from_secs(retention_days.max(1) as u64 * 86_400);
    let source_cutoff = std::time::SystemTime::now() - Duration::from_secs(86_400);
    let (mut files, mut dirs) = (0usize, 0usize);
    let mut stack = vec![root.clone()];
    while let Some(dir) = stack.pop() {
        let Ok(mut rd) = tokio::fs::read_dir(&dir).await else { continue };
        while let Ok(Some(entry)) = rd.next_entry().await {
            let path = entry.path();
            let Ok(meta) = entry.metadata().await else { continue };
            let modified = meta.modified().unwrap_or(std::time::SystemTime::now());
            if meta.is_dir() {
                if entry.file_name() == "source" && modified < source_cutoff {
                    if tokio::fs::remove_dir_all(&path).await.is_ok() {
                        dirs += 1;
                    }
                    continue;
                }
                stack.push(path);
            } else {
                let name = entry.file_name().to_string_lossy().to_lowercase();
                let is_image = [".png", ".jpg", ".jpeg", ".webp", ".gif", ".svg"].iter().any(|e| name.ends_with(e));
                if is_image && modified < cutoff && tokio::fs::remove_file(&path).await.is_ok() {
                    files += 1;
                    remove_empty_parents(&path, &root).await;
                }
            }
        }
    }
    (files, dirs)
}

// --------------------------------------------------------------------------------------- processing

pub struct ProcessOptions {
    pub rerun_ocr: bool,
}

fn no_table_error(pages: &[PageReading]) -> String {
    let detail = pages.iter().filter(|p| !p.error.is_empty()).map(|p| format!("{}: {}", p.image, p.error)).collect::<Vec<_>>().join("; ");
    format!(
        "Could not detect a valid standings table in the provided screenshot(s). Make sure the full standings table (ranks, names, scores) is visible and the screenshots are not heavily cropped or low resolution.{}",
        if detail.is_empty() { String::new() } else { format!(" ({detail})") }
    )
}

pub async fn process_spreadsheet_session(app: &App, session_id: &str, options: ProcessOptions) -> Result<SpreadsheetSession, String> {
    let config = app.config().await;
    let session = get_session(&app.store, session_id).await.ok_or("Spreadsheet session not found.")?;
    let team = find_spreadsheet_team(&config, &session.team_id).ok_or("Spreadsheet team config was not found.")?.clone();
    if session.status == "processing" {
        return Ok(session);
    }
    if !app.processing.lock().unwrap().insert(session.id.clone()) {
        return Ok(session);
    }
    let result = process_inner(app, &config, session, &team, options).await;
    app.processing.lock().unwrap().remove(session_id);
    result
}

async fn process_inner(app: &App, config: &DashboardConfig, session: SpreadsheetSession, team: &SpreadsheetTeam, options: ProcessOptions) -> Result<SpreadsheetSession, String> {
    let id = session.id.clone();
    update_session(&app.store, &id, |s| {
        s.status = "processing".into();
        s.error = String::new();
    })
    .await?;

    let attempt = async {
        let known = known_team_names(config);
        let learned = learned_roster(app, &session.team_id, &id).await;
        let ctx = team_context(team, &learned);
        let mut readings = session.readings.clone();
        if readings.is_empty() || options.rerun_ocr {
            readings = read_session_images(app, &session, config).await?;
        }
        if !readings.iter().any(|r| !r.rows.is_empty()) {
            return Err(no_table_error(&readings));
        }
        let parsed = apply_corrections(parsed_from_readings(&readings, &ctx, &known), &session.corrections);
        let mut next = session.clone();
        next.status = "processed".into();
        next.processed_at = now_iso();
        next.readings = readings;
        next.metadata = parsed.metadata.clone();
        next.team_event_name = if parsed.metadata.event_name.is_empty() { parsed.metadata.title.clone() } else { parsed.metadata.event_name.clone() };
        next.players = parsed.players;
        next.stats = parsed.stats;
        next.raw_ocr_text = parsed.raw_text;
        next.error = String::new();
        next.outputs = Outputs::default();
        let mut next = attach_attendance(app, next).await;
        next.team_name = team.name.clone();
        next.outputs = rebuild_artifacts(app, &next, config).await?;
        save_session(&app.store, next).await
    }
    .await;

    match attempt {
        Ok(saved) => {
            log_action(
                app,
                LogEntry::new("system", "Race Spreadsheet Generated", format!("Generated spreadsheet session **{}** for **{}** with {} parsed players.", saved.id, team.name, saved.players.len()))
                    .guild(&saved.guild_id)
                    .actor(&saved.author_id, &saved.author_tag)
                    .meta(json!({ "teamId": saved.team_id, "sessionId": saved.id, "players": saved.players.len() })),
            )
            .await;
            Ok(saved)
        }
        Err(error) => {
            let e = error.clone();
            let _ = update_session(&app.store, &id, |s| {
                s.status = "failed".into();
                s.error = e.clone();
            })
            .await;
            Err(error)
        }
    }
}

pub async fn rebuild_spreadsheet_session(app: &App, session_id: &str) -> Result<SpreadsheetSession, String> {
    let config = app.config().await;
    let session = get_session(&app.store, session_id).await.ok_or("Spreadsheet session not found.")?;
    let team = find_spreadsheet_team(&config, &session.team_id).ok_or("Spreadsheet team config was not found.")?.clone();
    let known = known_team_names(&config);
    let learned = learned_roster(app, &session.team_id, &session.id).await;
    let ctx = team_context(&team, &learned);

    // Sessions from the Gemini era have no stored page readings: rebuild from their saved players instead.
    let base = if session.readings.iter().any(|r| !r.rows.is_empty()) {
        parsed_from_readings(&session.readings, &ctx, &known)
    } else {
        Parsed { metadata: session.metadata.clone(), players: session.players.clone(), stats: session.stats.clone(), raw_text: session.raw_ocr_text.clone() }
    };
    let parsed = apply_corrections(base, &session.corrections);
    let mut next = session.clone();
    next.status = "processed".into();
    next.metadata = parsed.metadata.clone();
    next.players = parsed.players;
    next.stats = parsed.stats;
    if !parsed.raw_text.is_empty() {
        next.raw_ocr_text = parsed.raw_text;
    }
    next.team_event_name = if !parsed.metadata.event_name.is_empty() {
        parsed.metadata.event_name.clone()
    } else if !parsed.metadata.title.is_empty() {
        parsed.metadata.title.clone()
    } else {
        session.team_event_name.clone()
    };
    next.outputs = Outputs::default();
    let mut next = attach_attendance(app, next).await;
    next.outputs = rebuild_artifacts(app, &next, &config).await?;
    let saved = save_session(&app.store, next).await?;
    log_action(
        app,
        LogEntry::new("system", "Race Spreadsheet Rebuilt", format!("Rebuilt spreadsheet session **{}** for **{}**.", saved.id, team.name))
            .guild(&saved.guild_id)
            .actor(&saved.author_id, &saved.author_tag)
            .meta(json!({ "teamId": saved.team_id, "sessionId": saved.id })),
    )
    .await;
    Ok(saved)
}

pub async fn correct_spreadsheet_session(app: &App, session_id: &str, correction: Correction) -> Result<SpreadsheetSession, String> {
    let mut correction = correction;
    correction.created_at = now_iso();
    update_session(&app.store, session_id, |s| s.corrections.push(correction.clone())).await?.ok_or("Spreadsheet session not found.")?;
    rebuild_spreadsheet_session(app, session_id).await
}

/// Parse without saving or posting anything (the `preview` subcommand).
pub async fn preview_spreadsheet_session(app: &App, session_id: &str, rerun_ocr: bool) -> Result<SpreadsheetSession, String> {
    let config = app.config().await;
    let session = get_session(&app.store, session_id).await.ok_or("Spreadsheet session not found.")?;
    let team = find_spreadsheet_team(&config, &session.team_id).ok_or("Spreadsheet team config was not found.")?.clone();
    let known = known_team_names(&config);
    let learned = learned_roster(app, &session.team_id, &session.id).await;
    let ctx = team_context(&team, &learned);
    let mut readings = session.readings.clone();
    if readings.is_empty() || rerun_ocr {
        readings = read_session_images(app, &session, &config).await?;
    }
    let parsed = apply_corrections(parsed_from_readings(&readings, &ctx, &known), &session.corrections);
    let mut preview = session.clone();
    preview.status = "preview".into();
    preview.metadata = parsed.metadata.clone();
    preview.team_event_name = parsed.metadata.event_name.clone();
    preview.players = parsed.players;
    preview.stats = parsed.stats;
    preview.raw_ocr_text = parsed.raw_text;
    Ok(attach_attendance(app, preview).await)
}

// ------------------------------------------------------------------------------------- summaries

pub fn session_kab_players(session: &SpreadsheetSession) -> Vec<Player> {
    let own = own_players(session);
    let Some(threshold) = top_opponent_rank(session) else { return Vec::new() };
    own.into_iter().filter(|p| p.rank > 0 && p.rank < threshold).cloned().collect()
}

pub fn build_summary_text(session: &SpreadsheetSession) -> String {
    let stats = &session.stats;
    let meta = &session.metadata;
    let kab = session_kab_players(session);
    let mut best_own: Vec<&Player> = session.players.iter().filter(|p| p.team_type == "own").collect();
    best_own.sort_by_key(|p| if p.rank == 0 { 999 } else { p.rank });
    best_own.truncate(8);
    let mut lines = vec![
        format!("**{}**", if meta.title.is_empty() { "Race Session" } else { &meta.title }),
        format!("Session: `{}`", session.id),
        format!("Team: **{}**", [&meta.own_team_name, &session.team_name, &session.team_id].iter().find(|v| !v.is_empty()).map(|v| v.as_str()).unwrap_or("")),
        format!("Players parsed: **{}** ({} own, {} opponents)", stats.total_players, stats.own_players, stats.opponents),
        format!("Points: own **{}** vs opponents **{}**", stats.own_points, stats.opponent_points),
        format!("Scores: own **{}** vs opponents **{}**", stats.own_score, stats.opponent_score),
        format!("Top 10: own **{}** vs opponents **{}**", stats.own_top10, stats.opponent_top10),
        format!("#KAB this event: **{}** ({})", kab.len(), if kab.is_empty() { "none".to_string() } else { kab.iter().map(|p| p.player_name.clone()).collect::<Vec<_>>().join(", ") }),
    ];
    if !best_own.is_empty() {
        lines.push(String::new());
        lines.push("Best own players:".into());
        lines.push(
            best_own
                .iter()
                .map(|p| format!("#{} {} - {} point(s), {} score", p.rank, p.player_name, p.points.or(p.score).unwrap_or(0), p.score.map(|s| s.to_string()).unwrap_or_else(|| "no score".into())))
                .collect::<Vec<_>>()
                .join("\n"),
        );
    }
    if !stats.podium.is_empty() {
        lines.push(format!("Podium: {}", stats.podium.iter().map(|p| format!("#{} {}", p.rank, p.player_name)).collect::<Vec<_>>().join(", ")));
    }
    lines.push(String::new());
    lines.push(
        "Weekly and monthly reports are rebuilt from processed sessions. Missing players in a period receive 0 for that event, and #KAB counts events where a player ranked above every opponent."
            .into(),
    );
    let flagged: Vec<String> = session.players.iter().filter(|p| p.flagged).map(|p| format!("#{} {}", p.rank, p.player_name)).collect();
    if !flagged.is_empty() {
        lines.push(format!("Needs a look (low OCR confidence): {}. Use `/spreadsheets correct-name` if a name is wrong.", flagged.join(", ")));
    }
    if !session.outputs.conversion_error.is_empty() {
        lines.push(session.outputs.conversion_error.clone());
    }
    lines.join("\n")
}

pub fn build_summary_embed(session: &SpreadsheetSession) -> CreateEmbed {
    let mut e = CreateEmbed::new().title("Race Spreadsheet Summary").description(truncate(&build_summary_text(session), 4000)).colour(Colour::new(0x0f766e));
    let ts = [&session.processed_at, &session.updated_at].iter().find(|v| !v.is_empty()).and_then(|v| Timestamp::parse(v).ok());
    e = e.timestamp(ts.unwrap_or_else(Timestamp::now));
    e
}

pub fn build_period_report_embed(model: &ReportModel) -> CreateEmbed {
    CreateEmbed::new()
        .title(format!("{} {} Report", model.team_name, if model.period == "weekly" { "Weekly" } else { "Monthly" }))
        .colour(Colour::new(0x0f7dba))
        .description(report_embed_description(model))
        .timestamp(Timestamp::now())
}

pub async fn attachment_for(path: &str) -> Option<CreateAttachment> {
    if path.is_empty() {
        return None;
    }
    CreateAttachment::path(path).await.ok()
}

pub async fn session_files(session: &SpreadsheetSession, include_images: bool) -> Vec<CreateAttachment> {
    let mut out = Vec::new();
    out.extend(attachment_for(&session.outputs.spreadsheet_path).await);
    if include_images {
        out.extend(attachment_for(&session.outputs.spreadsheet_image_path).await);
        out.extend(attachment_for(&session.outputs.chart_path).await);
    }
    out
}

// ----------------------------------------------------------------------------------------- output

fn event_name(session: &SpreadsheetSession) -> String {
    event_name_for_session(session, "Team Event")
}

pub async fn send_session_output(app: &App, session: &SpreadsheetSession) -> Option<Message> {
    let config = app.config().await;
    let team = find_spreadsheet_team(&config, &session.team_id).cloned();
    let target = team.as_ref().map(|t| t.output_channel_id.clone()).filter(|c| !c.is_empty()).unwrap_or_else(|| session.channel_id.clone());
    let channel = channel_id(&target)?;
    let files = session_files(session, true).await;
    let content = if team.as_ref().is_some_and(|t| !t.output_channel_id.is_empty()) {
        format!("Final team-event output for **{}** (`{}`).", event_name(session), session.id)
    } else {
        format!("Spreadsheet session `{}` was processed. Configure an output channel to receive automatic final files and reports.", session.id)
    };
    let messages = send_file_chunks(app, channel, &content, files).await;
    if let Some(first) = messages.first() {
        // Remember the CDN links of the files that were posted.
        let urls: Vec<(String, String)> = messages.iter().flat_map(|m| m.attachments.iter().map(|a| (a.filename.clone(), a.url.clone()))).collect();
        let outputs = session.outputs.clone();
        let find = |path: &str| -> Option<String> {
            let name = Path::new(path).file_name()?.to_string_lossy().to_string();
            urls.iter().find(|(n, _)| *n == name).map(|(_, u)| u.clone())
        };
        let (a, b, c) = (find(&outputs.fods_path), find(&outputs.spreadsheet_path), find(&outputs.spreadsheet_image_path));
        let d = find(&outputs.chart_path);
        let _ = update_session(&app.store, &session.id, |s| {
            if let Some(v) = &a {
                s.outputs.fods_url = v.clone();
            }
            if let Some(v) = &b {
                s.outputs.spreadsheet_url = v.clone();
            }
            if let Some(v) = &c {
                s.outputs.spreadsheet_image_url = v.clone();
            }
            if let Some(v) = &d {
                s.outputs.chart_url = v.clone();
            }
        })
        .await;
        let _ = first;
    }
    if let Some(team) = &team {
        post_automatic_reports(app, team, session).await;
    }
    cleanup_generated_image_outputs(app, &session.outputs).await;
    messages.into_iter().next()
}

async fn send_file_chunks(app: &App, channel: ChannelId, content: &str, files: Vec<CreateAttachment>) -> Vec<Message> {
    let mut messages = Vec::new();
    if files.is_empty() {
        if let Ok(m) = channel.send_message(&app.http, CreateMessage::new().content(content).allowed_mentions(CreateAllowedMentions::new())).await {
            messages.push(m);
        }
        return messages;
    }
    for (index, chunk) in files.chunks(10).enumerate() {
        let mut m = CreateMessage::new()
            .content(if index == 0 { content.to_string() } else { "Additional generated files for the same event output.".to_string() })
            .allowed_mentions(CreateAllowedMentions::new());
        for f in chunk {
            m = m.add_file(f.clone());
        }
        match channel.send_message(&app.http, m).await {
            Ok(msg) => messages.push(msg),
            Err(error) => tracing::warn!("Spreadsheet output post failed: {error}"),
        }
    }
    messages
}

// ----------------------------------------------------------------------------------------- reports

pub struct GeneratedReport {
    pub model: ReportModel,
    pub file_path: String,
    pub chart_path: String,
    pub table_image_path: String,
}

fn reports_dir(app: &App, team_id: &str) -> PathBuf {
    spreadsheet_data_dir(app).join(safe_file_name(team_id, "team")).join("reports")
}

pub async fn generate_period_report(
    app: &App,
    team: &SpreadsheetTeam,
    period: &str,
    anchor: DateTime<Utc>,
    event_override: Option<&str>,
    anchor_session: Option<&SpreadsheetSession>,
) -> Result<Option<GeneratedReport>, String> {
    let bounds = period_bounds(period, anchor);
    let all = list_sessions(&app.store, &SessionFilter { team_id: Some(team.id.clone()), status: None }).await;
    let sessions = filter_sessions_for_report(&all, period, &bounds, anchor, event_override, anchor_session);
    if sessions.is_empty() {
        return Ok(None);
    }
    let info = TeamInfo { id: &team.id, name: &team.name, own_player_aliases: &team.own_player_aliases };
    let model = build_report_model(&info, &sessions, period, &bounds, &all);
    let dir = reports_dir(app, &team.id);
    tokio::fs::create_dir_all(&dir).await.map_err(|e| e.to_string())?;
    let base = format!("{}-{}-{}", safe_file_name(&model.team_id, "report"), model.period, model.period_key);
    let (m2, renderer) = (model.clone(), app.renderer.clone());
    let (xlsx, chart, table) = tokio::task::spawn_blocking(move || -> Result<_, String> {
        Ok((build_report_xlsx(&m2)?, renderer.svg_to_png(&build_report_chart_svg(&m2))?, renderer.svg_to_png(&build_report_table_svg(&m2))?))
    })
    .await
    .map_err(|e| e.to_string())??;
    let (xp, cp, tp) = (dir.join(format!("{base}.xlsx")), dir.join(format!("{base}-chart.png")), dir.join(format!("{base}-drivers.png")));
    tokio::fs::write(&xp, xlsx).await.map_err(|e| e.to_string())?;
    tokio::fs::write(&cp, chart).await.map_err(|e| e.to_string())?;
    tokio::fs::write(&tp, table).await.map_err(|e| e.to_string())?;
    let s = |p: &Path| p.to_string_lossy().to_string();
    Ok(Some(GeneratedReport { model, file_path: s(&xp), chart_path: s(&cp), table_image_path: s(&tp) }))
}

pub async fn report_attachments(report: &GeneratedReport) -> Vec<CreateAttachment> {
    let mut out = Vec::new();
    out.extend(attachment_for(&report.file_path).await);
    out.extend(attachment_for(&report.table_image_path).await);
    out.extend(attachment_for(&report.chart_path).await);
    out
}

pub async fn cleanup_report_images(app: &App, report: &GeneratedReport) {
    cleanup_generated_image_outputs(app, &Outputs { chart_path: report.chart_path.clone(), table_image_path: report.table_image_path.clone(), ..Default::default() }).await;
}

pub struct ReportResult {
    pub skipped: Option<String>,
    pub posted: bool,
}

pub struct ReportOptions {
    pub anchor: DateTime<Utc>,
    pub force: bool,
    pub reason: String,
}

pub async fn send_period_report(app: &App, team: &SpreadsheetTeam, period: &str, options: ReportOptions) -> ReportResult {
    let report = match generate_period_report(app, team, period, options.anchor, None, None).await {
        Ok(Some(r)) => r,
        Ok(None) => return ReportResult { skipped: Some("no-sessions".into()), posted: false },
        Err(e) => return ReportResult { skipped: Some(e), posted: false },
    };
    if !options.force && get_report_emission(&app.store, &team.id, period, &report.model.period_key).await.is_some() {
        return ReportResult { skipped: Some("already-emitted".into()), posted: false };
    }
    let Some(channel) = channel_id(&team.output_channel_id) else { return ReportResult { skipped: Some("missing-output-channel".into()), posted: false } };
    if channel.to_channel(&app.http).await.ok().and_then(|c| c.guild()).is_none() {
        return ReportResult { skipped: Some("output-channel-unavailable".into()), posted: false };
    }
    let files = report_attachments(&report).await;
    let content = if period == "weekly" {
        format!("Weekly report for **{}** - **{}** ({}).", team.name, report.model.event_name, report.model.period_label)
    } else {
        format!("Monthly report for **{}** ({}).", team.name, report.model.period_label)
    };
    let mut message = CreateMessage::new().content(content).allowed_mentions(CreateAllowedMentions::new());
    for f in files {
        message = message.add_file(f);
    }
    let sent = match channel.send_message(&app.http, message).await {
        Ok(m) => m,
        Err(e) => return ReportResult { skipped: Some(e.to_string()), posted: false },
    };
    let _ = mark_report_emitted(
        &app.store,
        ReportEmission {
            team_id: team.id.clone(),
            period: period.into(),
            period_key: report.model.period_key.clone(),
            file_path: report.file_path.clone(),
            attachment_urls: sent.attachments.iter().map(|a| a.url.clone()).collect(),
            event_count: report.model.events.len() as u32,
            reason: options.reason,
            channel_id: channel.to_string(),
            message_id: sent.id.to_string(),
            ..Default::default()
        },
    )
    .await;
    cleanup_report_images(app, &report).await;
    ReportResult { skipped: None, posted: true }
}

async fn post_automatic_reports(app: &App, team: &SpreadsheetTeam, session: &SpreadsheetSession) {
    if team.output_channel_id.is_empty() {
        return;
    }
    let current_name = event_name(session);
    if normalize_key(&current_name).is_empty() {
        return;
    }
    let sessions = list_sessions(&app.store, &SessionFilter { team_id: Some(team.id.clone()), status: Some("processed".into()) }).await;
    let current_date = session_date(session);
    let mut others: Vec<&SpreadsheetSession> = sessions.iter().filter(|s| s.id != session.id).collect();
    others.sort_by_key(|s| std::cmp::Reverse(session_date(s)));
    let previous = others.iter().find(|s| session_date(s) <= current_date).or_else(|| others.first()).copied();
    let Some(previous) = previous else { return };
    // A new event name means last week's event is complete: post its weekly report.
    if same_event_name(&event_name(previous), &current_name) {
        return;
    }
    let reason = format!("event-name-change:{}->{}", event_name(previous), current_name);
    send_period_report(app, team, "weekly", ReportOptions { anchor: session_date(previous), force: false, reason }).await;
}

// -------------------------------------------------------------------------------------- maintenance

fn previous_month_anchor(now: DateTime<Utc>) -> DateTime<Utc> {
    let first = Utc.with_ymd_and_hms(now.year(), now.month(), 1, 12, 0, 0).unwrap();
    first - chrono::Duration::days(1)
}

pub async fn run_spreadsheet_maintenance(app: &App, force_monthly: bool) -> usize {
    let config = app.config().await;
    cleanup_raw_data(&app.store, config.spreadsheets.raw_data_retention_days).await;
    cleanup_local_spreadsheet_images(app, config.spreadsheets.image_retention_days).await;
    let now = Utc::now();
    if !force_monthly && now.day() > 3 {
        return 0;
    }
    let mut posted = 0;
    for team in enabled_teams(&config).into_iter().filter(|t| !t.output_channel_id.is_empty()) {
        let r = send_period_report(
            app,
            team,
            "monthly",
            ReportOptions { anchor: previous_month_anchor(now), force: force_monthly, reason: if force_monthly { "forced-monthly".into() } else { "month-end".into() } },
        )
        .await;
        if r.posted {
            posted += 1;
        }
    }
    posted
}

/// Background jobs: process sessions whose grouping window ended, and run maintenance.
pub fn start_scheduler(app: Arc<App>) {
    let a = app.clone();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(4)).await;
            if !a.is_ready() {
                continue;
            }
            let config = a.config().await;
            let window = session_window_ms(&config);
            for team in enabled_teams(&config).into_iter().filter(|t| t.auto_process) {
                let pending = list_sessions(&a.store, &SessionFilter { team_id: Some(team.id.clone()), status: Some("pending".into()) }).await;
                for s in pending {
                    let last = parse_ms(if s.last_image_at.is_empty() { &s.created_at } else { &s.last_image_at }).unwrap_or(0);
                    if unix_ms() - last < window || a.processing.lock().unwrap().contains(&s.id) {
                        continue;
                    }
                    let app2 = a.clone();
                    tokio::spawn(async move { process_and_post(&app2, &s.id, &s.channel_id).await });
                }
            }
        }
    });
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(60)).await;
        loop {
            let _ = run_spreadsheet_maintenance(&app, false).await;
            tokio::time::sleep(Duration::from_secs(6 * 60 * 60)).await;
        }
    });
}

/// Auto-processing path: process, post the output, or report the failure in the submission channel.
pub async fn process_and_post(app: &App, session_id: &str, channel: &str) {
    match process_spreadsheet_session(app, session_id, ProcessOptions { rerun_ocr: false }).await {
        Ok(processed) => {
            if processed.status == "processed" {
                send_session_output(app, &processed).await;
            }
        }
        Err(error) => {
            if let Some(ch) = channel_id(channel) {
                let _ = ch
                    .send_message(&app.http, CreateMessage::new().content(format!("Spreadsheet extraction failed for session `{session_id}`: {error}")).allowed_mentions(CreateAllowedMentions::new()))
                    .await;
            }
        }
    }
}

pub fn can_access_team(member_roles: &[RoleId], perms: Permissions, team: &SpreadsheetTeam) -> bool {
    if perms.contains(Permissions::ADMINISTRATOR) {
        return true;
    }
    role_id(&team.access_role_id).is_some_and(|r| member_roles.contains(&r))
}
