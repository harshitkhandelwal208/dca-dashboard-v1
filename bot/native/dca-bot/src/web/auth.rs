//! Discord OAuth sign-in and the signed-cookie dashboard session (port of `dashboardAuth.js`).

use crate::app::App;
use crate::util::*;
use axum::extract::{Query, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Redirect, Response};
use axum::Json;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use dca_state::config::DashboardConfig;
use hmac::{Hmac, Mac};
use rand::RngCore;
use serde::Serialize;
use serde_json::{json, Value};
use serenity::all::*;
use sha2::Sha256;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

const DISCORD_API: &str = "https://discord.com/api/v10";
const SESSION_COOKIE: &str = "dca_dashboard_session";
const STATE_COOKIE: &str = "dca_dashboard_state";
const STATE_TTL_MS: i64 = 10 * 60 * 1000;

type HmacSha256 = Hmac<Sha256>;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionUser {
    pub id: String,
    pub username: String,
    pub global_name: String,
    pub avatar: String,
}

#[derive(Clone, Debug)]
pub struct Session {
    pub user: SessionUser,
    pub member_roles: Vec<String>,
    pub expires_at: i64,
    pub last_role_check_at: i64,
}

pub struct Web {
    pub app: Arc<App>,
    pub secret: Vec<u8>,
    pub sessions: Mutex<HashMap<String, Session>>,
}

fn env_number(name: &str, default: f64) -> f64 {
    std::env::var(name).ok().and_then(|v| v.trim().parse::<f64>().ok()).filter(|v| *v > 0.0).unwrap_or(default).max(1.0)
}

fn session_ttl_ms() -> i64 {
    (env_number("DASHBOARD_SESSION_HOURS", 8.0) * 3_600_000.0) as i64
}

fn role_recheck_ms() -> i64 {
    (env_number("DASHBOARD_ROLE_RECHECK_MINUTES", 5.0) * 60_000.0) as i64
}

fn role_grace_ms() -> i64 {
    (env_number("DASHBOARD_ROLE_RECHECK_GRACE_MINUTES", 30.0) * 60_000.0) as i64
}

pub fn session_secret() -> Vec<u8> {
    if let Ok(secret) = std::env::var("DASHBOARD_SESSION_SECRET") {
        if !secret.is_empty() {
            return secret.into_bytes();
        }
    }
    tracing::warn!("DASHBOARD_SESSION_SECRET is missing. Dashboard sessions will reset when the process restarts.");
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes).into_bytes()
}

fn random_token(bytes: usize) -> String {
    let mut buf = vec![0u8; bytes];
    rand::thread_rng().fill_bytes(&mut buf);
    URL_SAFE_NO_PAD.encode(buf)
}

impl Web {
    fn mac(&self, value: &str) -> HmacSha256 {
        let mut mac = HmacSha256::new_from_slice(&self.secret).expect("hmac accepts any key length");
        mac.update(value.as_bytes());
        mac
    }

    #[cfg(test)]
    pub fn sign_for_test(&self, value: &str) -> String {
        self.sign(value)
    }

    #[cfg(test)]
    pub fn unsign_for_test(&self, value: &str) -> Option<String> {
        self.unsign(value)
    }

    fn sign(&self, value: &str) -> String {
        format!("{value}.{}", URL_SAFE_NO_PAD.encode(self.mac(value).finalize().into_bytes()))
    }

    fn unsign(&self, signed: &str) -> Option<String> {
        let (value, signature) = signed.rsplit_once('.')?;
        let signature = URL_SAFE_NO_PAD.decode(signature).ok()?;
        self.mac(value).verify_slice(&signature).ok()?;
        Some(value.to_string())
    }

    fn signed_cookie(&self, headers: &HeaderMap, name: &str) -> Option<String> {
        self.unsign(&parse_cookies(headers).get(name)?.clone())
    }

    pub fn session(&self, headers: &HeaderMap) -> Option<(String, Session)> {
        let id = self.signed_cookie(headers, SESSION_COOKIE)?;
        let mut sessions = self.sessions.lock().unwrap();
        let session = sessions.get(&id)?.clone();
        if unix_ms() > session.expires_at {
            sessions.remove(&id);
            return None;
        }
        Some((id, session))
    }

