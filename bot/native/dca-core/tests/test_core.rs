//! Pure-logic tests for the spreadsheet pipeline (no models needed).

use dca_core::calc::*;
use dca_core::metrics::*;
use dca_core::report::*;
use dca_core::session::*;
use dca_state::models::*;

fn player(rank: u32, name: &str, own: bool, score: i64) -> Player {
    Player {
        rank,
        placement: rank,
        player_name: name.into(),
        team_type: if own { "own".into() } else { "opponent".into() },
        team_label: if own { "Discord 3\u{2122}".into() } else { "Opponent".into() },
        points: Some(event_points_for_rank(rank)),
        score: Some(score),
        ..Default::default()
    }
}

fn sample_session(id: &str, date: &str, title: &str, kab_first: bool) -> SpreadsheetSession {
    let mut players = vec![
        player(if kab_first { 1 } else { 2 }, "Alpha", true, 24000),
        player(if kab_first { 2 } else { 1 }, "Blue One", false, 25000),
        player(3, "Bravo", true, 21000),
        player(4, "Blue Two", false, 20000),
        player(5, "Charlie", true, 19000),
    ];
    players.sort_by_key(|p| p.rank);
    let stats = summarize(&players);
    SpreadsheetSession {
        id: id.into(),
        team_id: "discord3".into(),
        team_name: "Discord 3\u{2122}".into(),
        status: "processed".into(),
        created_at: date.into(),
        processed_at: date.into(),
        metadata: Metadata {
            title: title.into(),
            event_name: title.into(),
            own_team_name: "Discord 3\u{2122}".into(),
            teams: vec![serde_json::json!({ "label": "BANGLADESH", "teamType": "opponent" })],
            team_scores: Some(TeamScores { own: Some(2963.0), opponent: Some(1514.0), raw_line: "Discord 3\u{2122} 2963 vs BANGLADESH 1514".into() }),
            ..Default::default()
        },
        players,
        stats,
        ..Default::default()
    }
}

#[test]
fn event_points_follow_the_rank_table() {
    assert_eq!(event_points_for_rank(1), 300);
    assert_eq!(event_points_for_rank(57), 7);
    assert_eq!(event_points_for_rank(100), 0);
    assert_eq!(event_points_for_rank(0), 0);
}

#[test]
fn summary_kab_and_buckets() {
    let s = sample_session("s1", "2026-09-30T12:00:00Z", "Sky-Rock Samba", true);
    // Own player at rank 1 beat every opponent: #KAB.
    assert_eq!(s.stats.kab_players, vec!["Alpha"]);
    assert_eq!(s.stats.own_players, 3);
    assert_eq!(s.stats.opponents, 2);
    assert_eq!(s.stats.top_opponent_rank, Some(2));
    assert_eq!(s.players[0].player_name, "Alpha");
    assert_eq!(s.stats.buckets[0].own, 2);
    assert_eq!(blues_killed_for_rank(&s, 1), 2);
    assert_eq!(blues_killed_for_rank(&s, 3), 1);
    let k = session_kab_map(&s);
    assert_eq!(k["Alpha"], 1);
    assert_eq!(k["Bravo"], 0);
}

#[test]
fn corrections_rebuild_stats() {
    let s = sample_session("s1", "2026-09-30T12:00:00Z", "Sky-Rock Samba", false);
    let parsed = Parsed { metadata: s.metadata.clone(), players: s.players.clone(), stats: s.stats.clone(), raw_text: String::new() };
    let fixed = apply_corrections(
        parsed,
        &[
            Correction { row: 3, field: "player_name".into(), value: "Bravo2".into(), ..Default::default() },
            Correction { row: 4, field: "team_type".into(), value: "own".into(), ..Default::default() },
            Correction { row: 1, field: "event_name".into(), value: "Rocky Rush".into(), ..Default::default() },
            Correction { row: 5, field: "score".into(), value: "19 500".into(), ..Default::default() },
        ],
    );
    assert_eq!(fixed.metadata.event_name, "Rocky Rush");
    assert!(fixed.players.iter().any(|p| p.player_name == "Bravo2"));
    assert_eq!(fixed.stats.own_players, 4);
    assert_eq!(fixed.players.iter().find(|p| p.rank == 5).unwrap().score, Some(19_500));
}

#[test]
fn opponent_team_cleaning() {
    assert_eq!(clean_team_name("Discord 3\u{2122} 2 963 vs BANGLADESH 1 514", "Discord 3\u{2122}"), "BANGLADESH");
    assert_eq!(clean_team_name("", ""), "");
    let s = sample_session("s1", "2026-09-30T12:00:00Z", "T", false);
    assert_eq!(session_opponent_teams(&s), vec!["BANGLADESH"]);
    assert_eq!(team_score_from_metadata(&s, true), 2963);
}

#[test]
fn xlsx_fods_and_images_build() {
    let s = sample_session("s1", "2026-09-30T12:00:00Z", "Sky-Rock Samba", true);
    let xlsx = build_xlsx(&s).unwrap();
    assert!(xlsx.starts_with(b"PK"));
    let fods = build_fods(&s);
    assert!(fods.contains("Attendance") && fods.contains("Alpha"));
    assert!(build_chart_svg(&s).contains("Placement buckets"));
    assert!(build_spreadsheet_image_svg(&s).contains("Alpha"));
    let rows = result_workbook_rows(&s);
    assert_eq!(rows.len(), 6);
    assert!(rows[1].kab); // Alpha is #KAB
}

