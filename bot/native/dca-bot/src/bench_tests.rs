//! Resource benchmark (ignored by default): `cargo test --release -p dca-bot bench -- --ignored --nocapture`.
//! Run it inside a 0.1 CPU / 512 MB container (see docs/PERFORMANCE.md) to see how the bot behaves on Render's free tier.

use crate::flow_tests::*;
use crate::managers::recruitment::vision::classify;
use crate::managers::spreadsheet::{process_spreadsheet_session, ProcessOptions};
use dca_core::reader::Reader;
use dca_state::models::Attachment;
use std::time::Instant;

fn status_kb(key: &str) -> u64 {
    std::fs::read_to_string("/proc/self/status").unwrap_or_default().lines().find(|l| l.starts_with(key)).and_then(|l| l.split_whitespace().nth(1)).and_then(|v| v.parse().ok()).unwrap_or(0)
}

fn mb(kb: u64) -> String {
    format!("{:.0} MB", kb as f64 / 1024.0)
}

fn cpu_seconds() -> f64 {
    let stat = std::fs::read_to_string("/proc/self/stat").unwrap_or_default();
    let fields: Vec<&str> = stat.rsplit(')').next().unwrap_or("").split_whitespace().collect();
    let ticks = |i: usize| fields.get(i).and_then(|v| v.parse::<f64>().ok()).unwrap_or(0.0);
    (ticks(11) + ticks(12)) / 100.0
}

struct Stage {
    name: String,
    wall: f64,
    cpu: f64,
    rss: u64,
    peak: u64,
}

fn timed<T>(stages: &mut Vec<Stage>, name: &str, f: impl FnOnce() -> T) -> T {
    let (w, c) = (Instant::now(), cpu_seconds());
    let out = f();
    stages.push(Stage { name: name.to_string(), wall: w.elapsed().as_secs_f64(), cpu: cpu_seconds() - c, rss: status_kb("VmRSS:"), peak: status_kb("VmHWM:") });
    out
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn bench() {
    assert!(models_ready(), "download the OCR models first (bot/scripts/download-models.sh)");
    let mut stages: Vec<Stage> = Vec::new();
    let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(0);
    let bot = bot_dir();

    let rss_start = status_kb("VmRSS:");
    let reader = timed(&mut stages, "load OCR models (detector + Latin + multilingual)", || Reader::new(&bot.join("models"), &bot.join("fonts")).unwrap());
    let rss_loaded = status_kb("VmRSS:");
    let reader = std::sync::Arc::new(reader);

    let cancel = std::sync::atomic::AtomicBool::new(false);
    let licence = std::fs::read(bot.join("fixtures/guides/driver-license.jpg")).unwrap();
    let event = std::fs::read(bot.join("fixtures/guides/team-event-score.jpg")).unwrap();
    let page = std::fs::read(bot.join(SAMPLES[0])).unwrap();
    let podium = std::fs::read(bot.join(SAMPLES[4])).unwrap();
    let licence2 = std::fs::read(bot.join("fixtures/samples/Screenshot_20260926_102543.jpg")).unwrap();

    for (name, bytes) in
        [("licence (guide)", &licence), ("licence (sample 2)", &licence2), ("team-event screen (full)", &event), ("standings page (cropped, 28 rows)", &page), ("result/podium screen", &podium)]
    {
        let kind = timed(&mut stages, &format!("instant check: {name}"), || reader.classify_quick(bytes, &cancel).unwrap());
        assert_ne!(format!("{kind:?}"), "Unknown", "{name}");
    }
    for (name, bytes) in [("standings page (cropped, 28 rows)", &page), ("team-event screen (full)", &event), ("result/podium screen", &podium)] {
        timed(&mut stages, &format!("event screenshot read (one pass): {name}"), || reader.read_event_screenshot(bytes, &[]).unwrap());
    }
    let rss_read = status_kb("VmRSS:");

    // Whole flows through the fake Discord: the applicant's upload and the 5-screenshot spreadsheet.
    let rig = Rig::new("bench", false).await;
    *rig.app.reader.write().unwrap() = Some(reader.clone());
    let mut inputs = vec![];
    for (name, bytes) in [("driver-license.jpg", &licence), ("team-event-score.jpg", &event)] {
        let url = rig.mock.file_url(bytes.clone(), name);
        inputs.push((Attachment { name: name.into(), url, ..Default::default() }, if name.starts_with("driver") { "licenseAttachments" } else { "eventAttachments" }));
    }
    let config = rig.app.config().await;
    let started = (Instant::now(), cpu_seconds());
    let checks = classify(&rig.app, &inputs, &config).await.unwrap();
    stages.push(Stage {
        name: "applicant upload check (licence + event screenshot)".into(),
        wall: started.0.elapsed().as_secs_f64(),
        cpu: cpu_seconds() - started.1,
        rss: status_kb("VmRSS:"),
        peak: status_kb("VmHWM:"),
    });
    assert!(checks.iter().all(|c| c.kind != crate::managers::recruitment::vision::ImageKind::Unknown));
    drop(rig);

    let rig = sheet_rig_for_bench().await;
    *rig.app.reader.write().unwrap() = Some(reader.clone());
    submit_bench_pages(&rig).await;
    let sessions = dca_state::stores::list_sessions(&rig.app.store, &Default::default()).await;
    let started = (Instant::now(), cpu_seconds());
    let processed = process_spreadsheet_session(&rig.app, &sessions[0].id, ProcessOptions { rerun_ocr: false }).await.unwrap();
    stages.push(Stage {
        name: "spreadsheet: 5 screenshots -> 96 players, xlsx + 2 images".into(),
        wall: started.0.elapsed().as_secs_f64(),
        cpu: cpu_seconds() - started.1,
        rss: status_kb("VmRSS:"),
        peak: status_kb("VmHWM:"),
    });
    assert_eq!(processed.players.len(), 96);
    let started = (Instant::now(), cpu_seconds());
    crate::managers::spreadsheet::rebuild_spreadsheet_session(&rig.app, &sessions[0].id).await.unwrap();
    stages.push(Stage {
        name: "spreadsheet: rebuild outputs from stored readings".into(),
        wall: started.0.elapsed().as_secs_f64(),
        cpu: cpu_seconds() - started.1,
        rss: status_kb("VmRSS:"),
        peak: status_kb("VmHWM:"),
    });

    println!("\n=== benchmark ({} usable CPU thread(s)) ===", threads);
    println!("{:<62} {:>9} {:>9} {:>9} {:>9}", "stage", "wall (s)", "cpu (s)", "RSS", "peak");
    for s in &stages {
        println!("{:<62} {:>9.2} {:>9.2} {:>9} {:>9}", s.name, s.wall, s.cpu, mb(s.rss), mb(s.peak));
    }
    println!("\nmemory: start {}, models loaded {}, after readings {}, peak {} (VmHWM)", mb(rss_start), mb(rss_loaded), mb(rss_read), mb(status_kb("VmHWM:")));
}
