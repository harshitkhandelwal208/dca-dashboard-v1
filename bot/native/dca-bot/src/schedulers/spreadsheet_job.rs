//! Background spreadsheet processing job and scheduler.

use dca_core::excel::{generate_spreadsheet_xlsx, ExcelExportOptions};
use dca_core::review_image::generate_review_image;
use dca_core::standings_table::find_standings_table;
use dca_core::tesseract::OcrEngine;
use dca_core::team_event_vision::{read_standings_page, ParsedRow};
use dca_state::spreadsheet_store::SpreadsheetStore;
use serenity::all::{ChannelId, CreateAttachment, CreateEmbed, CreateMessage, Http, Timestamp};
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;

/// Process a single spreadsheet session from start to finish.
pub async fn process_spreadsheet_session(http: Arc<Http>, ocr: Arc<OcrEngine>, session_id: &str) {
    let Some(session) = SpreadsheetStore::get_session(session_id) else {
        return;
    };

    if session.status == "completed" || session.status == "cancelled" {
        return;
    }

    // Mark session as processing
    let _ = SpreadsheetStore::update_session(session_id, |s| {
        s.status = "processing".to_string();
    });

    let channel_id_u64: u64 = session.channel_id.parse().unwrap_or(0);
    let channel_id = ChannelId::new(channel_id_u64);

    if !session.images.is_empty() {
        let _ = channel_id.say(
            &http,
            format!("Processing standings for `{}` ({} screenshots)...", session.team_name, session.images.len()),
        ).await;
    }

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(45))
        .build()
        .unwrap_or_default();

    let mut all_pages = Vec::new();

    for img_val in &session.images {
        let url = img_val.get("url").and_then(|u| u.as_str()).unwrap_or("");
        if url.is_empty() {
            continue;
        }

        let Ok(resp) = client.get(url).send().await else {
            continue;
        };
        let Ok(body) = resp.bytes().await else {
            continue;
        };
        let Ok(img) = image::load_from_memory(&body) else {
            continue;
        };

        let width = img.width();
        let height = img.height();
        let rgba = img.to_rgba8();
        let raw_rgba = rgba.into_raw();

        if let Some(table) = find_standings_table(&raw_rgba, width, height, 4) {
            if let Ok(page) = read_standings_page(&ocr, &raw_rgba, width, height, 4, &table) {
                all_pages.push(page);
            }
        }
    }

    // Multi-page deduplication & ordering across standings pages
    let mut rows_by_rank: BTreeMap<u32, ParsedRow> = BTreeMap::new();
    for page in all_pages {
        for row in page.rows {
            match rows_by_rank.entry(row.rank) {
                std::collections::btree_map::Entry::Vacant(e) => {
                    e.insert(row);
                }
                std::collections::btree_map::Entry::Occupied(mut e) => {
                    // Pick the row with higher OCR confidence
                    if row.name_confidence > e.get().name_confidence {
                        e.insert(row);
                    }
                }
            }
        }
    }

    let merged_rows: Vec<ParsedRow> = rows_by_rank.into_values().collect();

    if merged_rows.is_empty() {
        let err_msg = "Could not detect a valid standings table in the provided screenshot(s).".to_string();
        let _ = SpreadsheetStore::update_session(session_id, |s| {
            s.status = "failed".to_string();
            s.error_message = Some(err_msg.clone());
        });

        let embed = CreateEmbed::new()
            .title(format!("Standings Processing Failed — {}", session.team_name))
            .description(
                "Our automated vision reader could not find the race standings table in the uploaded screenshot(s).\n\n\
                **Tips:**\n\
                • Ensure the full standings table (ranks, names, scores) is visible.\n\
                • Upload in-game screenshots without heavy cropping or low resolution.",
            )
            .color(0xef4444) // Red 500
            .timestamp(Timestamp::now());

        let _ = channel_id.send_message(&http, CreateMessage::new().embed(embed)).await;
        return;
    }

    let own_count = merged_rows.iter().filter(|r| r.is_own_team).count();
    let opp_count = merged_rows.iter().filter(|r| !r.is_own_team).count();
    let own_score: u32 = merged_rows.iter().filter(|r| r.is_own_team).map(|r| r.score).sum();
    let opp_score: u32 = merged_rows.iter().filter(|r| !r.is_own_team).map(|r| r.score).sum();
    let flagged_count = merged_rows.iter().filter(|r| r.flagged).count();

    // Generate .xlsx
    let xlsx_opts = ExcelExportOptions {
        team_name: &session.team_name,
        title: &format!("Team Event Standings - {}", session.team_name),
        rows: &merged_rows,
    };

    let xlsx_bytes = match generate_spreadsheet_xlsx(&xlsx_opts) {
        Ok(b) => b,
        Err(e) => {
            let _ = SpreadsheetStore::update_session(session_id, |s| {
                s.status = "failed".to_string();
                s.error_message = Some(format!("Excel generation error: {e}"));
            });
            let _ = channel_id.say(&http, format!("Failed to generate Excel workbook: {e}")).await;
            return;
        }
    };

    // Save xlsx to disk
    let _ = tokio::fs::create_dir_all("data/spreadsheets").await;
    let xlsx_path = format!("data/spreadsheets/{}.xlsx", session.id);
    let _ = tokio::fs::write(&xlsx_path, &xlsx_bytes).await;

    // Generate review image if any rows are flagged
    let review_bytes = generate_review_image(&merged_rows, &session.team_name).ok().flatten();
    let review_path = if let Some(ref rb) = review_bytes {
        let r_path = format!("data/spreadsheets/{}-review.png", session.id);
        let _ = tokio::fs::write(&r_path, rb).await;
        Some(r_path)
    } else {
        None
    };

    // Attachments for Discord
    let mut files = Vec::new();
    let safe_team_name = session.team_name.replace(' ', "_");
    files.push(CreateAttachment::bytes(
        xlsx_bytes,
        format!("{}_Standings.xlsx", safe_team_name),
    ));

    if let Some(ref rb) = review_bytes {
        files.push(CreateAttachment::bytes(
            rb.clone(),
            format!("{}_Review.png", safe_team_name),
        ));
    }

    let review_status_text = if flagged_count > 0 {
        format!("**{}** driver name(s) flagged for review. See attached `{}_Review.png`.", flagged_count, safe_team_name)
    } else {
        "All driver names verified with high confidence.".to_string()
    };

    let embed = CreateEmbed::new()
        .title(format!("Event Standings Generated — {}", session.team_name))
        .description(format!(
            "Standings successfully parsed from **{}** screenshot(s)!\n\n\
            • **Total Drivers:** {} ({} Own Team, {} Opponents)\n\
            • **Team Score:** **{}** points\n\
            • **Opponent Score:** **{}** points",
            session.images.len(),
            merged_rows.len(),
            own_count,
            opp_count,
            own_score,
            opp_score
        ))
        .field("Verification Status", review_status_text, false)
        .color(0x0f766e)
        .timestamp(Timestamp::now());

    let _ = channel_id.send_message(&http, CreateMessage::new().embed(embed).files(files)).await;

    // Update store to completed
    let _ = SpreadsheetStore::update_session(session_id, |s| {
        s.status = "completed".to_string();
        s.xlsx_path = Some(xlsx_path);
        s.review_image_path = review_path;
        s.readings = merged_rows.iter().map(|r| serde_json::json!({
            "rank": r.rank,
            "name": r.name,
            "score": r.score,
            "isOwnTeam": r.is_own_team,
            "confidence": r.name_confidence,
            "flagged": r.flagged,
            "flagReason": r.flag_reason,
        })).collect();
    });
}

/// Start background scheduler monitoring pending spreadsheet sessions whose timeout window expired.
pub async fn start_spreadsheet_scheduler(http: Arc<Http>, ocr: Arc<OcrEngine>) {
    tokio::spawn(async move {
        loop {
            sleep(Duration::from_secs(2)).await;
            let now = chrono::Utc::now().timestamp_millis() as u64;
            let sessions = SpreadsheetStore::list_sessions();

            for s in sessions {
                if s.status == "pending" && s.window_ends_at > 0 && now >= s.window_ends_at {
                    let sid = s.id.clone();
                    let http_clone = http.clone();
                    let ocr_clone = ocr.clone();
                    tokio::spawn(async move {
                        process_spreadsheet_session(http_clone, ocr_clone, &sid).await;
                    });
                }
            }
        }
    });
}