    fn create_session(&self, user: &User, roles: Vec<String>) -> String {
        let id = random_token(32);
        let now = unix_ms();
        self.sessions.lock().unwrap().insert(
            id.clone(),
            Session {
                user: SessionUser {
                    id: user.id.to_string(),
                    username: user.name.clone(),
                    global_name: user.global_name.clone().unwrap_or_default(),
                    avatar: user.avatar.map(|a| a.to_string()).unwrap_or_default(),
                },
                member_roles: roles,
                expires_at: now + session_ttl_ms(),
                last_role_check_at: now,
            },
        );
        id
    }

    fn destroy_session(&self, headers: &HeaderMap) {
        if let Some(id) = self.signed_cookie(headers, SESSION_COOKIE) {
            self.sessions.lock().unwrap().remove(&id);
        }
    }
}

fn parse_cookies(headers: &HeaderMap) -> HashMap<String, String> {
    let mut cookies = HashMap::new();
    for value in headers.get_all(header::COOKIE) {
        for part in value.to_str().unwrap_or("").split(';') {
            if let Some((name, value)) = part.split_once('=') {
                let name = name.trim();
                if !name.is_empty() {
                    cookies.insert(name.to_string(), value.trim().to_string());
                }
            }
        }
    }
    cookies
}

fn header_str<'a>(headers: &'a HeaderMap, name: &str) -> &'a str {
    headers.get(name).and_then(|v| v.to_str().ok()).unwrap_or("")
}

pub fn is_secure(headers: &HeaderMap) -> bool {
    header_str(headers, "x-forwarded-proto").split(',').next().unwrap_or("").trim() == "https"
        || std::env::var("DASHBOARD_BASE_URL").map(|v| v.starts_with("https://")).unwrap_or(false)
}

/// The public origin of this server (used for the OAuth redirect and uploaded file links).
pub fn base_url(headers: &HeaderMap, prefer_public: bool) -> String {
    let names: &[&str] = if prefer_public { &["DASHBOARD_PUBLIC_URL", "DASHBOARD_BASE_URL"] } else { &["DASHBOARD_BASE_URL"] };
    for name in names {
        if let Ok(value) = std::env::var(name) {
            if !value.is_empty() {
                return value.trim_end_matches('/').to_string();
            }
        }
    }
    let proto = header_str(headers, "x-forwarded-proto").split(',').next().unwrap_or("").trim();
    format!("{}://{}", if proto.is_empty() { "http" } else { proto }, header_str(headers, "host"))
}

fn set_cookie(headers: &HeaderMap, name: &str, value: &str, max_age_ms: i64) -> HeaderValue {
    let mut parts = vec![format!("{name}={value}"), "Path=/".into(), "HttpOnly".into(), "SameSite=Lax".into(), format!("Max-Age={}", max_age_ms / 1000)];
    if is_secure(headers) {
        parts.push("Secure".into());
    }
    HeaderValue::from_str(&parts.join("; ")).unwrap_or_else(|_| HeaderValue::from_static(""))
}

fn with_cookies(mut response: Response, cookies: Vec<HeaderValue>) -> Response {
    for cookie in cookies {
        response.headers_mut().append(header::SET_COOKIE, cookie);
    }
    response
}

// ------------------------------------------------------------------------------------------------ config

pub fn configured_guild_ids(config: Option<&DashboardConfig>) -> Vec<String> {
    let env = |k: &str| std::env::var(k).unwrap_or_default();
    let mut ids: Vec<String> = Vec::new();
    let candidates = [
        config.map(|c| c.bot.community_guild_id.clone()).unwrap_or_default(),
        config.map(|c| c.bot.recruitment_guild_id.clone()).unwrap_or_default(),
        config.map(|c| c.bot.guild_id.clone()).unwrap_or_default(),
        env("COMMUNITY_GUILD_ID"),
        env("RECRUITMENT_GUILD_ID"),
        env("DISCORD_GUILD_ID"),
    ];
    for id in candidates {
        if !id.is_empty() && !ids.contains(&id) {
            ids.push(id);
        }
    }
    ids
}

