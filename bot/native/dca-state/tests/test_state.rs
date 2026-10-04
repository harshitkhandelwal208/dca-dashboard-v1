use dca_state::config::{default_config, load_config, normalize_config, save_config, update_config};
use dca_state::firebase::{from_firestore, to_firestore};
use dca_state::models::{BotLog, RecruitmentBan, SpreadsheetSession, Ticket};
use dca_state::stores::*;
use dca_state::StateStore;
use serde_json::json;

fn temp_store(name: &str) -> StateStore {
    let dir = std::env::temp_dir().join(format!("dca-state-test-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    StateStore::local(dir)
}

#[tokio::test]
async fn config_defaults_roundtrip_and_normalise() {
    let store = temp_store("config");
    let loaded = load_config(&store).await;
    assert_eq!(loaded.recruitment.teams.len(), 4);
    assert_eq!(loaded.member_counts.teams.len(), 7);
    assert_eq!(loaded.spreadsheets.session_window_minutes, 1);
    assert!(!loaded.spreadsheets.enabled);

    // Junk from the dashboard is cleaned the way the JS normaliser did.
    let raw = json!({
        "recruitment": { "panelChannelId": "123", "panelColor": "ff00aa", "maxOpenTicketsPerUser": 99, "teams": ["  Alpha ", "", "Beta"] },
        "spreadsheets": { "sessionWindowMinutes": 500, "outputFormat": "pdf",
            "teams": [{ "id": "x y", "name": "Team X", "monitoredChannelId": "123456789012345678", "ownPlayerAliases": "a, b\nc, a" }] }
    });
    let cfg = normalize_config(&raw, false);
    assert_eq!(cfg.recruitment.panel_channel_id, ""); // not a snowflake
    assert_eq!(cfg.recruitment.panel_color, "#ff00aa");
    assert_eq!(cfg.recruitment.max_open_tickets_per_user, 10);
    assert_eq!(cfg.recruitment.teams, vec!["Alpha", "Beta"]);
    assert_eq!(cfg.spreadsheets.session_window_minutes, 30);
    assert_eq!(cfg.spreadsheets.output_format, "xlsx");
    let team = &cfg.spreadsheets.teams[0];
    assert_ne!(team.id, "x y");
    assert_eq!(team.monitored_channel_id, "123456789012345678");
    assert_eq!(team.own_player_aliases, vec!["a", "b", "c"]);

    let (saved, _) = update_config(&store, |c| c.member_counts.title = "Counts".into()).await.unwrap();
    assert_eq!(saved.member_counts.title, "Counts");
    assert_eq!(load_config(&store).await.member_counts.title, "Counts");

    let mut again = default_config();
    again.welcome.message = "hi {member}".into();
    save_config(&store, &again).await.unwrap();
    assert_eq!(load_config(&store).await.welcome.message, "hi {member}");
}

#[test]
fn firestore_value_conversion_is_lossless() {
    let value = json!({
        "a": 1, "b": 2.5, "c": "text", "d": true, "e": null,
        "f": [1, "x", { "g": [] }], "h": { "i": { "j": "k" } }, "big": 9007199254740993_i64
    });
    assert_eq!(from_firestore(&to_firestore(&value)), value);
}

#[tokio::test]
async fn tickets_logs_and_bans() {
    let store = temp_store("tickets");
    let ticket = Ticket { thread_id: "t1".into(), applicant_id: "u1".into(), status: "open".into(), created_at: "2026-01-01T00:00:00Z".into(), ..Default::default() };
    save_ticket(&store, ticket).await.unwrap();
    assert!(save_ticket(&store, Ticket::default()).await.is_err());
    update_ticket(&store, "t1", |t| t.claimed_by_id = "m1".into()).await.unwrap();
    assert!(update_ticket(&store, "nope", |_| {}).await.unwrap().is_none());
    let got = get_ticket(&store, "t1").await.unwrap();
    assert_eq!(got.claimed_by_id, "m1");
    assert!(!got.updated_at.is_empty());
    assert_eq!(list_tickets(&store, &TicketFilter { status: Some("open".into()), applicant_id: Some("u1".into()) }).await.len(), 1);
    assert_eq!(list_tickets(&store, &TicketFilter { status: Some("closed".into()), ..Default::default() }).await.len(), 0);

    append_recruitment_log(&store, json!({ "outcome": "accepted", "team": "Discord", "closedAt": "2026-01-02T00:00:00Z" })).await.unwrap();
    append_recruitment_log(&store, json!({ "outcome": "rejected", "team": "" })).await.unwrap();
    assert_eq!(list_recruitment_logs(&store, 10).await.len(), 1);

    add_recruitment_ban(&store, RecruitmentBan { user_id: "u9".into(), reason: "spam".into(), ..Default::default() }).await.unwrap();
    add_recruitment_ban(&store, RecruitmentBan { user_id: "u9".into(), user_tag: "u9#0".into(), ..Default::default() }).await.unwrap();
    let bans = list_recruitment_bans(&store).await;
    assert_eq!(bans.len(), 1);
    assert_eq!(bans[0].user_tag, "u9#0");
    assert!(add_recruitment_ban(&store, RecruitmentBan::default()).await.is_err());
}

#[tokio::test]
async fn warnings_reminders_and_logs() {
    let store = temp_store("misc");
    add_warning(&store, "u1", "g1", "spam").await.unwrap();
    add_warning(&store, "u1", "g1", "").await.unwrap();
    add_warning(&store, "u1", "g2", "other guild").await.unwrap();
    assert_eq!(list_warnings(&store, "u1", "g1").await.len(), 2);
    assert_eq!(clear_warnings(&store, "u1", "g1").await.unwrap(), 2);
    assert_eq!(list_warnings(&store, "u1", "g2").await.len(), 1);

    let a = add_reminder(&store, "u1", "c1", "g1", "one", 1_000).await.unwrap();
    let b = add_reminder(&store, "u1", "c1", "g1", "two", 9_000_000_000_000).await.unwrap();
    assert_eq!((a.id, b.id), (1, 2));
    assert!(!remove_reminder(&store, "someone-else", 2).await.unwrap());
    let due = take_due_reminders(&store, 5_000).await;
    assert_eq!(due.len(), 1);
    assert_eq!(due[0].text, "one");
    assert_eq!(list_reminders(&store, Some("u1")).await.len(), 1);
    assert!(remove_reminder(&store, "u1", 2).await.unwrap());

    for i in 0..3 {
        append_bot_log(&store, BotLog { kind: if i == 1 { "ticket".into() } else { "system".into() }, title: format!("t{i}"), ..Default::default() }).await.unwrap();
    }
    let logs = list_bot_logs(&store, 10, "").await;
    assert_eq!(logs.len(), 3);
    assert_eq!(logs[0].title, "t2"); // newest first
    assert_eq!(list_bot_logs(&store, 10, "ticket").await.len(), 1);
}

#[tokio::test]
async fn sessions_report_emissions_and_concurrent_mutations() {
    let store = temp_store("sessions");
    let saved = save_session(&store, SpreadsheetSession { team_id: "t".into(), status: "pending".into(), ..Default::default() }).await.unwrap();
    assert!(saved.id.starts_with("race-"));
    update_session(&store, &saved.id, |s| s.status = "processed".into()).await.unwrap();
    assert_eq!(get_session(&store, &saved.id).await.unwrap().status, "processed");
    assert!(latest_session(&store, "t", &["pending"]).await.is_none());
    assert!(latest_session(&store, "t", &["processed"]).await.is_some());

    assert!(get_report_emission(&store, "t", "weekly", "2026-W01").await.is_none());
    mark_report_emitted(&store, dca_state::models::ReportEmission { team_id: "t".into(), period: "weekly".into(), period_key: "2026-W01".into(), ..Default::default() }).await.unwrap();
    assert!(get_report_emission(&store, "t", "weekly", "2026-W01").await.is_some());

    // 50 concurrent increments must all land (no lost updates).
    let mut handles = Vec::new();
    for _ in 0..50 {
        let store = store.clone();
        handles.push(tokio::spawn(async move {
            add_warning(&store, "u", "g", "x").await.unwrap();
        }));
    }
    for handle in handles {
        handle.await.unwrap();
    }
    assert_eq!(list_warnings(&store, "u", "g").await.len(), 50);
}