#[test]
fn period_bounds_match_the_old_bot() {
    let anchor = chrono::DateTime::parse_from_rfc3339("2026-10-04T10:00:00Z").unwrap().with_timezone(&chrono::Utc);
    let weekly = period_bounds("weekly", anchor);
    assert_eq!(weekly.label, "Week of 2026-09-28");
    assert_eq!(weekly.key, "2026-W40");
    let monthly = period_bounds("monthly", anchor);
    assert_eq!(monthly.label, "October 2026");
    assert_eq!(monthly.key, "2026-10");
    assert!(parse_anchor_date("2026-13-40").is_err());
}

#[test]
fn weekly_and_monthly_reports() {
    let mut a = sample_session("s1", "2026-09-29T12:00:00Z", "Sky-Rock Samba", false);
    a.attendance.missing_players = vec![];
    let b = sample_session("s2", "2026-09-30T12:00:00Z", "Sky-Rock  Samba", true);
    let other = sample_session("s3", "2026-09-30T18:00:00Z", "Totally Different", false);
    let all = vec![a, b, other];
    let anchor = chrono::DateTime::parse_from_rfc3339("2026-10-01T00:00:00Z").unwrap().with_timezone(&chrono::Utc);
    let bounds = period_bounds("weekly", anchor);
    let sessions = filter_sessions_for_report(&all, "weekly", &bounds, anchor, Some("Sky-Rock Samba"), None);
    assert_eq!(sessions.len(), 2); // other event excluded, OCR spacing tolerated
    let aliases = vec!["Delta".to_string()];
    let team = TeamInfo { id: "discord3", name: "Discord 3\u{2122}", own_player_aliases: &aliases };
    let model = build_report_model(&team, &sessions, "weekly", &bounds, &all);
    assert_eq!(model.events.len(), 2);
    let delta = model.rows.iter().find(|r| r.name == "Delta").unwrap();
    assert_eq!(delta.missed, 2); // known roster member who attended nothing scores 0
    let alpha = model.rows.iter().find(|r| r.name == "Alpha").unwrap();
    assert_eq!(alpha.attended, 2);
    assert!(alpha.kab >= 1);
    assert!(build_report_xlsx(&model).unwrap().starts_with(b"PK"));
    assert!(build_report_table_svg(&model).contains("Alpha"));
    assert!(build_report_chart_svg(&model).contains("blues"));
    assert!(report_embed_description(&model).contains("Top drivers"));

    let mb = period_bounds("monthly", anchor);
    let monthly_sessions = filter_sessions_for_report(&all, "monthly", &mb, anchor, None, None);
    assert_eq!(monthly_sessions.len(), 0); // all sessions are in September
    let sep = chrono::DateTime::parse_from_rfc3339("2026-09-15T00:00:00Z").unwrap().with_timezone(&chrono::Utc);
    let sb = period_bounds("monthly", sep);
    let monthly_sessions = filter_sessions_for_report(&all, "monthly", &sb, sep, None, None);
    assert_eq!(monthly_sessions.len(), 3);
    let monthly = build_report_model(&team, &monthly_sessions, "monthly", &sb, &all);
    assert!(build_report_table_svg(&monthly).contains("Enemy team(s)"));
}

#[test]
fn same_event_names_tolerate_ocr_noise() {
    assert!(same_event_name("Sky-Rock Samba", "Sky-RockSamba"));
    assert!(same_event_name("Sky-Rock Samba", "Sky-Rock Sarnba"));
    assert!(!same_event_name("Sky-Rock Samba", "Rocky Rush"));
}

/// `DCA_STATS_IN=<report cases with the old bot's stats>`: the Rust `summarize` must reproduce them.
#[test]
fn summarize_matches_the_old_bot() {
    let Ok(path) = std::env::var("DCA_STATS_IN") else { return };
    let cases: Vec<serde_json::Value> = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let mut checked = 0;
    for case in cases {
        for session in case["sessions"].as_array().unwrap() {
            let players: Vec<dca_state::models::Player> = serde_json::from_value(session["players"].clone()).unwrap();
            let ours = serde_json::to_value(dca_core::session::summarize(&players)).unwrap();
            let theirs = &session["stats"];
            for key in [
                "totalPlayers",
                "ownPlayers",
                "opponents",
                "ownAverageRank",
                "opponentAverageRank",
                "ownPoints",
                "opponentPoints",
                "ownScore",
                "opponentScore",
                "ownTop10",
                "opponentTop10",
                "topOpponentRank",
                "kabCount",
                "kabPlayers",
                "buckets",
                "opponentsBelowByPlayer",
            ] {
                if let (Some(a), Some(b)) = (ours[key].as_f64(), theirs[key].as_f64()) {
                    assert!((a - b).abs() < 1e-9, "{key}: {a} vs {b}");
                } else {
                    assert_eq!(ours[key], theirs[key], "{key}");
                }
            }
            checked += 1;
        }
    }
    assert!(checked > 0);
}
