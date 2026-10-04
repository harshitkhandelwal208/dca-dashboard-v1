//! End-to-end checks of the spreadsheet pipeline (download -> PaddleOCR -> session -> xlsx/PNG outputs) without Discord.
//! They need the PaddleOCR models (`bot/scripts/download-models.sh`) and skip themselves when those are missing.

use crate::app::{App, Dirs};
use crate::managers::spreadsheet::{process_spreadsheet_session, rebuild_spreadsheet_session, ProcessOptions};
use dca_core::reader::Reader;
use dca_state::config::{update_config, SpreadsheetTeam};
use dca_state::models::{Attachment, SpreadsheetSession};
use dca_state::stores::{get_session, save_session};
use dca_state::StateStore;
use serenity::http::Http;
use std::path::PathBuf;
use std::sync::Arc;

fn bot_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..").canonicalize().unwrap()
}

fn models_ready(dir: &std::path::Path) -> bool {
    dca_core::ocr::core_files().iter().all(|f| dir.join(f).exists())
}

async fn serve_file(bytes: Vec<u8>) -> String {
    let router = axum::Router::new().route("/shot.jpg", axum::routing::get(move || {
        let bytes = bytes.clone();
        async move { ([("content-type", "image/jpeg")], bytes) }
    }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move { axum::serve(listener, router).await.ok() });
    format!("http://127.0.0.1:{port}/shot.jpg")
}

#[tokio::test(flavor = "multi_thread")]
async fn team_event_screenshot_becomes_a_spreadsheet_session() {
    let bot = bot_dir().join("bot");
    let models = bot.join("models");
    if !models_ready(&models) {
        eprintln!("skipping: PaddleOCR models not downloaded");
        return;
    }
    let sample = bot.join("fixtures/guides/team-event-score.jpg");
    let dir = std::env::temp_dir().join(format!("dca-e2e-{}", std::process::id()));
    let dirs = Dirs { data: dir.clone(), fonts: bot.join("fonts"), models: models.clone(), assets: bot.join("assets"), dashboard: dir.join("dist") };
    let app = App::new(Arc::new(Http::new("test-token")), StateStore::local(&dir), dirs.clone());
    *app.reader.write().unwrap() = Some(Arc::new(Reader::new(&models, &dirs.fonts).expect("models load")));

    update_config(&app.store, |c| {
        c.spreadsheets.enabled = true;
        c.spreadsheets.teams = vec![SpreadsheetTeam {
            id: "discord3".into(),
            name: "Discord 3".into(),
            enabled: true,
            monitored_channel_id: "1".into(),
            own_team_aliases: vec!["Discord 3".into()],
            auto_process: true,
            ..Default::default()
        }];
    })
    .await
    .unwrap();

    let url = serve_file(std::fs::read(&sample).unwrap()).await;
    let session = save_session(
        &app.store,
        SpreadsheetSession {
            id: "race-test-1".into(),
            team_id: "discord3".into(),
            team_name: "Discord 3".into(),
            status: "pending".into(),
            images: vec![Attachment { id: "1".into(), name: "shot.jpg".into(), url, ..Default::default() }],
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let processed = process_spreadsheet_session(&app, &session.id, ProcessOptions { rerun_ocr: true }).await.expect("processing succeeds");
    assert_eq!(processed.status, "processed");
    let names: Vec<String> = processed.players.iter().map(|p| p.player_name.to_lowercase()).collect();
    for expected in ["md.tufayel", "primax", "zafer"] {
        assert!(names.iter().any(|n| n.contains(&expected[..4])), "{expected} missing from {names:?}");
    }
    assert!(processed.players.len() >= 7, "{} players: {names:?}", processed.players.len());
    assert!(processed.players.iter().any(|p| p.team_type == "own") && processed.players.iter().any(|p| p.team_type == "opponent"));
    for path in [&processed.outputs.spreadsheet_path, &processed.outputs.chart_path, &processed.outputs.spreadsheet_image_path] {
        assert!(!path.is_empty() && std::path::Path::new(path).exists(), "missing output {path:?}");
    }

    // Stored readings let the outputs be rebuilt without running OCR again.
    let rebuilt = rebuild_spreadsheet_session(&app, &session.id).await.expect("rebuild");
    assert_eq!(rebuilt.players.len(), processed.players.len());
    assert!(get_session(&app.store, &session.id).await.unwrap().readings.iter().any(|r| !r.rows.is_empty()));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_instant_check_recognises_the_game_screens_and_turns_sideways_files() {
    use dca_core::reader::ScreenKind;
    let bot = bot_dir().join("bot");
    let models = bot.join("models");
    if !models_ready(&models) {
        eprintln!("skipping: PaddleOCR models not downloaded");
        return;
    }
    let reader = Reader::new(&models, &bot.join("fonts")).unwrap();
    let cancel = std::sync::atomic::AtomicBool::new(false);
    let load = |p: &str| std::fs::read(bot.join(p)).unwrap();
    let png = |img: image::RgbImage| {
        let mut out = std::io::Cursor::new(Vec::new());
        img.write_to(&mut out, image::ImageFormat::Png).unwrap();
        out.into_inner()
    };
    let licence = load("fixtures/guides/driver-license.jpg");
    let event = load("fixtures/guides/team-event-score.jpg");
    let sample = load("fixtures/samples/IMG_20260603_201451_085.jpg");
    let podium = load("fixtures/samples/Screenshot_20260603-134456_Hill_Climb_Racing_2.jpg");
    assert_eq!(reader.classify_quick(&licence, &cancel).unwrap(), ScreenKind::DriverLicence);
    assert_eq!(reader.classify_quick(&load("fixtures/samples/Screenshot_20260926_102543.jpg"), &cancel).unwrap(), ScreenKind::DriverLicence);
    assert_eq!(reader.classify_quick(&event, &cancel).unwrap(), ScreenKind::Standings);
    assert_eq!(reader.classify_quick(&sample, &cancel).unwrap(), ScreenKind::Standings, "a cropped page");
    assert_eq!(reader.classify_quick(&podium, &cancel).unwrap(), ScreenKind::Podium);
    // Lying on its side, either way round.
    let decoded = image::load_from_memory(&event).unwrap().to_rgb8();
    assert_eq!(reader.classify_quick(&png(image::imageops::rotate90(&decoded)), &cancel).unwrap(), ScreenKind::Standings);
    assert_eq!(reader.classify_quick(&png(image::imageops::rotate270(&decoded)), &cancel).unwrap(), ScreenKind::Standings);
    // Not the game.
    let mut flat = image::RgbImage::new(1000, 600);
    for (x, y, p) in flat.enumerate_pixels_mut() {
        *p = image::Rgb([(x / 4) as u8, (y / 3) as u8, 90]);
    }
    assert_eq!(reader.classify_quick(&png(flat), &cancel).unwrap(), ScreenKind::Unknown);
}
