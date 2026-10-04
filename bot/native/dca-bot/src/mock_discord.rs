//! A tiny fake Discord REST server for tests: serenity's `Http` is pointed at it (the proxy option), every request is
//! recorded, and the endpoints the bot uses answer with plausible objects. Lets whole flows (applying, closing,
//! spreadsheet posts...) run for real without a Discord connection.

use axum::body::Bytes;
use axum::extract::{FromRequest, Multipart, Request, State};
use axum::http::{header, Method, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use serde_json::{json, Value};
use serenity::http::{Http, HttpBuilder};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

#[derive(Clone, Debug)]
pub struct Call {
    pub method: String,
    pub path: String,
    pub body: Value,
    /// Names of uploaded files (multipart messages).
    pub files: Vec<String>,
}

#[derive(Default)]
struct Inner {
    calls: Vec<Call>,
    files: HashMap<String, (Vec<u8>, String)>,
    /// Messages per channel (most recent last).
    messages: HashMap<String, Vec<Value>>,
    /// Channel type and parent registered by tests / created threads: id -> (type, guild, name, parent).
    channels: HashMap<String, (u8, String, String, String)>,
    /// Members: user id -> role ids.
    roles: HashMap<String, Vec<String>>,
    /// Make these routes fail (substring of "METHOD path") with the given status.
    failures: Vec<(String, u16)>,
}

pub struct MockDiscord {
    pub base: String,
    inner: Arc<Mutex<Inner>>,
    next: Arc<AtomicU64>,
}

pub const BOT_ID: u64 = 900_000_000_000_000_001;
pub const GUILD_ID: u64 = 800_000_000_000_000_001;
/// Roles every guild of the mock has: Administrator and Ban Members.
pub const ADMIN_ROLE: u64 = 820_000_000_000_000_009;
pub const BAN_ROLE: u64 = 820_000_000_000_000_008;

fn user_json(id: &str, name: &str, bot: bool) -> Value {
    json!({ "id": id, "username": name, "discriminator": "0", "global_name": name, "avatar": null, "bot": bot })
}

impl MockDiscord {
    pub async fn start() -> MockDiscord {
        let inner = Arc::new(Mutex::new(Inner::default()));
        let next = Arc::new(AtomicU64::new(700_000_000_000_000_000));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
        let state = (inner.clone(), next.clone(), base.clone());
        let router = Router::new().route("/files/{n}/{name}", get(serve_file)).fallback(handle).with_state(state);
        tokio::spawn(async move { axum::serve(listener, router).await.ok() });
        MockDiscord { base, inner, next }
    }

    pub fn http(&self) -> Arc<Http> {
        Arc::new(HttpBuilder::new("Bot test-token").proxy(self.base.clone()).ratelimiter_disabled(true).application_id(serenity::model::id::ApplicationId::new(BOT_ID)).build())
    }

    pub fn next_id(&self) -> u64 {
        self.next.fetch_add(1, Ordering::SeqCst)
    }

    /// Host a file and return its URL (as an attachment URL would look).
    pub fn file_url(&self, bytes: Vec<u8>, name: &str) -> String {
        let n = self.next_id();
        let kind = if name.ends_with(".png") { "image/png" } else { "image/jpeg" };
        self.inner.lock().unwrap().files.insert(format!("{n}/{name}"), (bytes, kind.to_string()));
        format!("{}/files/{n}/{name}", self.base)
    }

    pub fn register_channel(&self, id: u64, kind: u8, name: &str) {
        self.inner.lock().unwrap().channels.insert(id.to_string(), (kind, GUILD_ID.to_string(), name.to_string(), String::new()));
    }

    pub fn set_member_roles(&self, user: u64, roles: &[u64]) {
        self.inner.lock().unwrap().roles.insert(user.to_string(), roles.iter().map(|r| r.to_string()).collect());
    }

    pub fn fail(&self, route_contains: &str, status: u16) {
        self.inner.lock().unwrap().failures.push((route_contains.to_string(), status));
    }

    pub fn calls(&self) -> Vec<Call> {
        self.inner.lock().unwrap().calls.clone()
    }

    pub fn calls_matching(&self, method: &str, path_contains: &str) -> Vec<Call> {
        self.calls().into_iter().filter(|c| c.method == method && c.path.contains(path_contains)).collect()
    }

    pub fn messages_in(&self, channel: u64) -> Vec<Value> {
        self.inner.lock().unwrap().messages.get(&channel.to_string()).cloned().unwrap_or_default()
    }

    pub fn all_messages(&self) -> Vec<(String, Value)> {
        self.inner.lock().unwrap().messages.iter().flat_map(|(c, v)| v.iter().map(|m| (c.clone(), m.clone()))).collect()
    }
}

type St = (Arc<Mutex<Inner>>, Arc<AtomicU64>, String);

async fn serve_file(State((inner, _, _)): State<St>, axum::extract::Path((n, name)): axum::extract::Path<(String, String)>) -> Response {
    match inner.lock().unwrap().files.get(&format!("{n}/{name}")).cloned() {
        Some((bytes, kind)) => ([(header::CONTENT_TYPE, kind)], bytes).into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

fn message_json(id: u64, channel: &str, content: &str, embeds: Value, attachments: Value, components: Value) -> Value {
    // Posted "a minute ago", so age-based logic (the panel channel sweep) sees an ordinary message.
    let posted = serenity::all::Timestamp::from_unix_timestamp(serenity::all::Timestamp::now().unix_timestamp() - 60).unwrap().to_string();
    json!({
        "id": id.to_string(), "channel_id": channel, "author": user_json(&BOT_ID.to_string(), "dcabot", true), "content": content,
        "timestamp": posted, "edited_timestamp": null, "tts": false, "mention_everyone": false,
        "mentions": [], "mention_roles": [], "attachments": attachments, "embeds": embeds, "pinned": false, "type": 0, "components": components,
    })
}

fn channel_json(id: &str, kind: u8, guild: &str, name: &str, parent: &str) -> Value {
    let mut v = json!({ "id": id, "type": kind, "guild_id": guild, "name": name, "position": 0, "permission_overwrites": [], "nsfw": false });
    if kind == 11 || kind == 12 {
        v["parent_id"] = json!(parent);
        v["owner_id"] = json!(BOT_ID.to_string());
        v["message_count"] = json!(0);
        v["member_count"] = json!(1);
        v["thread_metadata"] = json!({ "archived": false, "auto_archive_duration": 60, "archive_timestamp": "2026-10-04T12:00:00.000000+00:00", "locked": false, "invitable": false });
    } else if kind != 1 {
        v["parent_id"] = Value::Null;
    }
    v
}

async fn handle(State((inner, next, base)): State<St>, request: Request) -> Response {
    let method = request.method().clone();
    let path = request.uri().path().to_string();
    let content_type = request.headers().get(header::CONTENT_TYPE).and_then(|v| v.to_str().ok()).unwrap_or("").to_string();
    let route = path.trim_start_matches("/api/v10").to_string();
    let seg: Vec<&str> = route.trim_matches('/').split('/').collect();
    let alloc = || next.fetch_add(1, Ordering::SeqCst);

    let (mut body, mut files): (Value, Vec<(String, Vec<u8>)>) = (Value::Null, Vec::new());
    if content_type.starts_with("multipart/") {
        if let Ok(mut multipart) = Multipart::from_request(request, &()).await {
            while let Ok(Some(field)) = multipart.next_field().await {
                let name = field.name().unwrap_or("").to_string();
                let file_name = field.file_name().map(str::to_string);
                let data = field.bytes().await.unwrap_or_default();
                if name == "payload_json" {
                    body = serde_json::from_slice(&data).unwrap_or(Value::Null);
                } else if let Some(file_name) = file_name {
                    files.push((file_name, data.to_vec()));
                }
            }
        }
    } else {
        let bytes: Bytes = axum::body::to_bytes(request.into_body(), 64 * 1024 * 1024).await.unwrap_or_default();
        body = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    }
    inner.lock().unwrap().calls.push(Call { method: method.to_string(), path: route.clone(), body: body.clone(), files: files.iter().map(|f| f.0.clone()).collect() });

    let key = format!("{method} {route}");
    if let Some((_, status)) = inner.lock().unwrap().failures.iter().find(|(r, _)| key.contains(r)) {
        return (StatusCode::from_u16(*status).unwrap(), axum::Json(json!({ "message": "mock failure", "code": 50013 }))).into_response();
    }

    let ok = |v: Value| axum::Json(v).into_response();
    let no_content = || StatusCode::NO_CONTENT.into_response();
    let guild = GUILD_ID.to_string();

    // Store uploaded files and build attachment objects pointing at them.
    let mut attachments = Vec::new();
    for (name, data) in files {
        let n = alloc();
        let kind = if name.ends_with(".png") { "image/png" } else if name.ends_with(".xlsx") { "application/octet-stream" } else { "image/jpeg" };
        inner.lock().unwrap().files.insert(format!("{n}/{name}"), (data.clone(), kind.to_string()));
        let url = format!("{base}/files/{n}/{name}");
        attachments.push(json!({ "id": n.to_string(), "filename": name, "size": data.len(), "url": url, "proxy_url": url, "content_type": kind }));
    }

    match (method.clone(), seg.as_slice()) {
        (Method::GET, ["users", "@me"]) => ok(user_json(&BOT_ID.to_string(), "dcabot", true)),
        (Method::GET, ["users", id]) => ok(user_json(id, &format!("user{}", &id[id.len().saturating_sub(4)..]), false)),
        (Method::POST, ["users", "@me", "channels"]) => {
            let recipient = body["recipient_id"].as_str().unwrap_or("1").to_string();
            let id = format!("5{}", &recipient[1.min(recipient.len())..]);
            inner.lock().unwrap().channels.insert(id.clone(), (1, String::new(), String::new(), String::new()));
            ok(json!({ "id": id, "type": 1, "recipients": [user_json(&recipient, "recipient", false)], "last_message_id": null }))
        }
        (Method::GET, ["channels", id]) => {
            let known = inner.lock().unwrap().channels.get(*id).cloned();
            let (kind, g, name, parent) = known.unwrap_or((0, guild.clone(), "general".into(), String::new()));
            ok(channel_json(id, kind, &g, &name, &parent))
        }
        (Method::PATCH, ["channels", id]) => {
            let known = inner.lock().unwrap().channels.get(*id).cloned();
            let (kind, g, name, parent) = known.unwrap_or((12, guild.clone(), "thread".into(), String::new()));
            let mut v = channel_json(id, kind, &g, body["name"].as_str().unwrap_or(&name), &parent);
            if kind == 11 || kind == 12 {
                v["thread_metadata"]["archived"] = body["archived"].clone();
                v["thread_metadata"]["locked"] = body["locked"].clone();
            }
            ok(v)
        }
        (Method::DELETE, ["channels", id]) => ok(channel_json(id, 12, &guild, "thread", "")),
        (Method::GET, ["channels", id, "messages"]) => ok(Value::Array(inner.lock().unwrap().messages.get(*id).cloned().unwrap_or_default().into_iter().rev().collect())),
        (Method::GET, ["channels", id, "messages", mid]) => {
            let found = inner.lock().unwrap().messages.get(*id).and_then(|v| v.iter().find(|m| m["id"] == *mid).cloned());
            match found {
                Some(m) => ok(m),
                None => (StatusCode::NOT_FOUND, axum::Json(json!({ "message": "Unknown Message", "code": 10008 }))).into_response(),
            }
        }
        (Method::POST, ["channels", id, "messages"]) => {
            let message = message_json(alloc(), id, body["content"].as_str().unwrap_or(""), body["embeds"].clone().as_array().map(|_| body["embeds"].clone()).unwrap_or(json!([])), Value::Array(attachments), body["components"].clone().as_array().map(|_| body["components"].clone()).unwrap_or(json!([])));
            inner.lock().unwrap().messages.entry(id.to_string()).or_default().push(message.clone());
            ok(message)
        }
        (Method::PATCH, ["channels", id, "messages", mid]) => {
            let mut store = inner.lock().unwrap();
            let list = store.messages.entry(id.to_string()).or_default();
            if let Some(m) = list.iter_mut().find(|m| m["id"] == *mid) {
                for key in ["content", "embeds", "components"] {
                    if !body[key].is_null() {
                        m[key] = body[key].clone();
                    }
                }
                return ok(m.clone());
            }
            (StatusCode::NOT_FOUND, axum::Json(json!({ "message": "Unknown Message", "code": 10008 }))).into_response()
        }
        (Method::DELETE, ["channels", id, "messages", mid]) => {
            if let Some(list) = inner.lock().unwrap().messages.get_mut(*id) {
                list.retain(|m| m["id"] != *mid);
            }
            no_content()
        }
        (Method::POST, ["channels", id, "messages", "bulk-delete"]) => {
            let ids: Vec<String> = body["messages"].as_array().map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect()).unwrap_or_default();
            if let Some(list) = inner.lock().unwrap().messages.get_mut(*id) {
                list.retain(|m| !ids.iter().any(|i| m["id"] == *i));
            }
            no_content()
        }
        (Method::POST, ["channels", id, "threads"]) => {
            let tid = alloc().to_string();
            let kind = body["type"].as_u64().unwrap_or(11) as u8;
            let name = body["name"].as_str().unwrap_or("thread").to_string();
            inner.lock().unwrap().channels.insert(tid.clone(), (kind, guild.clone(), name.clone(), id.to_string()));
            ok(channel_json(&tid, kind, &guild, &name, id))
        }
        (Method::PUT, ["channels", _, "thread-members", _]) | (Method::DELETE, ["channels", _, "thread-members", _]) => no_content(),
        (Method::GET, ["channels", id, "pins"]) | (Method::GET, ["channels", id, "invites"]) => {
            let _ = id;
            ok(json!([]))
        }
        (Method::POST, ["channels", _, "invites"]) => ok(json!({ "code": "abc123", "channel": { "id": "1", "name": "welcome", "type": 0 }, "created_at": "2026-10-04T12:00:00.000000+00:00", "max_age": 0, "max_uses": 1, "temporary": false, "uses": 0, "inviter": user_json(&BOT_ID.to_string(), "dcabot", true) })),
        (Method::PUT, ["channels", ..]) | (Method::DELETE, ["channels", ..]) => no_content(),
        (Method::POST, ["interactions", _, _, "callback"]) => no_content(),
        (Method::PATCH, ["webhooks", _, _, "messages", "@original"]) | (Method::POST, ["webhooks", _, _]) => {
            let channel = "1";
            let message = message_json(alloc(), channel, body["content"].as_str().unwrap_or(""), body["embeds"].clone().as_array().map(|_| body["embeds"].clone()).unwrap_or(json!([])), Value::Array(attachments), json!([]));
            inner.lock().unwrap().messages.entry(format!("interaction:{}", route.split('/').nth(2).unwrap_or(""))).or_default().push(message.clone());
            ok(message)
        }
        (Method::GET, ["guilds", id]) => ok(json!({ "id": id, "name": "Test Guild", "icon": null, "owner_id": "100000000000000001", "roles": [
            { "id": id, "name": "@everyone", "color": 0, "colors": { "primary_color": 0, "secondary_color": null, "tertiary_color": null }, "hoist": false, "position": 0, "permissions": "0", "managed": false, "mentionable": false },
            { "id": ADMIN_ROLE.to_string(), "name": "Admin", "color": 0, "colors": { "primary_color": 0, "secondary_color": null, "tertiary_color": null }, "hoist": false, "position": 5, "permissions": "8", "managed": false, "mentionable": false },
            { "id": BAN_ROLE.to_string(), "name": "Moderator", "color": 0, "colors": { "primary_color": 0, "secondary_color": null, "tertiary_color": null }, "hoist": false, "position": 4, "permissions": "4", "managed": false, "mentionable": false },
        ], "afk_timeout": 300, "verification_level": 0, "default_message_notifications": 0, "explicit_content_filter": 0, "emojis": [], "stickers": [], "features": [], "mfa_level": 0, "system_channel_flags": 0, "premium_tier": 0, "preferred_locale": "en-US", "nsfw_level": 0, "premium_progress_bar_enabled": false, "approximate_member_count": 42 })),
        (Method::GET, ["guilds", _, "channels"]) => ok(json!([])),
        (Method::GET, ["guilds", _, "bans", _]) => (StatusCode::NOT_FOUND, axum::Json(json!({ "message": "Unknown Ban", "code": 10026 }))).into_response(),
        (Method::PUT, ["guilds", _, "bans", _]) | (Method::DELETE, ["guilds", _, "bans", _]) | (Method::DELETE, ["guilds", _, "members", _]) | (Method::PATCH, ["guilds", _, "members", _]) => no_content(),
        (Method::GET, ["guilds", _, "members"]) => {
            let members: Vec<Value> = inner.lock().unwrap().roles.iter().map(|(uid, roles)| json!({ "user": user_json(uid, &format!("user{}", &uid[uid.len().saturating_sub(4)..]), false), "roles": roles, "joined_at": "2026-01-01T00:00:00.000000+00:00", "deaf": false, "mute": false, "flags": 0 })).collect();
            ok(Value::Array(members))
        }
        (Method::GET, ["guilds", id, "members", uid]) => {
            let roles = inner.lock().unwrap().roles.get(*uid).cloned().unwrap_or_default();
            let _ = id;
            ok(json!({ "user": user_json(uid, &format!("user{}", &uid[uid.len().saturating_sub(4)..]), false), "roles": roles, "joined_at": "2026-01-01T00:00:00.000000+00:00", "deaf": false, "mute": false, "flags": 0 }))
        }
        (Method::PUT, ["guilds", _, "members", uid, "roles", rid]) => {
            inner.lock().unwrap().roles.entry(uid.to_string()).or_default().push(rid.to_string());
            no_content()
        }
        (Method::DELETE, ["guilds", _, "members", uid, "roles", rid]) => {
            if let Some(r) = inner.lock().unwrap().roles.get_mut(*uid) {
                r.retain(|x| x != rid);
            }
            no_content()
        }
        _ => (StatusCode::NOT_FOUND, axum::Json(json!({ "message": "Unknown route in mock", "code": 0 }))).into_response(),
    }
}
