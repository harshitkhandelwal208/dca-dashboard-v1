//! Whole bot flows against the fake Discord (`mock_discord`) with the real OCR: applying for recruitment, the instant
//! screenshot check, closing tickets, spreadsheet posts. No Discord connection or keys needed.

use crate::app::{App, Dirs};
use crate::managers::recruitment;
use crate::mock_discord::{MockDiscord, BOT_ID, GUILD_ID};
use crate::responder::{Ix, Responder};
use dca_core::reader::Reader;
use dca_state::config::update_config;
use dca_state::stores::{list_tickets, TicketFilter};
use dca_state::StateStore;
use serde_json::{json, Value};
use serenity::all::*;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

pub const PANEL: u64 = 810_000_000_000_000_001;
pub const APPLICANT: u64 = 100_000_000_000_000_042;
pub const DM_USER: u64 = 100_000_000_000_000_099;
pub const RECRUITER_ROLE: u64 = 820_000_000_000_000_001;

pub fn bot_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../bot").canonicalize().unwrap()
}

pub fn models_ready() -> bool {
    dca_core::ocr::core_files().iter().all(|f| bot_dir().join("models").join(f).exists())
}

pub struct Rig {
    pub mock: MockDiscord,
    pub app: Arc<App>,
    pub dir: PathBuf,
}

