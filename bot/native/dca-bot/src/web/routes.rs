//! Health + dashboard API routes (port of `dashboardRoutes.js` / `discordApi.js`).

use super::auth::{self, Web};
use crate::managers::{ban_panel, member_count, reaction_roles, recruitment};
use crate::util::*;
use axum::body::{Body, Bytes};
use axum::extract::{Path, Query, State};
use axum::http::{header, HeaderMap, Request, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use dca_state::config::{save_config_value, update_config, DashboardConfig};
use dca_state::models::Transcript;
use dca_state::stores::*;
use dca_state::util::now_iso;
use serde_json::{json, Value};
use serenity::all::*;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tower::ServiceExt;
use tower_http::services::ServeDir;

type Web_ = Arc<Web>;

fn error(status: StatusCode, message: impl Into<String>) -> Response {
    (status, Json(json!({ "error": message.into() }))).into_response()
}

fn config_json(config: &DashboardConfig) -> Value {
    serde_json::to_value(config).unwrap_or(Value::Null)
}

// ---------------------------------------------------------------------------------------------- health

fn ocr_status(web: &Web) -> Value {
    let error = web.app.reader_error.read().unwrap().clone();
    if web.app.reader().is_some() {
        json!("ready")
    } else if !error.is_empty() {
        json!({ "state": "error", "error": error })
    } else {
        json!("loading")
    }
}

pub async fn root(State(web): State<Web_>) -> Response {
    let ready = web.app.is_ready();
    (
        if ready { StatusCode::OK } else { StatusCode::SERVICE_UNAVAILABLE },
        Json(json!({
            "status": if ready { "alive" } else { "disconnected" },
            "uptime": web.app.uptime_secs(),
            "discordStatus": if ready { "connected" } else { "disconnected" },
            "ocr": ocr_status(&web),
            "timestamp": now_iso(),
        })),
    )
        .into_response()
}

pub async fn health(State(web): State<Web_>) -> Response {
    let ready = web.app.ready.load(Ordering::Relaxed);
    (
        if ready { StatusCode::OK } else { StatusCode::SERVICE_UNAVAILABLE },
        Json(json!({
            "status": if ready { "healthy" } else { "unhealthy" },
            "uptime": web.app.uptime_secs(),
            "discordStatus": if ready { "connected" } else { "disconnected" },
            "ocr": ocr_status(&web),
            "timestamp": now_iso(),
        })),
    )
        .into_response()
}

// ---------------------------------------------------------------------------------------------- lookups

fn channel_label(channel: &GuildChannel, categories: &HashMap<ChannelId, String>) -> String {
    match channel.parent_id.and_then(|p| categories.get(&p)) {
        Some(category) => format!("{category} / #{}", channel.name),
        None => format!("#{}", channel.name),
    }
}

async fn single_guild_lookups(web: &Web, guild: &str) -> Value {
    let Some(guild_id) = guild_id(guild) else { return json!({ "ready": false, "guild": null, "channels": [], "roles": [] }) };
    let http = &web.app.http;
    let (info, channels) = tokio::join!(http.get_guild(guild_id), http.get_channels(guild_id));
    let (info, channels) = match (info, channels) {
        (Ok(i), Ok(c)) => (i, c),
        (Err(e), _) | (_, Err(e)) => return json!({ "ready": false, "guild": null, "channels": [], "roles": [], "error": e.to_string() }),
    };
    let categories: HashMap<ChannelId, String> = channels.iter().filter(|c| c.kind == ChannelType::Category).map(|c| (c.id, c.name.clone())).collect();
    let mut text: Vec<&GuildChannel> = channels.iter().filter(|c| matches!(c.kind, ChannelType::Text | ChannelType::News)).collect();
    text.sort_by(|a, b| a.position.cmp(&b.position).then_with(|| a.name.cmp(&b.name)));
    let mut roles: Vec<&Role> = info.roles.values().filter(|r| r.id.get() != guild_id.get() && !r.managed).collect();
    roles.sort_by(|a, b| b.position.cmp(&a.position).then_with(|| a.name.cmp(&b.name)));
    json!({
        "ready": true,
        "guild": {
            "id": info.id.to_string(),
            "name": info.name,
            "icon": info.icon.map(|i| format!("https://cdn.discordapp.com/icons/{}/{}.png?size=80", info.id, i)).unwrap_or_default(),
        },
        "channels": text.iter().map(|c| json!({ "id": c.id.to_string(), "name": channel_label(c, &categories), "type": u8::from(c.kind) })).collect::<Vec<_>>(),
        "roles": roles.iter().map(|r| json!({
            "id": r.id.to_string(),
            "name": r.name,
            "color": if r.colour.0 != 0 { format!("#{:06x}", r.colour.0) } else { String::new() },
            "position": r.position,
        })).collect::<Vec<_>>(),
    })
}

async fn guild_lookups(web: &Web, config: &DashboardConfig) -> Value {
    let env = |k: &str| std::env::var(k).unwrap_or_default();
    let first = |values: [String; 4]| values.into_iter().find(|v| !v.is_empty()).unwrap_or_default();
    let community_id = first([config.bot.community_guild_id.clone(), config.bot.guild_id.clone(), env("COMMUNITY_GUILD_ID"), env("DISCORD_GUILD_ID")]);
    let recruitment_id = first([config.bot.recruitment_guild_id.clone(), env("RECRUITMENT_GUILD_ID"), config.bot.guild_id.clone(), env("DISCORD_GUILD_ID")]);
    let community = single_guild_lookups(web, &community_id).await;
    let recruitment = if !recruitment_id.is_empty() && recruitment_id != community_id { single_guild_lookups(web, &recruitment_id).await } else { community.clone() };
    let mut merged = community.clone();
    if let Some(map) = merged.as_object_mut() {
        map.insert("community".into(), community);
        map.insert("recruitment".into(), recruitment);
    }
    merged
}

// ---------------------------------------------------------------------------------------------- me

pub async fn me(State(web): State<Web_>, headers: HeaderMap) -> Response {
    let session = web.session(&headers);
    let oauth = auth::oauth_config(&web, &headers).await;
    let bot = web.app.ensure_bot_id().await.ok().map(|id| (id, web.app.bot_tag.read().unwrap().clone()));
    let user = session.as_ref().map(|(_, s)| {
        let mut value = serde_json::to_value(&s.user).unwrap_or(Value::Null);
        let avatar = if s.user.avatar.is_empty() { String::new() } else { format!("https://cdn.discordapp.com/avatars/{}/{}.png?size=80", s.user.id, s.user.avatar) };
        value["avatarUrl"] = json!(avatar);
        value
    });
    Json(json!({
        "authenticated": session.is_some(),
        "user": user,
        "setup": { "configured": oauth.configured(), "missing": oauth.missing },
        "bot": { "ready": bot.is_some(), "user": bot.map(|(id, tag)| json!({ "id": id.to_string(), "tag": tag })) },
        "recruitment": { "recruiterRoleId": std::env::var("RECRUITER_ROLE_ID").ok().filter(|v| !v.is_empty()).or_else(|| std::env::var("RECRUITMENT_RECRUITER_ROLE_ID").ok()).unwrap_or_default() },
    }))
    .into_response()
}

// ---------------------------------------------------------------------------------------------- config

macro_rules! authed {
    ($web:expr, $headers:expr) => {
        match auth::require(&$web, &$headers).await {
            Ok(user) => user,
            Err(response) => return response,
        }
    };
}

pub async fn get_config(State(web): State<Web_>, headers: HeaderMap) -> Response {
    authed!(web, headers);
    let store = &web.app.store;
    let config = web.app.config().await;
    let filter = TicketFilter::default();
    let (lookups, recruitment_logs, bot_logs, tickets) = tokio::join!(
        guild_lookups(&web, &config),
        list_recruitment_logs(store, 50),
        list_bot_logs(store, 100, ""),
        list_tickets(store, &filter)
    );
    Json(json!({
        "config": config_json(&config),
        "lookups": lookups,
        "logs": recruitment_logs,
        "recruitmentLogs": recruitment_logs,
        "botLogs": bot_logs,
        "tickets": tickets,
    }))
    .into_response()
}

pub async fn put_config(State(web): State<Web_>, headers: HeaderMap, raw: Bytes) -> Response {
    authed!(web, headers);
    let body: Value = if raw.is_empty() { json!({}) } else { match serde_json::from_slice(&raw) { Ok(v) => v, Err(e) => return error(StatusCode::BAD_REQUEST, format!("Invalid JSON body: {e}")) } };
    let saved = match save_config_value(&web.app.store, &body).await {
        Ok(saved) => saved,
        Err(message) => return error(StatusCode::BAD_REQUEST, message),
    };
    let sync = match reaction_roles::sync_reaction_roles(&web.app, true).await {
        Ok((config, results)) => json!({ "config": config_json(&config), "results": results }),
        Err(message) => json!({ "error": message }),
    };
    let config = sync.get("config").cloned().unwrap_or_else(|| config_json(&saved));
    Json(json!({ "config": config, "sync": sync })).into_response()
}

pub async fn sync_reaction_roles(State(web): State<Web_>, headers: HeaderMap) -> Response {
    authed!(web, headers);
    match reaction_roles::sync_reaction_roles(&web.app, true).await {
        Ok((config, results)) => Json(json!({ "config": config_json(&config), "results": results })).into_response(),
        Err(message) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": message }))).into_response(),
    }
}

pub async fn sync_member_counts(State(web): State<Web_>, headers: HeaderMap) -> Response {
    authed!(web, headers);
    let sync = member_count::sync_member_count_message(&web.app, false, " From Dashboard").await;
    if sync.skipped {
        return Json(json!({ "skipped": true, "reason": sync.reason })).into_response();
    }
    let config = match sync.config {
        Some(c) => c,
        None => web.app.config().await,
    };
    Json(json!({ "config": config_json(&config), "created": sync.created, "channelId": sync.channel_id, "messageId": sync.message_id })).into_response()
}

pub async fn sync_panel(State(web): State<Web_>, headers: HeaderMap) -> Response {
    authed!(web, headers);
    let sync = recruitment::ensure_recruitment_panel(&web.app).await;
    let config = web.app.config().await;
    let sync = if sync.skipped {
        json!({ "skipped": true, "reason": sync.reason })
    } else {
        json!({ "created": sync.created, "channelId": sync.channel_id, "messageId": sync.message_id })
    };
    Json(json!({ "config": config_json(&config), "sync": sync })).into_response()
}

pub async fn sync_ban_list(State(web): State<Web_>, headers: HeaderMap) -> Response {
    authed!(web, headers);
    let sync = ban_panel::sync_recruitment_ban_list(&web.app, true).await;
    let config = match &sync.config {
        Some(c) => c.clone(),
        None => web.app.config().await,
    };
    let sync = if sync.skipped {
        json!({ "skipped": true, "reason": sync.reason })
    } else {
        json!({ "count": sync.count, "messages": sync.messages, "channelId": sync.channel_id })
    };
    Json(json!({ "config": config_json(&config), "sync": sync })).into_response()
}

pub async fn recruitment_logs(State(web): State<Web_>, headers: HeaderMap) -> Response {
    authed!(web, headers);
    Json(json!({ "logs": list_recruitment_logs(&web.app.store, 100).await })).into_response()
}

pub async fn tickets(State(web): State<Web_>, headers: HeaderMap, Query(query): Query<HashMap<String, String>>) -> Response {
    authed!(web, headers);
    let status = query.get("status").filter(|s| !s.is_empty()).cloned();
    Json(json!({ "tickets": list_tickets(&web.app.store, &TicketFilter { status, applicant_id: None }).await })).into_response()
}

pub async fn logs(State(web): State<Web_>, headers: HeaderMap, Query(query): Query<HashMap<String, String>>) -> Response {
    authed!(web, headers);
    let kind = query.get("type").cloned().unwrap_or_default();
    let limit = query.get("limit").and_then(|v| v.parse::<usize>().ok()).filter(|v| *v > 0).unwrap_or(150);
    let (bot_logs, recruitment_logs) = tokio::join!(list_bot_logs(&web.app.store, limit, &kind), list_recruitment_logs(&web.app.store, limit));
    Json(json!({ "botLogs": bot_logs, "recruitmentLogs": recruitment_logs })).into_response()
}

pub async fn transcript(State(web): State<Web_>, headers: HeaderMap, Path(thread_id): Path<String>) -> Response {
    authed!(web, headers);
    let store = &web.app.store;
    let Some(mut ticket) = get_ticket(store, &thread_id).await else { return error(StatusCode::NOT_FOUND, "Ticket not found.") };
    let mut transcript = ticket.transcript.clone();
    if transcript.as_ref().map_or(true, |t| t.text.is_empty()) {
        transcript = None;
        if let Some(thread) = channel_id(&thread_id) {
            let messages = recruitment::fetch_thread_messages(&web.app, thread, 250).await;
            if !messages.is_empty() {
                let lines: Vec<String> = messages.iter().map(recruitment::transcript_line).collect();
                let text: String = lines.join("\n").chars().take(300_000).collect();
                let fresh = Transcript { text: text.clone(), created_at: now_iso(), line_count: lines.len() as u64, source: "discord".into() };
                let saved = fresh.clone();
                let preview: String = text.chars().take(10_000).collect();
                if let Ok(Some(updated)) = update_ticket(store, &thread_id, move |t| {
                    t.transcript = Some(saved);
                    t.transcript_saved = true;
                    t.transcript_preview = preview;
                })
                .await
                {
                    ticket = updated;
                }
                transcript = Some(fresh);
            }
        }
    }
    let preview = if !ticket.transcript_preview.is_empty() { ticket.transcript_preview.clone() } else { transcript.as_ref().map(|t| t.text.chars().take(10_000).collect()).unwrap_or_default() };
    Json(json!({
        "threadId": ticket.thread_id,
        "applicantTag": if ticket.applicant_tag.is_empty() { &ticket.applicant_id } else { &ticket.applicant_tag },
        "status": ticket.status,
        "outcome": if ticket.team.is_empty() { &ticket.outcome } else { &ticket.team },
        "closedAt": ticket.closed_at,
        "transcript": transcript,
        "transcriptPreview": preview,
        "applicantThreadImages": ticket.applicant_thread_images,
    }))
    .into_response()
}

// ---------------------------------------------------------------------------------------------- uploads

pub fn upload_limit() -> usize {
    let raw = std::env::var("DASHBOARD_UPLOAD_LIMIT").unwrap_or_else(|_| "100mb".into()).to_lowercase();
    let digits: String = raw.chars().take_while(|c| c.is_ascii_digit() || *c == '.').collect();
    let number: f64 = digits.parse().unwrap_or(100.0);
    let multiplier = match raw[digits.len()..].trim() {
        "kb" | "k" => 1024.0,
        "gb" | "g" => 1024.0 * 1024.0 * 1024.0,
        "b" | "" if digits.len() == raw.len() => 1.0,
        "b" => 1.0,
        _ => 1024.0 * 1024.0,
    };
    (number * multiplier) as usize
}

fn upload_dir(web: &Web) -> PathBuf {
    std::env::var("DASHBOARD_UPLOAD_DIR").ok().filter(|v| !v.is_empty()).map(PathBuf::from).unwrap_or_else(|| web.app.dirs.data.join("uploads"))
}

fn sanitize_filename(value: &str) -> String {
    let lowered = value.to_lowercase();
    let mut out = String::new();
    let mut pending_dash = false;
    for c in lowered.chars() {
        if c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-' {
            if pending_dash && !out.is_empty() {
                out.push('-');
            }
            pending_dash = false;
            out.push(c);
        } else {
            pending_dash = true;
        }
    }
    let trimmed = out.trim_matches('-').chars().take(80).collect::<String>();
    if trimmed.is_empty() {
        "video".into()
    } else {
        trimmed
    }
}

