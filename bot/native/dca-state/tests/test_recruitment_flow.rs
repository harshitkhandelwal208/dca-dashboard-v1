//! Old-format records written by the Node bot must keep loading (unknown fields are preserved).

use dca_state::stores::*;
use dca_state::StateStore;
use serde_json::json;

#[tokio::test]
async fn legacy_node_records_load_and_keep_unknown_fields() {
    let dir = std::env::temp_dir().join(format!("dca-state-legacy-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("recruitmentTickets.json"),
        serde_json::to_string(&json!({ "tickets": { "111": {
            "threadId": "111", "applicantId": "5", "status": "closed", "team": "Discord", "someFutureField": { "x": 1 },
            "licenseAnalysis": { "inGameName": "Racer", "garagePower": "6195", "eventScores": [], "rawGeminiText": "legacy" }
        } } }))
        .unwrap(),
    )
    .unwrap();

    let store = StateStore::local(&dir);
    let ticket = get_ticket(&store, "111").await.unwrap();
    assert_eq!(ticket.team, "Discord");
    assert_eq!(ticket.license_analysis.as_ref().unwrap().in_game_name, "Racer");

    update_ticket(&store, "111", |t| t.claimed_by_id = "9".into()).await.unwrap();
    let raw: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(dir.join("recruitmentTickets.json")).unwrap()).unwrap();
    assert_eq!(raw["tickets"]["111"]["someFutureField"]["x"], 1);
    assert_eq!(raw["tickets"]["111"]["licenseAnalysis"]["rawGeminiText"], "legacy");
}

#[tokio::test]
async fn damaged_file_is_kept_and_not_silently_lost() {
    let dir = std::env::temp_dir().join(format!("dca-state-damaged-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("warnings.json"), "{ not json").unwrap();
    let store = StateStore::local(&dir);
    assert!(list_warnings(&store, "u", "g").await.is_empty());
    assert!(dir.join("warnings.json.damaged").exists());
}