impl Rig {
    pub async fn new(name: &str, with_ocr: bool) -> Rig {
        let mock = MockDiscord::start().await;
        let dir = std::env::temp_dir().join(format!("dca-flow-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let bot = bot_dir();
        let dirs = Dirs { data: dir.clone(), fonts: bot.join("fonts"), models: bot.join("models"), assets: bot.join("assets"), dashboard: dir.join("dist") };
        let app = App::new(mock.http(), StateStore::local(&dir), dirs.clone());
        *app.bot_id.write().unwrap() = Some(UserId::new(BOT_ID));
        if with_ocr {
            *app.reader.write().unwrap() = Some(Arc::new(Reader::new(&dirs.models, &dirs.fonts).expect("models load")));
        }
        mock.register_channel(PANEL, 0, "apply");
        update_config(&app.store, |c| {
            c.recruitment.enabled = true;
            c.recruitment.panel_channel_id = PANEL.to_string();
            c.recruitment.screenshot_dm_user_id = DM_USER.to_string();
            c.recruitment.recruiter_role_id = RECRUITER_ROLE.to_string();
            c.recruitment.private_threads = true;
            c.bot.recruitment_guild_id = GUILD_ID.to_string();
        })
        .await
        .unwrap();
        Rig { mock, app, dir }
    }

    pub fn image(&self, path: &str) -> String {
        let bytes = std::fs::read(bot_dir().join(path)).unwrap();
        let name = std::path::Path::new(path).file_name().unwrap().to_string_lossy().to_string();
        self.mock.file_url(bytes, &name)
    }
}

impl Drop for Rig {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

pub fn user_json(id: u64, name: &str) -> Value {
    json!({ "id": id.to_string(), "username": name, "discriminator": "0", "global_name": name, "avatar": null })
}

pub fn member_json(id: u64, name: &str, roles: &[u64], permissions: &str) -> Value {
    json!({ "user": user_json(id, name), "roles": roles.iter().map(|r| r.to_string()).collect::<Vec<_>>(), "joined_at": "2026-01-01T00:00:00.000000+00:00", "deaf": false, "mute": false, "permissions": permissions, "flags": 0 })
}

pub fn message_json(id: u64, channel: u64, author: u64, content: &str, attachments: Vec<Value>) -> Value {
    json!({
        "id": id.to_string(), "channel_id": channel.to_string(), "guild_id": GUILD_ID.to_string(), "author": user_json(author, "applicant"), "content": content,
        "timestamp": "2026-10-04T12:00:00.000000+00:00", "edited_timestamp": null, "tts": false, "mention_everyone": false, "mentions": [], "mention_roles": [],
        "attachments": attachments, "embeds": [], "pinned": false, "type": 0, "components": []
    })
}

pub fn attachment_json(id: u64, name: &str, url: &str) -> Value {
    json!({ "id": id.to_string(), "filename": name, "size": 100000, "url": url, "proxy_url": url, "content_type": if name.ends_with(".png") { "image/png" } else { "image/jpeg" }, "width": 1600, "height": 720 })
}

/// A button click by `user` on a message in `channel`.
pub fn button_click(mock: &MockDiscord, custom_id: &str, user: u64, channel: u64, roles: &[u64], permissions: &str) -> ComponentInteraction {
    let value = json!({
        "id": mock.next_id().to_string(), "application_id": BOT_ID.to_string(), "type": 3, "token": format!("tok-{}", mock.next_id()), "version": 1,
        "data": { "custom_id": custom_id, "component_type": 2 },
        "guild_id": GUILD_ID.to_string(), "channel_id": channel.to_string(),
        "channel": { "id": channel.to_string(), "type": 0 },
        "member": member_json(user, "applicant", roles, permissions), "user": user_json(user, "applicant"),
        "message": message_json(mock.next_id(), channel, BOT_ID, "panel", vec![]),
        "app_permissions": "0", "locale": "en-US", "entitlements": [], "attachment_size_limit": 26214400
    });
    serde_json::from_value(value).expect("interaction json")
}

pub fn parse_message(value: Value) -> Message {
    serde_json::from_value(value).expect("message json")
}

/// Wait for the applicant's collector to be registered, then drop an upload into it.
pub async fn upload(rig: &Rig, files: &[(&str, String)]) {
    let key = (ChannelId::new(PANEL), UserId::new(APPLICANT));
    let mut waited = 0;
    loop {
        if rig.app.collectors.lock().unwrap().contains_key(&key) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
        waited += 1;
        assert!(waited < 400, "the bot never started waiting for the upload");
    }
    let attachments = files.iter().enumerate().map(|(i, (name, url))| attachment_json(rig.mock.next_id() + i as u64, name, url)).collect();
    let message = parse_message(message_json(rig.mock.next_id(), PANEL, APPLICANT, "", attachments));
    let sender = rig.app.collectors.lock().unwrap().get(&key).cloned().unwrap();
    sender.send(message).unwrap();
}

pub async fn wait_for(what: &str, mut condition: impl FnMut() -> bool) {
    for _ in 0..1200 {
        if condition() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("timed out waiting for {what}");
}

pub fn responder(rig: &Rig, click: ComponentInteraction) -> Responder {
    Responder::new(rig.app.http.clone(), Ix::Comp(click))
}

pub fn body_text(call: &crate::mock_discord::Call) -> String {
    call.body.to_string()
}

// ------------------------------------------------------------------------------------------------ tests

async fn start_apply(rig: &Rig) -> tokio::task::JoinHandle<()> {
    let click = button_click(&rig.mock, "recruitment:apply", APPLICANT, PANEL, &[], "0");
    let r = responder(rig, click);
    tokio::spawn(recruitment::collect_license(rig.app.clone(), r))
}

fn edited_replies(rig: &Rig) -> Vec<String> {
    rig.mock.calls_matching("PATCH", "/messages/@original").iter().map(body_text).collect()
}

/// Every reply of any kind (first response, edit, follow-up).
fn all_replies(rig: &Rig) -> Vec<String> {
    rig.mock
        .calls()
        .into_iter()
        .filter(|c| (c.method == "POST" && (c.path.contains("/callback") || c.path.starts_with("/webhooks/"))) || (c.method == "PATCH" && c.path.contains("/@original")))
        .map(|c| if c.path.contains("/callback") { c.body["data"].to_string() } else { c.body.to_string() })
        .collect()
}

fn followups(rig: &Rig) -> Vec<String> {
    rig.mock.calls().into_iter().filter(|c| c.method == "POST" && c.path.starts_with("/webhooks/")).map(|c| body_text(&c)).collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn applying_with_a_real_licence_goes_through_without_waiting_for_the_full_reader() {
    if !models_ready() {
        eprintln!("skipping: PaddleOCR models not downloaded");
        return;
    }
    let rig = Rig::new("apply-licence", true).await;
    let task = start_apply(&rig).await;
    let licence = rig.image("fixtures/guides/driver-license.jpg");
    let started = std::time::Instant::now();
    upload(&rig, &[("driver-license.jpg", licence)]).await;

    // The prompt for event screenshots appears once the licence is accepted.
    wait_for("the event question", || edited_replies(&rig).iter().any(|b| b.contains("Do you have team event score screenshots"))).await;
    let accepted_in = started.elapsed();
    task.await.unwrap();

    // The upload was mirrored to the DM store and removed from the channel.
    let dm_posts = rig.mock.calls().into_iter().filter(|c| c.method == "POST" && c.path.ends_with("/messages") && !c.files.is_empty()).count();
    assert_eq!(dm_posts, 1, "one mirrored upload");
    assert_eq!(rig.mock.calls_matching("DELETE", "/messages/").len(), 1, "the public upload was deleted");
    eprintln!("licence accepted in {accepted_in:?}");

    // "No" -> the ticket is created.
    let click = button_click(&rig.mock, &format!("recruitment:event:no:{}", token_of(&rig)), APPLICANT, PANEL, &[], "0");
    recruitment::handle_component(rig.app.clone(), responder(&rig, click.clone()), click.data.custom_id.clone()).await;
    wait_for("the ticket", || !rig.mock.calls_matching("POST", "/threads").is_empty()).await;
    wait_for("the confirmation", || followups(&rig).iter().any(|b| b.contains("Your application ticket has been created"))).await;

    let tickets = list_tickets(&rig.app.store, &TicketFilter::default()).await;
    assert_eq!(tickets.len(), 1);
    assert_eq!(tickets[0].applicant_id, APPLICANT.to_string());
    assert_eq!(tickets[0].license_attachments.len(), 1);
    assert_eq!(tickets[0].license_attachments[0].source, "dm-mirror");
    assert_eq!(tickets[0].status, "open");
    let thread_messages = rig.mock.all_messages().into_iter().filter(|(c, m)| c.as_str() != PANEL.to_string() && m["embeds"].to_string().contains("Recruitment Application")).count();
    assert_eq!(thread_messages, 1, "the application embed was posted in the thread");
    assert!(!followups(&rig).iter().any(|b| b.contains("Screenshot check")), "a verified screenshot carries no warning");
}

fn token_of(rig: &Rig) -> String {
    rig.app.apply_sessions.lock().unwrap().keys().next().cloned().expect("an application in progress")
}

#[tokio::test(flavor = "multi_thread")]
async fn a_licence_and_event_screenshots_in_one_message_create_the_ticket_automatically() {
    if !models_ready() {
        eprintln!("skipping: PaddleOCR models not downloaded");
        return;
    }
    let rig = Rig::new("apply-both", true).await;
    let task = start_apply(&rig).await;
    let licence = rig.image("fixtures/guides/driver-license.jpg");
    let event = rig.image("fixtures/guides/team-event-score.jpg");
    let started = std::time::Instant::now();
    upload(&rig, &[("driver-license.jpg", licence), ("team-event-score.jpg", event)]).await;
    task.await.unwrap();
    eprintln!("licence + event upload -> ticket created in {:?}", started.elapsed());

    assert!(edited_replies(&rig).iter().any(|b| b.contains("Creating your application ticket now")), "{:?}", edited_replies(&rig));
    assert!(!edited_replies(&rig).iter().any(|b| b.contains("Do you have team event score screenshots")), "nothing left to ask");
    assert!(followups(&rig).iter().any(|b| b.contains("Your application ticket has been created")));
    let tickets = list_tickets(&rig.app.store, &TicketFilter::default()).await;
    assert_eq!(tickets.len(), 1);
    assert_eq!(tickets[0].license_attachments.len(), 1);
    assert_eq!(tickets[0].event_attachments.len(), 1);
    assert!(rig.app.apply_sessions.lock().unwrap().is_empty(), "the session is closed");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_screenshot_that_is_not_the_game_gets_the_guide_and_another_try() {
    if !models_ready() {
        eprintln!("skipping: PaddleOCR models not downloaded");
        return;
    }
    let rig = Rig::new("apply-wrong", true).await;
    // A picture that has nothing to do with the game.
    let mut img = image::RgbImage::new(1280, 720);
    for (x, y, p) in img.enumerate_pixels_mut() {
        *p = image::Rgb([(x / 6) as u8, (y / 4) as u8, ((x + y) / 9) as u8]);
    }
    let mut png = std::io::Cursor::new(Vec::new());
    img.write_to(&mut png, image::ImageFormat::Png).unwrap();
    let junk = rig.mock.file_url(png.into_inner(), "gradient.png");

    let task = start_apply(&rig).await;
    upload(&rig, &[("gradient.png", junk)]).await;
    wait_for("the guide", || followups(&rig).iter().any(|b| b.contains("Correct driver")) ).await;
    assert!(followups(&rig).iter().any(|b| b.contains("Attempt 1/")), "{:?}", followups(&rig));
    assert!(list_tickets(&rig.app.store, &TicketFilter::default()).await.is_empty(), "no ticket for a wrong image");

    // A real licence on the second try works.
    let licence = rig.image("fixtures/guides/driver-license.jpg");
    upload(&rig, &[("driver-license.jpg", licence)]).await;
    wait_for("the event question", || edited_replies(&rig).iter().any(|b| b.contains("Do you have team event score screenshots"))).await;
    task.await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_check_that_runs_out_of_time_never_blocks_the_applicant() {
    if !models_ready() {
        eprintln!("skipping: PaddleOCR models not downloaded");
        return;
    }
    let rig = Rig::new("apply-slow", true).await;
    rig.app.quick_secs.store(1, std::sync::atomic::Ordering::Relaxed);
    // The check lane is busy (a slow host): the instant check cannot start in time.
    let busy = rig.app.quick_gate.acquire().await.unwrap();
    let task = start_apply(&rig).await;
    let licence = rig.image("fixtures/guides/driver-license.jpg");
    let started = std::time::Instant::now();
    upload(&rig, &[("driver-license.jpg", licence)]).await;
    wait_for("the event question", || edited_replies(&rig).iter().any(|b| b.contains("Do you have team event score screenshots"))).await;
    task.await.unwrap();
    assert!(started.elapsed() < Duration::from_secs(8), "waited {:?}", started.elapsed());
    drop(busy);

    let click = button_click(&rig.mock, &format!("recruitment:event:no:{}", token_of(&rig)), APPLICANT, PANEL, &[], "0");
    recruitment::handle_component(rig.app.clone(), responder(&rig, click.clone()), click.data.custom_id.clone()).await;
    wait_for("the ticket", || !rig.mock.calls_matching("POST", "/threads").is_empty()).await;
    wait_for("the application embed", || rig.mock.all_messages().iter().any(|(_, m)| m["embeds"].to_string().contains("Recruitment Application"))).await;
    let flagged = rig.mock.all_messages().into_iter().any(|(_, m)| m["embeds"].to_string().contains("Screenshot check"));
    assert!(flagged, "recruiters are told the screenshot was accepted unchecked");
}

#[tokio::test(flavor = "multi_thread")]
async fn without_the_ocr_models_applicants_are_still_let_through() {
    let rig = Rig::new("apply-nomodels", false).await;
    let task = start_apply(&rig).await;
    let licence = rig.image("fixtures/guides/driver-license.jpg");
    upload(&rig, &[("driver-license.jpg", licence)]).await;
    wait_for("the event question", || edited_replies(&rig).iter().any(|b| b.contains("Do you have team event score screenshots"))).await;
    task.await.unwrap();
}

// ------------------------------------------------------------------------------------- spreadsheet samples

pub const SHEET_CHANNEL: u64 = 830_000_000_000_000_001;
pub const OUTPUT_CHANNEL: u64 = 830_000_000_000_000_002;
pub const SAMPLES: [&str; 5] = [
    "fixtures/samples/IMG_20260603_201451_085.jpg",
    "fixtures/samples/IMG_20260603_201453_701.jpg",
    "fixtures/samples/IMG_20260603_201456_348.jpg",
    "fixtures/samples/IMG_20260603_201458_573.jpg",
    "fixtures/samples/Screenshot_20260603-134456_Hill_Climb_Racing_2.jpg",
];

/// `(rank, name, score)` of the 96-player sample event, transcribed by hand from the screenshots.
pub fn expected_players() -> Vec<(u32, String, u64)> {
    let text = std::fs::read_to_string(bot_dir().join("fixtures/samples/team-event-expected.tsv")).unwrap();
    text.lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
        .map(|l| {
            let mut parts = l.split('\t');
            (parts.next().unwrap().parse().unwrap(), parts.next().unwrap().to_string(), parts.next().unwrap().parse().unwrap())
        })
        .collect()
}

fn fold_name(text: &str) -> Vec<char> {
    text.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(|c| c.to_lowercase())
        .map(|c| match c {
            'l' | 'i' | '1' | '|' => 'i',
            '0' => 'o',
            '5' | '$' => 's',
            '8' => 'b',
            '2' => 'z',
            other => other,
        })
        .collect()
}

fn edits(a: &[char], b: &[char]) -> usize {
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for i in 1..=a.len() {
        let mut cur = vec![i; b.len() + 1];
        for j in 1..=b.len() {
            cur[j] = (prev[j] + 1).min(cur[j - 1] + 1).min(prev[j - 1] + usize::from(a[i - 1] != b[j - 1]));
        }
        prev = cur;
    }
    prev[b.len()]
}

/// A reading counts as the right name when only look-alike letters or a couple of characters differ.
pub fn same_name(read: &str, truth: &str) -> bool {
    let (a, b) = (fold_name(read), fold_name(truth));
    edits(&a, &b) <= (b.len() / 5).min(2)
}

async fn sheet_rig(name: &str) -> Rig {
    let rig = Rig::new(name, true).await;
    rig.mock.register_channel(SHEET_CHANNEL, 0, "team-events");
    rig.mock.register_channel(OUTPUT_CHANNEL, 0, "event-output");
    update_config(&rig.app.store, |c| {
        c.spreadsheets.enabled = true;
        c.spreadsheets.teams = vec![dca_state::config::SpreadsheetTeam {
            id: "discord".into(),
            name: "Discord".into(),
            enabled: true,
            monitored_channel_id: SHEET_CHANNEL.to_string(),
            output_channel_id: OUTPUT_CHANNEL.to_string(),
            own_team_aliases: vec!["Discord".into()],
            auto_process: true,
            ..Default::default()
        }];
    })
    .await
    .unwrap();
    rig
}

async fn submit_pages(rig: &Rig, order: &[usize]) {
    for (n, index) in order.iter().enumerate() {
        let path = SAMPLES[*index];
        let name = std::path::Path::new(path).file_name().unwrap().to_string_lossy().to_string();
        let url = rig.image(path);
        let mut message = message_json(rig.mock.next_id(), SHEET_CHANNEL, APPLICANT + 7, "", vec![attachment_json(rig.mock.next_id(), &name, &url)]);
        message["author"]["bot"] = json!(false);
        let handled = crate::managers::spreadsheet::handle_message(&rig.app, &parse_message(message)).await;
        assert!(handled, "page {n} was picked up");
    }
}

fn check_sample_event(session: &dca_state::models::SpreadsheetSession) {
    let expected = expected_players();
    assert_eq!(session.status, "processed", "{}", session.error);
    assert_eq!(session.players.len(), 96, "every one of the 96 players, once");
    let mut right_names = 0;
    let mut wrong = Vec::new();
    for (rank, name, score) in &expected {
        let player = session.players.iter().find(|p| p.rank == *rank).unwrap_or_else(|| panic!("rank {rank} missing"));
        assert_eq!(player.score, Some(*score as i64), "score of rank {rank} ({name})");
        if same_name(&player.player_name, name) {
            right_names += 1;
        } else {
            wrong.push(format!("#{rank} {name:?} read as {:?}", player.player_name));
        }
    }
    eprintln!("names: {right_names}/96 right; misses: {wrong:?}");
    assert!(right_names >= 88, "only {right_names}/96 names right: {wrong:?}");
    assert_eq!(session.metadata.event_name, "Double-time Dilemma");
    let scores = session.metadata.team_scores.as_ref().expect("team scores from the result screen");
    assert_eq!((scores.own, scores.opponent), (Some(3312.0), Some(1210.0)));
    assert!(session.metadata.opponent_team_name.to_lowercase().contains("liberty"), "{:?}", session.metadata.opponent_team_name);
    // The first three are on the yellow (own) side, ranks 5, 6 and 12 on the blue side.
    for rank in [1, 2, 3] {
        assert_eq!(session.players.iter().find(|p| p.rank == rank).unwrap().team_type, "own", "rank {rank}");
    }
    for rank in [5, 6, 12] {
        assert_eq!(session.players.iter().find(|p| p.rank == rank).unwrap().team_type, "opponent", "rank {rank}");
    }
    assert_eq!(session.players.iter().find(|p| p.rank == 1).unwrap().points, Some(300));
    assert!(session.stats.own_players >= 40 && session.stats.opponents >= 25, "{} own / {} opponents", session.stats.own_players, session.stats.opponents);
}

#[tokio::test(flavor = "multi_thread")]
async fn five_screenshots_of_one_event_become_one_correct_spreadsheet_session() {
    if !models_ready() {
        eprintln!("skipping: PaddleOCR models not downloaded");
        return;
    }
    let rig = sheet_rig("sheet-sample").await;
    let order = [0, 1, 2, 3, 4];
    submit_pages(&rig, &order).await;
    let sessions = dca_state::stores::list_sessions(&rig.app.store, &Default::default()).await;
    assert_eq!(sessions.len(), 1, "all pages from one person join one session");
    assert_eq!(sessions[0].images.len(), 5);
    let started = std::time::Instant::now();
    crate::managers::spreadsheet::process_and_post(&rig.app, &sessions[0].id, &SHEET_CHANNEL.to_string()).await;
    eprintln!("processed in {:?}", started.elapsed());

    let session = dca_state::stores::get_session(&rig.app.store, &sessions[0].id).await.unwrap();
    check_sample_event(&session);

    // The output channel got the XLSX, the spreadsheet image and the chart.
    let posts = rig.mock.messages_in(OUTPUT_CHANNEL);
    assert!(!posts.is_empty(), "something was posted to the output channel");
    let files: Vec<String> = posts.iter().flat_map(|m| m["attachments"].as_array().cloned().unwrap_or_default()).filter_map(|a| a["filename"].as_str().map(str::to_string)).collect();
    assert!(files.iter().any(|f| f.ends_with(".xlsx")), "{files:?}");
    assert_eq!(files.iter().filter(|f| f.ends_with(".png")).count(), 2, "{files:?}");
    assert!(posts[0]["content"].as_str().unwrap().contains("Double-time Dilemma"), "{}", posts[0]["content"]);
}

#[tokio::test(flavor = "multi_thread")]
async fn pages_in_any_order_give_the_same_event() {
    if !models_ready() {
        eprintln!("skipping: PaddleOCR models not downloaded");
        return;
    }
    let rig = sheet_rig("sheet-shuffled").await;
    submit_pages(&rig, &[4, 3, 1, 0, 2]).await;
    let sessions = dca_state::stores::list_sessions(&rig.app.store, &Default::default()).await;
    let processed = crate::managers::spreadsheet::process_spreadsheet_session(&rig.app, &sessions[0].id, crate::managers::spreadsheet::ProcessOptions { rerun_ocr: false }).await.expect("processing");
    check_sample_event(&processed);
}

pub async fn sheet_rig_for_bench() -> Rig {
    sheet_rig("sheet-bench").await
}

pub async fn submit_bench_pages(rig: &Rig) {
    submit_pages(rig, &[0, 1, 2, 3, 4]).await;
}

pub async fn sheet_rig_pub(name: &str) -> Rig {
    sheet_rig(name).await
}

// ---------------------------------------------------------------------------------------------- closing

pub const LOG_CHANNEL: u64 = 810_000_000_000_000_002;
pub const RECRUITER: u64 = 100_000_000_000_000_055;

#[tokio::test(flavor = "multi_thread")]
async fn a_recruiter_closes_the_ticket_and_one_background_reading_adds_the_stats() {
    if !models_ready() {
        eprintln!("skipping: PaddleOCR models not downloaded");
        return;
    }
    let rig = Rig::new("close", true).await;
    rig.mock.register_channel(LOG_CHANNEL, 0, "recruitment-log");
    update_config(&rig.app.store, |c| {
        c.recruitment.log_channel_id = LOG_CHANNEL.to_string();
        c.recruitment.teams = vec!["Discord 3\u{2122}".into(), "Nascar DC".into()];
    })
    .await
    .unwrap();

    // The applicant applies with both screenshots.
    let task = start_apply(&rig).await;
    upload(&rig, &[("driver-license.jpg", rig.image("fixtures/guides/driver-license.jpg")), ("team-event-score.jpg", rig.image("fixtures/guides/team-event-score.jpg"))]).await;
    task.await.unwrap();
    let tickets = list_tickets(&rig.app.store, &TicketFilter::default()).await;
    assert_eq!(tickets.len(), 1);
    let thread: u64 = tickets[0].thread_id.parse().unwrap();

    // A recruiter closes it with the first team as the outcome.
    let closing = std::time::Instant::now();
    let click = button_click(&rig.mock, "recruitment:close-team:team:0", RECRUITER, thread, &[RECRUITER_ROLE], "0");
    recruitment::handle_component(rig.app.clone(), responder(&rig, click.clone()), click.data.custom_id.clone()).await;
    wait_for("the ticket to close", || rig.mock.calls_matching("POST", &format!("/channels/{LOG_CHANNEL}/messages")).len() >= 1).await;
    tokio::time::sleep(Duration::from_millis(500)).await;

    eprintln!("close -> log posted in {:?}", closing.elapsed());
    let closed = dca_state::stores::get_ticket(&rig.app.store, &thread.to_string()).await.unwrap();
    assert_eq!(closed.status, "closed");
    assert_eq!(closed.outcome, "accepted");
    assert_eq!(closed.team, "Discord 3\u{2122}");
    assert_eq!(closed.closed_by_id, RECRUITER.to_string());
    // The log is posted at once; the one background reading completes it.
    let first = rig.mock.messages_in(LOG_CHANNEL);
    assert!(first[0]["embeds"].to_string().contains("Reading the screenshots") || first[0]["embeds"].to_string().contains("BlackWing"));
    let id = thread.to_string();
    let mut analysed = None;
    for _ in 0..1200 {
        if let Some(a) = dca_state::stores::get_ticket(&rig.app.store, &id).await.and_then(|t| t.license_analysis) {
            analysed = Some(a);
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let analysis = analysed.expect("the background reading finished");
    assert!(analysis.in_game_name.contains("BlackWing"), "{analysis:?}");
    assert_eq!(analysis.garage_power, "6195");
    assert_eq!(analysis.event_scores.len(), 1);
    assert_eq!(analysis.event_scores[0].rank, "60");
    tokio::time::sleep(Duration::from_millis(300)).await;

    // The recruitment log embed, the lock and the archive.
    let log = rig.mock.messages_in(LOG_CHANNEL);
    assert_eq!(log.len(), 1);
    let embed = log[0]["embeds"].to_string();
    for needle in ["Discord 3", "1 licence, 1 team event", &APPLICANT.to_string(), "BlackWing", "6195", "rank 60"] {
        assert!(embed.contains(needle), "{needle} missing from {embed}");
    }
    let patches = rig.mock.calls_matching("PATCH", &format!("/channels/{thread}"));
    assert!(patches.iter().any(|c| c.body["locked"] == json!(true)), "locked: {patches:?}");
    assert!(patches.iter().any(|c| c.body["archived"] == json!(true)), "archived");
    assert_eq!(dca_state::stores::list_recruitment_logs(&rig.app.store, 10).await.len(), 1);

    // Closing again is refused.
    let again = button_click(&rig.mock, "recruitment:close-team:rejected", RECRUITER, thread, &[RECRUITER_ROLE], "0");
    recruitment::handle_component(rig.app.clone(), responder(&rig, again.clone()), again.data.custom_id.clone()).await;
    assert!(all_replies(&rig).iter().any(|b| b.contains("already closed")), "{:?}", all_replies(&rig));

    // Somebody who is not a recruiter cannot close tickets.
    let intruder = button_click(&rig.mock, "recruitment:close", APPLICANT, thread, &[], "0");
    recruitment::handle_component(rig.app.clone(), responder(&rig, intruder.clone()), intruder.data.custom_id.clone()).await;
    assert!(all_replies(&rig).iter().any(|b| b.to_lowercase().contains("recruiter")), "{:?}", all_replies(&rig));
}
