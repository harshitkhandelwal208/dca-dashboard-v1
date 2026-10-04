//! Record stores built on [`StateStore`]. Each mirrors one of the original JS stores (same scope names, same
//! document layout, same caps).

use crate::models::*;
use crate::state_store::StateStore;
use crate::util::{now_iso, now_ms, parse_ms, random_hex};
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::{json, Map, Value};

fn to_value<T: Serialize>(value: &T) -> Value {
    serde_json::to_value(value).unwrap_or(Value::Null)
}

fn from_value<T: DeserializeOwned + Default>(value: &Value) -> T {
    serde_json::from_value(value.clone()).unwrap_or_default()
}

/// Make sure `root[key]` exists and is an object / array, returning it.
fn object_at<'a>(root: &'a mut Value, key: &str) -> &'a mut Map<String, Value> {
    if !root.is_object() {
        *root = json!({});
    }
    let map = root.as_object_mut().unwrap();
    let entry = map.entry(key.to_string()).or_insert_with(|| json!({}));
    if !entry.is_object() {
        *entry = json!({});
    }
    entry.as_object_mut().unwrap()
}

fn array_at<'a>(root: &'a mut Value, key: &str) -> &'a mut Vec<Value> {
    if !root.is_object() {
        *root = json!({});
    }
    let map = root.as_object_mut().unwrap();
    let entry = map.entry(key.to_string()).or_insert_with(|| json!([]));
    if !entry.is_array() {
        *entry = json!([]);
    }
    entry.as_array_mut().unwrap()
}

fn list_of<T: DeserializeOwned + Default>(value: &Value, key: &str) -> Vec<T> {
    value
        .get(key)
        .and_then(Value::as_array)
        .map(|items| items.iter().filter(|v| v.is_object()).map(from_value::<T>).collect())
        .unwrap_or_default()
}

fn s<'a>(value: &'a Value, key: &str) -> &'a str {
    value.get(key).and_then(Value::as_str).unwrap_or("")
}

// =============================================================================================== tickets

const TICKETS_SCOPE: &str = "recruitmentTickets";
const LOGS_SCOPE: &str = "recruitmentLogs";
const MAX_RECRUITMENT_LOGS: usize = 500;

#[derive(Default, Clone)]
pub struct TicketFilter {
    pub status: Option<String>,
    pub applicant_id: Option<String>,
}

pub async fn list_tickets(store: &StateStore, filter: &TicketFilter) -> Vec<Ticket> {
    let state = store.read(TICKETS_SCOPE, json!({ "tickets": {} })).await;
    let mut tickets: Vec<Ticket> = state
        .get("tickets")
        .and_then(Value::as_object)
        .map(|map| map.values().filter(|v| v.is_object()).map(from_value::<Ticket>).collect())
        .unwrap_or_default();
    tickets.retain(|t| filter.status.as_ref().map_or(true, |st| &t.status == st));
    tickets.retain(|t| filter.applicant_id.as_ref().map_or(true, |id| &t.applicant_id == id));
    tickets.sort_by(|a, b| {
        let ka = if a.updated_at.is_empty() { &a.created_at } else { &a.updated_at };
        let kb = if b.updated_at.is_empty() { &b.created_at } else { &b.updated_at };
        kb.cmp(ka)
    });
    tickets
}

pub async fn get_ticket(store: &StateStore, thread_id: &str) -> Option<Ticket> {
    let state = store.read(TICKETS_SCOPE, json!({ "tickets": {} })).await;
    state
        .get("tickets")
        .and_then(|t| t.get(thread_id))
        .filter(|v| v.is_object())
        .map(from_value::<Ticket>)
}

pub async fn save_ticket(store: &StateStore, mut ticket: Ticket) -> Result<Ticket, String> {
    if ticket.thread_id.is_empty() {
        return Err("Ticket is missing threadId.".into());
    }
    ticket.updated_at = now_iso();
    let saved = ticket.clone();
    store
        .mutate(TICKETS_SCOPE, json!({ "tickets": {} }), |state| {
            object_at(state, "tickets").insert(ticket.thread_id.clone(), to_value(&ticket));
        })
        .await?;
    Ok(saved)
}