fn allowed_role_from_env() -> String {
    ["DASHBOARD_ALLOWED_ROLE_ID", "DISCORD_DASHBOARD_ROLE_ID", "DASHBOARD_ROLE_ID"]
        .iter()
        .filter_map(|k| std::env::var(k).ok())
        .find(|v| !v.is_empty())
        .unwrap_or_default()
}

pub fn allowed_role_id(config: &DashboardConfig) -> String {
    if config.bot.dashboard_allowed_role_id.is_empty() {
        allowed_role_from_env()
    } else {
        config.bot.dashboard_allowed_role_id.clone()
    }
}

pub struct OAuthConfig {
    pub client_id: String,
    pub client_secret: String,
    pub guild_ids: Vec<String>,
    pub allowed_role_id: String,
    pub redirect_uri: String,
    pub missing: Vec<&'static str>,
}

impl OAuthConfig {
    pub fn configured(&self) -> bool {
        self.missing.is_empty()
    }
}

pub async fn oauth_config(web: &Web, headers: &HeaderMap) -> OAuthConfig {
    let config = web.app.config().await;
    let guild_ids = configured_guild_ids(Some(&config));
    let client_id = std::env::var("DISCORD_CLIENT_ID").unwrap_or_default();
    let client_secret = std::env::var("DISCORD_CLIENT_SECRET").unwrap_or_default();
    let allowed = allowed_role_id(&config);
    let redirect_uri = std::env::var("DISCORD_REDIRECT_URI").ok().filter(|v| !v.is_empty()).unwrap_or_else(|| format!("{}/auth/discord/callback", base_url(headers, false)));
    let mut missing = Vec::new();
    if client_id.is_empty() {
        missing.push("DISCORD_CLIENT_ID");
    }
    if client_secret.is_empty() {
        missing.push("DISCORD_CLIENT_SECRET");
    }
    if guild_ids.is_empty() {
        missing.push("DISCORD_GUILD_ID");
    }
    if allowed.is_empty() {
        missing.push("DASHBOARD_ALLOWED_ROLE_ID");
    }
    if std::env::var("DISCORD_TOKEN").unwrap_or_default().is_empty() {
        missing.push("DISCORD_TOKEN");
    }
    OAuthConfig { client_id, client_secret, guild_ids, allowed_role_id: allowed, redirect_uri, missing }
}

// ------------------------------------------------------------------------------------------ role lookups

fn http_status(error: &serenity::Error) -> u16 {
    match error {
        serenity::Error::Http(HttpError::UnsuccessfulRequest(resp)) => resp.status_code.as_u16(),
        _ => 0,
    }
}

struct MemberLookup {
    roles: Vec<String>,
    members: usize,
    transient_errors: Vec<String>,
}

/// The user's roles in every configured guild (bot token lookups).
async fn bot_member_roles(app: &App, user: UserId, guild_ids: &[String]) -> MemberLookup {
    let mut lookup = MemberLookup { roles: Vec::new(), members: 0, transient_errors: Vec::new() };
    for id in guild_ids {
        let Some(guild) = guild_id(id) else { continue };
        match app.http.get_member(guild, user).await {
            Ok(member) => {
                lookup.members += 1;
                for role in member.roles {
                    let role = role.to_string();
                    if !lookup.roles.contains(&role) {
                        lookup.roles.push(role);
                    }
                }
            }
            Err(error) => {
                if ![403, 404].contains(&http_status(&error)) {
                    lookup.transient_errors.push(error.to_string());
                }
            }
        }
    }
    lookup
}

/// `Some(true/false)` when verified, `None` when Discord could not be reached and there is nothing cached to rely on.
async fn session_still_has_role(web: &Web, id: &str, session: &Session) -> Option<bool> {
    let config = web.app.config().await;
    let allowed = allowed_role_id(&config);
    if allowed.is_empty() {
        return Some(false);
    }
    if unix_ms() - session.last_role_check_at < role_recheck_ms() && session.member_roles.contains(&allowed) {
        return Some(true);
    }
    let user = user_id(&session.user.id)?;
    let lookup = bot_member_roles(&web.app, user, &configured_guild_ids(Some(&config))).await;
    if lookup.members == 0 && !lookup.transient_errors.is_empty() {
        let recently_verified = session.last_role_check_at > 0 && unix_ms() - session.last_role_check_at < role_grace_ms();
        if recently_verified && session.member_roles.contains(&allowed) {
            tracing::warn!("Dashboard role recheck failed; using cached verified role: {}", lookup.transient_errors.join("; "));
            return Some(true);
        }
        tracing::error!("Dashboard role recheck failed: {}", lookup.transient_errors.join("; "));
        return None;
    }
    let ok = lookup.roles.contains(&allowed);
    if let Some(stored) = web.sessions.lock().unwrap().get_mut(id) {
        stored.last_role_check_at = unix_ms();
        stored.member_roles = lookup.roles;
    }
    Some(ok)
}