fn upload_extension(headers: &HeaderMap) -> &'static str {
    let name = headers.get("x-file-name").and_then(|v| v.to_str().ok()).unwrap_or("").to_lowercase();
    for ext in [".mp4", ".mov", ".webm", ".m4v"] {
        if name.ends_with(ext) {
            return ext;
        }
    }
    let kind = headers.get(header::CONTENT_TYPE).and_then(|v| v.to_str().ok()).unwrap_or("").to_lowercase();
    if kind.contains("quicktime") {
        ".mov"
    } else if kind.contains("webm") {
        ".webm"
    } else {
        ".mp4"
    }
}

async fn save_tutorial_upload(web: &Web, headers: &HeaderMap, tutorial_id: &str, config: &DashboardConfig, body: Bytes) -> Result<String, String> {
    if body.is_empty() {
        return Err("No video data was uploaded.".into());
    }
    let raw_name = headers.get("x-file-name").and_then(|v| v.to_str().ok()).unwrap_or(tutorial_id);
    let stem = std::path::Path::new(raw_name).file_stem().and_then(|s| s.to_str()).unwrap_or(tutorial_id);
    let file_name = format!("{tutorial_id}-{}-{}{}", unix_ms(), sanitize_filename(stem), upload_extension(headers));

    let channel = if config.recruitment.tutorial_upload_channel_id.is_empty() { std::env::var("DASHBOARD_UPLOAD_CHANNEL_ID").unwrap_or_default() } else { config.recruitment.tutorial_upload_channel_id.clone() };
    if let Some(channel) = channel_id(&channel) {
        let message = channel
            .send_message(
                &web.app.http,
                CreateMessage::new()
                    .content(format!("Tutorial upload: {tutorial_id}"))
                    .allowed_mentions(CreateAllowedMentions::new())
                    .add_file(CreateAttachment::bytes(body.to_vec(), file_name)),
            )
            .await
            .map_err(|e| e.to_string())?;
        return message.attachments.first().map(|a| a.url.clone()).ok_or_else(|| "Discord did not return an attachment URL.".to_string());
    }
    let dir = upload_dir(web);
    tokio::fs::create_dir_all(&dir).await.map_err(|e| e.to_string())?;
    tokio::fs::write(dir.join(&file_name), &body).await.map_err(|e| e.to_string())?;
    let encoded: String = file_name.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect();
    Ok(format!("{}/uploads/{encoded}", auth::base_url(headers, true)))
}