pub async fn update_ticket(
    store: &StateStore,
    thread_id: &str,
    f: impl FnOnce(&mut Ticket),
) -> Result<Option<Ticket>, String> {
    store
        .mutate(TICKETS_SCOPE, json!({ "tickets": {} }), |state| {
            let tickets = object_at(state, "tickets");
            let current = tickets.get(thread_id).filter(|v| v.is_object())?;
            let mut ticket: Ticket = from_value(current);
            f(&mut ticket);
            ticket.thread_id = thread_id.to_string();
            ticket.updated_at = now_iso();
            tickets.insert(thread_id.to_string(), to_value(&ticket));
            Some(ticket)
        })
        .await
}

pub async fn list_recruitment_logs(store: &StateStore, limit: usize) -> Vec<Value> {
    let state = store.read(LOGS_SCOPE, json!({ "logs": [] })).await;
    let mut logs: Vec<Value> = state
        .get("logs")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter(|log| s(log, "outcome") == "accepted" || !s(log, "team").is_empty())
        .collect();
    logs.sort_by(|a, b| {
        let ka = if s(a, "closedAt").is_empty() { s(a, "createdAt") } else { s(a, "closedAt") };
        let kb = if s(b, "closedAt").is_empty() { s(b, "createdAt") } else { s(b, "closedAt") };
        kb.cmp(ka)
    });
    logs.truncate(limit.clamp(1, 200));
    logs
}

pub async fn append_recruitment_log(store: &StateStore, log: Value) -> Result<Value, String> {
    let mut entry = log;
    if !entry.is_object() {
        entry = json!({});
    }
    let obj = entry.as_object_mut().unwrap();
    obj.entry("id".to_string()).or_insert_with(|| json!(format!("{}-{}", now_ms(), random_hex(12))));
    if obj.get("createdAt").and_then(Value::as_str).unwrap_or("").is_empty() {
        obj.insert("createdAt".into(), json!(now_iso()));
    }
    let saved = entry.clone();
    store
        .mutate(LOGS_SCOPE, json!({ "logs": [] }), |state| {
            let logs = array_at(state, "logs");
            logs.insert(0, entry);
            logs.truncate(MAX_RECRUITMENT_LOGS);
        })
        .await?;
    Ok(saved)
}

/// Change one recruitment log entry (the closing log is completed later by the background reading).
pub async fn update_recruitment_log(store: &StateStore, id: &str, f: impl FnOnce(&mut Value)) -> Result<(), String> {
    store
        .mutate(LOGS_SCOPE, json!({ "logs": [] }), |state| {
            if let Some(entry) = array_at(state, "logs").iter_mut().find(|e| e.get("id").and_then(Value::as_str) == Some(id)) {
                f(entry);
            }
        })
        .await
}

// ================================================================================================= bans

const BANS_SCOPE: &str = "recruitmentBans";

pub async fn list_recruitment_bans(store: &StateStore) -> Vec<RecruitmentBan> {
    let state = store.read(BANS_SCOPE, json!({ "bans": {} })).await;
    let mut bans: Vec<RecruitmentBan> = state
        .get("bans")
        .and_then(Value::as_object)
        .map(|m| m.values().filter(|v| v.is_object()).map(from_value::<RecruitmentBan>).collect())
        .unwrap_or_default();
    bans.sort_by(|a, b| {
        let ka = if a.updated_at.is_empty() { &a.created_at } else { &a.updated_at };
        let kb = if b.updated_at.is_empty() { &b.created_at } else { &b.updated_at };
        kb.cmp(ka)
    });
    bans
}

pub async fn add_recruitment_ban(store: &StateStore, entry: RecruitmentBan) -> Result<RecruitmentBan, String> {
    if entry.user_id.is_empty() {
        return Err("Ban entry is missing userId.".into());
    }
    store
        .mutate(BANS_SCOPE, json!({ "bans": {} }), |state| {
            let bans = object_at(state, "bans");
            let previous: RecruitmentBan = bans.get(&entry.user_id).map(from_value).unwrap_or_default();
            let now = now_iso();
            let next = RecruitmentBan {
                user_id: entry.user_id.clone(),
                user_tag: if entry.user_tag.is_empty() { previous.user_tag.clone() } else { entry.user_tag.clone() },
                reason: if entry.reason.is_empty() { "No reason provided.".into() } else { entry.reason.clone() },
                banned_by_id: entry.banned_by_id.clone(),
                banned_by_tag: entry.banned_by_tag.clone(),
                guild_id: entry.guild_id.clone(),
                created_at: if previous.created_at.is_empty() { now.clone() } else { previous.created_at.clone() },
                updated_at: now,
            };
            bans.insert(entry.user_id.clone(), to_value(&next));
            next
        })
        .await
}