fn json_error(status: StatusCode, message: &str) -> Response {
    (status, Json(json!({ "error": message }))).into_response()
}

/// `requireDashboardAuth`: a signed-in session that still holds the allowed role.
pub async fn require(web: &Arc<Web>, headers: &HeaderMap) -> Result<SessionUser, Response> {
    let Some((id, session)) = web.session(headers) else {
        return Err(json_error(StatusCode::UNAUTHORIZED, "Discord sign in required."));
    };
    match session_still_has_role(web, &id, &session).await {
        Some(true) => Ok(session.user),
        Some(false) => {
            web.destroy_session(headers);
            Err(with_cookies(json_error(StatusCode::FORBIDDEN, "Required Discord role is missing."), vec![set_cookie(headers, SESSION_COOKIE, "", 0)]))
        }
        None => Err(json_error(StatusCode::SERVICE_UNAVAILABLE, "Could not verify Discord role right now.")),
    }
}

// ------------------------------------------------------------------------------------------------ OAuth

pub async fn login(State(web): State<Arc<Web>>, headers: HeaderMap) -> Response {
    let oauth = oauth_config(&web, &headers).await;
    if !oauth.configured() {
        return Redirect::to("/dashboard?auth=setup").into_response();
    }
    let state = random_token(24);
    let cookie = set_cookie(&headers, STATE_COOKIE, &web.sign(&state), STATE_TTL_MS);
    let query = serde_urlencoded_pairs(&[
        ("response_type", "code"),
        ("client_id", &oauth.client_id),
        ("scope", "identify guilds.members.read"),
        ("redirect_uri", &oauth.redirect_uri),
        ("state", &state),
        ("prompt", "consent"),
    ]);
    with_cookies(Redirect::to(&format!("https://discord.com/oauth2/authorize?{query}")).into_response(), vec![cookie])
}

fn serde_urlencoded_pairs(pairs: &[(&str, &str)]) -> String {
    pairs.iter().map(|(k, v)| format!("{}={}", encode(k), encode(v))).collect::<Vec<_>>().join("&")
}

fn encode(value: &str) -> String {
    let mut out = String::new();
    for b in value.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

fn public_error_detail(message: &str) -> &'static str {
    let text = message.to_lowercase();
    let has = |needles: &[&str]| needles.iter().any(|n| text.contains(n));
    if text.contains("state") {
        "oauth_state"
    } else if text.contains("missing dashboard oauth settings") {
        "missing_setup"
    } else if has(&["invalid_grant", "invalid_client", "invalid client", "client_secret", "client id", "unauthorized_client", "redirect_uri", "redirect uri"]) {
        "oauth_exchange"
    } else if text.contains("access token") {
        "missing_access_token"
    } else if has(&["unknown guild", "guild", "missing access", "404"]) {
        "guild_member_lookup"
    } else if has(&["401", "unauthorized"]) {
        "discord_unauthorized"
    } else if has(&["429", "rate"]) {
        "discord_rate_limited"
    } else {
        "discord_callback"
    }
}

async fn oauth_json(request: reqwest::RequestBuilder) -> Result<Value, (u16, String)> {
    let response = request.send().await.map_err(|e| (0, e.to_string()))?;
    let status = response.status();
    let text = response.text().await.unwrap_or_default();
    let data: Value = serde_json::from_str(&text).unwrap_or_else(|_| json!({ "message": text }));
    if !status.is_success() {
        let message = ["error_description", "error", "message"].iter().find_map(|k| data.get(*k).and_then(Value::as_str)).map(str::to_string).unwrap_or_else(|| format!("Discord API returned {}", status.as_u16()));
        return Err((status.as_u16(), message));
    }
    Ok(data)
}