pub async fn upload_tutorial(State(web): State<Web_>, headers: HeaderMap, Path(id): Path<String>, body: Bytes) -> Response {
    authed!(web, headers);
    let tutorial_id = sanitize_filename(&id);
    let config = web.app.config().await;
    if !config.recruitment.tutorials.iter().any(|t| t.id == tutorial_id) {
        return error(StatusCode::BAD_REQUEST, "Tutorial not found.");
    }
    let url = match save_tutorial_upload(&web, &headers, &tutorial_id, &config, body).await {
        Ok(url) => url,
        Err(message) => {
            tracing::error!("Tutorial upload failed: {message}");
            return error(StatusCode::BAD_REQUEST, message);
        }
    };
    let id = tutorial_id.clone();
    match update_config(&web.app.store, move |c| {
        for tutorial in c.recruitment.tutorials.iter_mut().filter(|t| t.id == id) {
            tutorial.video_url = url.clone();
        }
    })
    .await
    {
        Ok((saved, _)) => {
            let tutorial = saved.recruitment.tutorials.iter().find(|t| t.id == tutorial_id).cloned();
            Json(json!({ "config": config_json(&saved), "tutorial": tutorial })).into_response()
        }
        Err(message) => error(StatusCode::BAD_REQUEST, message),
    }
}