// ============================================================================================ bot logs

const BOT_LOGS_SCOPE: &str = "botLogs";
const MAX_BOT_LOGS: usize = 1000;

pub async fn append_bot_log(store: &StateStore, mut log: BotLog) -> Result<BotLog, String> {
    if log.id.is_empty() {
        log.id = format!("{}-{}", now_ms(), random_hex(12));
    }
    if log.kind.is_empty() {
        log.kind = "system".into();
    }
    if log.title.is_empty() {
        log.title = "Log Entry".into();
    }
    if log.created_at.is_empty() {
        log.created_at = now_iso();
    }
    if log.metadata.is_null() {
        log.metadata = json!({});
    }
    let saved = log.clone();
    store
        .mutate(BOT_LOGS_SCOPE, json!({ "logs": [] }), |state| {
            let logs = array_at(state, "logs");
            logs.insert(0, to_value(&log));
            logs.truncate(MAX_BOT_LOGS);
        })
        .await?;
    Ok(saved)
}

pub async fn list_bot_logs(store: &StateStore, limit: usize, kind: &str) -> Vec<BotLog> {
    let state = store.read(BOT_LOGS_SCOPE, json!({ "logs": [] })).await;
    list_of::<BotLog>(&state, "logs")
        .into_iter()
        .filter(|log| kind.is_empty() || log.kind == kind)
        .take(limit.clamp(1, 500))
        .collect()
}

// ============================================================================================ warnings

const WARNINGS_SCOPE: &str = "warnings";

pub async fn add_warning(store: &StateStore, user_id: &str, guild_id: &str, reason: &str) -> Result<Warning, String> {
    if user_id.is_empty() {
        return Err("Warning is missing userId.".into());
    }
    if guild_id.is_empty() {
        return Err("Warning is missing guildId.".into());
    }
    let warning = Warning {
        id: format!("{}-{}", now_ms(), random_hex(11)),
        user_id: user_id.into(),
        guild_id: guild_id.into(),
        reason: if reason.trim().is_empty() { "No reason provided.".into() } else { reason.into() },
        created_at: now_iso(),
    };
    let saved = warning.clone();
    store
        .mutate(WARNINGS_SCOPE, json!({ "warnings": [] }), |state| {
            array_at(state, "warnings").push(to_value(&warning));
        })
        .await?;
    Ok(saved)
}

pub async fn list_warnings(store: &StateStore, user_id: &str, guild_id: &str) -> Vec<Warning> {
    let state = store.read(WARNINGS_SCOPE, json!({ "warnings": [] })).await;
    let mut list: Vec<Warning> = list_of::<Warning>(&state, "warnings")
        .into_iter()
        .filter(|w| w.user_id == user_id && w.guild_id == guild_id)
        .collect();
    list.sort_by(|a, b| a.created_at.cmp(&b.created_at));
    list
}

pub async fn clear_warnings(store: &StateStore, user_id: &str, guild_id: &str) -> Result<usize, String> {
    store
        .mutate(WARNINGS_SCOPE, json!({ "warnings": [] }), |state| {
            let list = array_at(state, "warnings");
            let before = list.len();
            list.retain(|w| !(s(w, "userId") == user_id && s(w, "guildId") == guild_id));
            before - list.len()
        })
        .await
}

// =========================================================================================== reminders

const REMINDERS_SCOPE: &str = "reminders";

pub async fn add_reminder(
    store: &StateStore,
    user_id: &str,
    channel_id: &str,
    guild_id: &str,
    text: &str,
    due_at_ms: i64,
) -> Result<Reminder, String> {
    store
        .mutate(REMINDERS_SCOPE, json!({ "nextId": 1, "reminders": [] }), |state| {
            let next = state.get("nextId").and_then(Value::as_u64).unwrap_or(1).max(1);
            let reminder = Reminder {
                id: next,
                user_id: user_id.into(),
                channel_id: channel_id.into(),
                guild_id: guild_id.into(),
                text: text.into(),
                due_at: due_at_ms,
                created_at: now_iso(),
            };
            array_at(state, "reminders").push(to_value(&reminder));
            state["nextId"] = json!(next + 1);
            reminder
        })
        .await
}