async fn finish_login(web: &Arc<Web>, headers: &HeaderMap, query: &HashMap<String, String>, oauth: &OAuthConfig, expected_state: &str) -> Result<Option<String>, String> {
    if !oauth.configured() {
        return Err(format!("Missing dashboard OAuth settings: {}", oauth.missing.join(", ")));
    }
    let code = query.get("code").map(String::as_str).unwrap_or("");
    let state = query.get("state").map(String::as_str).unwrap_or("");
    if code.is_empty() || state.is_empty() || state != expected_state {
        return Err("Invalid Discord OAuth state.".into());
    }
    let client = &web.app.web;
    let token = oauth_json(client.post(format!("{DISCORD_API}/oauth2/token")).form(&[
        ("client_id", oauth.client_id.as_str()),
        ("client_secret", oauth.client_secret.as_str()),
        ("grant_type", "authorization_code"),
        ("code", code),
        ("redirect_uri", oauth.redirect_uri.as_str()),
    ]))
    .await
    .map_err(|e| e.1)?;
    let access = token.get("access_token").and_then(Value::as_str).unwrap_or("").to_string();
    if access.is_empty() {
        return Err("Discord did not return an access token.".into());
    }
    let user: Value = oauth_json(client.get(format!("{DISCORD_API}/users/@me")).bearer_auth(&access)).await.map_err(|e| e.1)?;

    let mut roles: Vec<String> = Vec::new();
    let mut members = 0;
    let mut transient: Vec<String> = Vec::new();
    for guild in &oauth.guild_ids {
        match oauth_json(client.get(format!("{DISCORD_API}/users/@me/guilds/{guild}/member")).bearer_auth(&access)).await {
            Ok(member) => {
                members += 1;
                for role in member.get("roles").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str) {
                    if !roles.iter().any(|r| r == role) {
                        roles.push(role.to_string());
                    }
                }
            }
            Err((status, message)) => {
                if ![403, 404].contains(&status) {
                    transient.push(message);
                }
            }
        }
    }
    if members == 0 && !transient.is_empty() {
        return Err(transient.join("; "));
    }
    if !roles.contains(&oauth.allowed_role_id) {
        return Ok(None);
    }
    let user: User = serde_json::from_value(user).map_err(|e| e.to_string())?;
    let _ = headers;
    Ok(Some(web.create_session(&user, roles)))
}

pub async fn callback(State(web): State<Arc<Web>>, headers: HeaderMap, Query(query): Query<HashMap<String, String>>) -> Response {
    let oauth = oauth_config(&web, &headers).await;
    let expected = web.signed_cookie(&headers, STATE_COOKIE).unwrap_or_default();
    let clear_state = set_cookie(&headers, STATE_COOKIE, "", 0);
    match finish_login(&web, &headers, &query, &oauth, &expected).await {
        Ok(Some(session_id)) => {
            let cookie = set_cookie(&headers, SESSION_COOKIE, &web.sign(&session_id), session_ttl_ms());
            with_cookies(Redirect::to("/dashboard").into_response(), vec![clear_state, cookie])
        }
        Ok(None) => with_cookies(Redirect::to("/dashboard?auth=denied").into_response(), vec![clear_state]),
        Err(message) => {
            tracing::error!("Discord dashboard sign in failed: {message} (redirect_uri {}, guilds {:?})", oauth.redirect_uri, oauth.guild_ids);
            with_cookies(Redirect::to(&format!("/dashboard?auth=error&detail={}", encode(public_error_detail(&message)))).into_response(), vec![clear_state])
        }
    }
}

pub async fn logout_api(State(web): State<Arc<Web>>, headers: HeaderMap) -> Response {
    web.destroy_session(&headers);
    with_cookies(Json(json!({ "ok": true })).into_response(), vec![set_cookie(&headers, SESSION_COOKIE, "", 0)])
}

pub async fn logout_redirect(State(web): State<Arc<Web>>, headers: HeaderMap) -> Response {
    web.destroy_session(&headers);
    with_cookies(Redirect::to("/dashboard").into_response(), vec![set_cookie(&headers, SESSION_COOKIE, "", 0)])
}