// ---------------------------------------------------------------------------------------------- static files

async fn serve_dir(dir: PathBuf, path: &str, request: Request<Body>, max_age: &str) -> Response {
    let (mut parts, body) = request.into_parts();
    let uri = format!("/{}", path.trim_start_matches('/'));
    parts.uri = match uri.parse() {
        Ok(uri) => uri,
        Err(_) => return StatusCode::NOT_FOUND.into_response(),
    };
    let request = Request::from_parts(parts, body);
    let mut response = match ServeDir::new(dir).append_index_html_on_directories(false).oneshot(request).await {
        Ok(response) => response.into_response(),
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };
    if response.status().is_success() {
        if let Ok(value) = header::HeaderValue::from_str(&format!("public, max-age={max_age}")) {
            response.headers_mut().insert(header::CACHE_CONTROL, value);
        }
    }
    response
}

pub async fn index(State(web): State<Web_>) -> Response {
    match tokio::fs::read(web.app.dirs.dashboard.join("index.html")).await {
        Ok(html) => ([(header::CONTENT_TYPE, "text/html; charset=utf-8")], html).into_response(),
        Err(_) => (StatusCode::NOT_FOUND, "The dashboard has not been built (run `npm run build` in dashboard/).").into_response(),
    }
}

pub async fn dashboard_file(State(web): State<Web_>, Path(path): Path<String>, request: Request<Body>) -> Response {
    serve_dir(web.app.dirs.dashboard.clone(), &path, request, "3600").await
}

pub async fn upload_file(State(web): State<Web_>, Path(path): Path<String>, request: Request<Body>) -> Response {
    serve_dir(upload_dir(&web), &path, request, "604800").await
}

pub async fn static_file(State(web): State<Web_>, request: Request<Body>) -> Response {
    let path = request.uri().path().to_string();
    serve_dir(web.app.dirs.dashboard.clone(), &path, request, "3600").await
}