pub async fn list_reminders(store: &StateStore, user_id: Option<&str>) -> Vec<Reminder> {
    let state = store.read(REMINDERS_SCOPE, json!({ "nextId": 1, "reminders": [] })).await;
    list_of::<Reminder>(&state, "reminders")
        .into_iter()
        .filter(|r| user_id.map_or(true, |id| r.user_id == id))
        .collect()
}

pub async fn remove_reminder(store: &StateStore, user_id: &str, id: u64) -> Result<bool, String> {
    store
        .mutate(REMINDERS_SCOPE, json!({ "nextId": 1, "reminders": [] }), |state| {
            let list = array_at(state, "reminders");
            let before = list.len();
            list.retain(|r| !(r.get("id").and_then(Value::as_u64) == Some(id) && s(r, "userId") == user_id));
            before != list.len()
        })
        .await
}

/// Remove and return every reminder that is due.
pub async fn take_due_reminders(store: &StateStore, now: i64) -> Vec<Reminder> {
    store
        .mutate(REMINDERS_SCOPE, json!({ "nextId": 1, "reminders": [] }), |state| {
            let list = array_at(state, "reminders");
            let mut due = Vec::new();
            list.retain(|r| {
                let reminder: Reminder = from_value(r);
                if reminder.due_at <= now {
                    due.push(reminder);
                    false
                } else {
                    true
                }
            });
            due
        })
        .await
        .unwrap_or_default()
}

// ================================================================================ spreadsheet sessions

const SESSIONS_SCOPE: &str = "spreadsheetSessions";
const REPORTS_SCOPE: &str = "spreadsheetReportEmissions";
const MAX_SESSIONS: usize = 500;

pub fn new_session_id() -> String {
    let stamp = chrono::Utc::now().format("%Y%m%d%H%M%S");
    format!("race-{stamp}-{}", random_hex(6))
}

fn session_sort_key(session: &SpreadsheetSession) -> i64 {
    let value = if session.updated_at.is_empty() { &session.created_at } else { &session.updated_at };
    parse_ms(value).unwrap_or(0)
}

#[derive(Default, Clone)]
pub struct SessionFilter {
    pub team_id: Option<String>,
    pub status: Option<String>,
}

pub async fn list_sessions(store: &StateStore, filter: &SessionFilter) -> Vec<SpreadsheetSession> {
    let state = store.read(SESSIONS_SCOPE, json!({ "sessions": [] })).await;
    // Filter on the raw records first: only the sessions asked for are turned into structs (the stored readings of
    // old sessions make that expensive).
    state
        .get("sessions")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter(|v| v.is_object())
                .filter(|v| filter.team_id.as_ref().map_or(true, |id| s(v, "teamId") == id))
                .filter(|v| filter.status.as_ref().map_or(true, |st| s(v, "status") == st))
                .map(from_value::<SpreadsheetSession>)
                .collect()
        })
        .unwrap_or_default()
}

pub async fn get_session(store: &StateStore, id: &str) -> Option<SpreadsheetSession> {
    let state = store.read(SESSIONS_SCOPE, json!({ "sessions": [] })).await;
    state.get("sessions").and_then(Value::as_array).and_then(|items| items.iter().find(|v| v.is_object() && s(v, "id") == id)).map(from_value::<SpreadsheetSession>)
}

fn write_session(list: &mut Vec<Value>, mut session: SpreadsheetSession) -> SpreadsheetSession {
    let now = now_iso();
    if session.id.is_empty() {
        session.id = new_session_id();
    }
    if session.created_at.is_empty() {
        session.created_at = now.clone();
    }
    session.updated_at = now;
    let value = to_value(&session);
    match list.iter().position(|item| s(item, "id") == session.id) {
        Some(index) => list[index] = value,
        None => list.insert(0, value),
    }
    if list.len() > MAX_SESSIONS {
        // Newest first, keep the newest MAX_SESSIONS.
        let key = |v: &Value| {
            let at = if s(v, "updatedAt").is_empty() { s(v, "createdAt") } else { s(v, "updatedAt") };
            parse_ms(at).unwrap_or(0)
        };
        list.sort_by_key(|v| std::cmp::Reverse(key(v)));
        list.truncate(MAX_SESSIONS);
    }
    session
}

pub async fn save_session(store: &StateStore, session: SpreadsheetSession) -> Result<SpreadsheetSession, String> {
    store
        .mutate(SESSIONS_SCOPE, json!({ "sessions": [] }), |state| write_session(array_at(state, "sessions"), session))
        .await
}

pub async fn update_session(
    store: &StateStore,
    id: &str,
    f: impl FnOnce(&mut SpreadsheetSession),
) -> Result<Option<SpreadsheetSession>, String> {
    store
        .mutate(SESSIONS_SCOPE, json!({ "sessions": [] }), |state| {
            let list = array_at(state, "sessions");
            let current = list.iter().find(|item| s(item, "id") == id)?;
            let mut session: SpreadsheetSession = from_value(current);
            f(&mut session);
            session.id = id.to_string();
            Some(write_session(list, session))
        })
        .await
}

pub async fn latest_session(store: &StateStore, team_id: &str, statuses: &[&str]) -> Option<SpreadsheetSession> {
    list_sessions(store, &SessionFilter { team_id: Some(team_id.into()), status: None })
        .await
        .into_iter()
        .find(|session| statuses.is_empty() || statuses.contains(&session.status.as_str()))
}

/// Drop bulky raw OCR text from old processed sessions. Returns the number of sessions cleaned.
pub async fn cleanup_raw_data(store: &StateStore, retention_days: u32) -> usize {
    let cutoff = now_ms() - (retention_days.max(1) as i64) * 86_400_000;
    store
        .mutate(SESSIONS_SCOPE, json!({ "sessions": [] }), |state| {
            let mut cleaned = 0;
            for item in array_at(state, "sessions").iter_mut() {
                let has_raw = !s(item, "rawOcrText").is_empty() || item.get("readings").and_then(Value::as_array).map_or(false, |a| !a.is_empty());
                if s(item, "status") != "processed" || !has_raw {
                    continue;
                }
                let date = [s(item, "processedAt"), s(item, "updatedAt"), s(item, "createdAt")]
                    .iter()
                    .find(|v| !v.is_empty())
                    .and_then(|v| parse_ms(v))
                    .unwrap_or(0);
                if date == 0 || date > cutoff {
                    continue;
                }
                item["rawOcrText"] = json!("");
                item["readings"] = json!([]);
                item["rawDataCleanedAt"] = json!(now_iso());
                cleaned += 1;
            }
            cleaned
        })
        .await
        .unwrap_or(0)
}

pub async fn get_report_emission(store: &StateStore, team_id: &str, period: &str, period_key: &str) -> Option<ReportEmission> {
    let state = store.read(REPORTS_SCOPE, json!({ "reports": [] })).await;
    list_of::<ReportEmission>(&state, "reports")
        .into_iter()
        .find(|r| r.team_id == team_id && r.period == period && r.period_key == period_key)
}

pub async fn mark_report_emitted(store: &StateStore, mut emission: ReportEmission) -> Result<ReportEmission, String> {
    emission.emitted_at = now_iso();
    let saved = emission.clone();
    store
        .mutate(REPORTS_SCOPE, json!({ "reports": [] }), |state| {
            let list = array_at(state, "reports");
            let value = to_value(&emission);
            match list.iter().position(|r| {
                s(r, "teamId") == emission.team_id && s(r, "period") == emission.period && s(r, "periodKey") == emission.period_key
            }) {
                Some(index) => list[index] = value,
                None => list.insert(0, value),
            }
            list.sort_by(|a, b| s(b, "emittedAt").cmp(s(a, "emittedAt")));
            list.truncate(500);
        })
        .await?;
    Ok(saved)
}

// ====================================================================================== team role queue

const ASSIGNMENTS_SCOPE: &str = "teamRoleAssignments";

pub async fn queue_assignments(store: &StateStore, items: Vec<TeamRoleAssignment>) -> Result<(), String> {
    store
        .mutate(ASSIGNMENTS_SCOPE, json!({ "assignments": {} }), |state| {
            let map = object_at(state, "assignments");
            for item in items {
                map.insert(item.id.clone(), to_value(&item));
            }
        })
        .await
}

pub async fn list_assignments(store: &StateStore) -> Vec<TeamRoleAssignment> {
    let state = store.read(ASSIGNMENTS_SCOPE, json!({ "assignments": {} })).await;
    state
        .get("assignments")
        .and_then(Value::as_object)
        .map(|m| m.values().filter(|v| v.is_object()).map(from_value::<TeamRoleAssignment>).collect())
        .unwrap_or_default()
}

pub async fn save_assignments(store: &StateStore, items: Vec<TeamRoleAssignment>) -> Result<(), String> {
    queue_assignments(store, items).await
}
