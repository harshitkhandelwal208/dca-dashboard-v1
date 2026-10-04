//! Intake processing for recruitment and spreadsheets with instant verification and fail-open manual fallbacks.

use dca_core::recruitment_vision::{instant_verify_screenshot, scan_driver_license};
use dca_core::tesseract::OcrEngine;
use dca_state::config::{load_dashboard_config, DashboardConfig};
use dca_state::recruitment_store::{RecruitmentStore, Ticket};
use dca_state::spreadsheet_store::{SpreadsheetSession, SpreadsheetStore};
use poise::serenity_prelude as serenity;
use std::sync::Arc;

pub async fn handle_potential_spreadsheet_image(
    ctx: &serenity::Context,
    msg: &serenity::Message,
    config: &DashboardConfig,
) -> bool {
    let channel_id_str = msg.channel_id.to_string();
    let team_opt = config.spreadsheets.teams.iter().find(|t| t.channel_id == channel_id_str);

    let Some(team) = team_opt else {
        return false;
    };

    let image_attachments: Vec<&serenity::Attachment> = msg.attachments.iter()
        .filter(|a| a.content_type.as_deref().map(|ct| ct.starts_with("image/")).unwrap_or(false)
            || a.filename.ends_with(".png") || a.filename.ends_with(".jpg") || a.filename.ends_with(".jpeg") || a.filename.ends_with(".webp"))
        .collect();

    if image_attachments.is_empty() {
        return false;
    }

    // Instant reaction (< 100ms)
    let _ = msg.react(ctx, '📥').await;

    let now_str = chrono::Utc::now().to_rfc3339();
    let window_ms = config.spreadsheets.session_window_seconds * 1000;
    let window_ends_at = chrono::Utc::now().timestamp_millis() as u64 + window_ms;

    let sessions = SpreadsheetStore::list_sessions();
    let existing_pending = sessions.into_iter().find(|s| {
        s.channel_id == channel_id_str && s.author_id == msg.author.id.to_string() && s.status == "pending"
    });

    let mut session = existing_pending.unwrap_or_else(|| SpreadsheetSession {
        id: format!("race-{}", uuid::Uuid::new_v4().to_string().chars().take(8).collect::<String>()),
        team_id: team.id.clone(),
        team_name: team.name.clone(),
        guild_id: msg.guild_id.map(|g| g.to_string()).unwrap_or_default(),
        channel_id: channel_id_str.clone(),
        author_id: msg.author.id.to_string(),
        author_tag: msg.author.tag(),
        status: "pending".to_string(),
        created_at: now_str.clone(),
        last_image_at: Some(now_str.clone()),
        window_ends_at,
        message_ids: vec![msg.id.to_string()],
        images: Vec::new(),
        readings: Vec::new(),
        corrections: Vec::new(),
        xlsx_path: None,
        review_image_path: None,
        error_message: None,
    });

    for att in &image_attachments {
        session.images.push(serde_json::json!({
            "id": att.id.to_string(),
            "url": att.url,
            "filename": att.filename,
            "size": att.size
        }));
    }

    session.last_image_at = Some(now_str);
    session.window_ends_at = window_ends_at;

    let _ = SpreadsheetStore::save_session(session);
    true
}

pub async fn handle_recruitment_image_upload(
    ctx: &serenity::Context,
    msg: &serenity::Message,
    ocr_engine: Arc<OcrEngine>,
) -> bool {
    let config = load_dashboard_config();
    if !config.recruitment.enabled {
        return false;
    }

    // Check if in recruitment panel channel
    let channel_id_str = msg.channel_id.to_string();
    if channel_id_str != config.recruitment.panel_channel_id {
        return false;
    }

    let image_attachments: Vec<&serenity::Attachment> = msg.attachments.iter()
        .filter(|a| a.content_type.as_deref().map(|ct| ct.starts_with("image/")).unwrap_or(false)
            || a.filename.ends_with(".png") || a.filename.ends_with(".jpg") || a.filename.ends_with(".jpeg") || a.filename.ends_with(".webp"))
        .collect();

    if image_attachments.is_empty() {
        return false;
    }

    let first_attachment = image_attachments[0];

    // Download image bytes
    let client = reqwest::Client::new();
    let resp = match client.get(&first_attachment.url).send().await {
        Ok(r) => r,
        Err(_) => return false,
    };
    let body_bytes = match resp.bytes().await {
        Ok(b) => b,
        Err(_) => return false,
    };

    // Instant verification (< 5ms)
    let Ok(img) = image::load_from_memory(&body_bytes) else {
        return false;
    };

    let width = img.width();
    let height = img.height();
    let rgba = img.to_rgba8();
    let raw_rgba = rgba.into_raw();

    let instant = instant_verify_screenshot(&raw_rgba, width, height, 4);

    if instant.is_ingame {
        // INSTANT PASS! Acknowledge immediately, no waiting for slow OCR!
        let _ = msg.delete(ctx).await;

        let ticket_id = uuid::Uuid::new_v4().to_string().chars().take(8).collect::<String>();

        // Try creating dedicated thread for applicant
        let thread_opt = msg.channel_id.create_thread(
            ctx,
            serenity::CreateThread::new(format!("ticket-{}", msg.author.name))
                .kind(serenity::ChannelType::PublicThread),
        ).await.ok();

        let target_channel_id = thread_opt.as_ref().map(|t| t.id).unwrap_or(msg.channel_id);
        let thread_id_str = thread_opt.as_ref().map(|t| t.id.to_string());

        let ticket = Ticket {
            id: ticket_id.clone(),
            applicant_id: msg.author.id.to_string(),
            applicant_tag: msg.author.tag(),
            status: "open".to_string(),
            thread_id: thread_id_str,
            channel_id: Some(channel_id_str.clone()),
            claimed_by_id: None,
            claimed_by_tag: None,
            assigned_team: None,
            created_at: chrono::Utc::now().to_rfc3339(),
            closed_at: None,
            license_analysis: None,
            license_attachments: vec![serde_json::json!({
                "url": first_attachment.url,
                "filename": first_attachment.filename
            })],
            event_attachments: Vec::new(),
            screenshot_check: Some(serde_json::json!({
                "verified": true,
                "kind": format!("{:?}", instant.kind),
                "photo": instant.is_photo,
                "note": instant.note
            })),
            manual_entry: false,
        };

        let _ = RecruitmentStore::save_ticket(ticket);

        let claim_btn = serenity::CreateButton::new(format!("ticket:claim:{}", ticket_id))
            .label("Claim Ticket")
            .style(serenity::ButtonStyle::Primary);
        let close_btn = serenity::CreateButton::new(format!("ticket:close:{}", ticket_id))
            .label("Close Ticket")
            .style(serenity::ButtonStyle::Danger);
        let row = serenity::CreateActionRow::Buttons(vec![claim_btn, close_btn]);

        let recruiter_mention = if !config.recruitment.recruiter_role_id.is_empty() {
            format!("<@&{}>", config.recruitment.recruiter_role_id)
        } else {
            "Recruiters".to_string()
        };

        let embed = serenity::CreateEmbed::new()
            .title("Driver Profile Verified")
            .description(format!(
                "Welcome to DCA, <@{}>! Your in-game screenshot was verified instantly.\n\
                Ticket **#{}** is open. {} will review your profile shortly.\n\n\
                *Stats are extracting in the background.*",
                msg.author.id, ticket_id, recruiter_mention
            ))
            .color(0x0f766e)
            .timestamp(serenity::Timestamp::now());

        let _ = target_channel_id.send_message(
            ctx,
            serenity::CreateMessage::new().embed(embed).components(vec![row]),
        ).await;

        // Background asynchronous deep OCR scan
        let engine_clone = ocr_engine.clone();
        let bytes_vec = body_bytes.to_vec();
        let http_clone = ctx.http.clone();
        let applicant_id = msg.author.id;
        tokio::spawn(async move {
            let profile = scan_driver_license(&engine_clone, &bytes_vec, width, height);

            // Save OCR license profile analysis into the ticket
            let _ = RecruitmentStore::update_ticket(&ticket_id, |t| {
                t.license_analysis = Some(serde_json::to_value(&profile).unwrap_or_default());
            });

            // Calculate suggested team tier
            let config = load_dashboard_config();
            let gp = profile.garage_power.unwrap_or(0);
            let suggested_team = crate::handlers::recruitment_auto::determine_team_for_gp(gp, None, &config);

            let player_name = profile.player_name.as_deref().unwrap_or("Applicant");
            let cup_pts = profile.cup_points.map(|p| p.to_string()).unwrap_or_else(|| "N/A".to_string());
            let season_pts = profile.season_points.map(|p| p.to_string()).unwrap_or_else(|| "N/A".to_string());

            let claim_btn = serenity::CreateButton::new(format!("ticket:claim:{}", ticket_id))
                .label("Claim Ticket")
                .style(serenity::ButtonStyle::Primary);
            let close_btn = serenity::CreateButton::new(format!("ticket:close:{}", ticket_id))
                .label("Close Ticket")
                .style(serenity::ButtonStyle::Danger);
            let row = serenity::CreateActionRow::Buttons(vec![claim_btn, close_btn]);

            let embed = serenity::CreateEmbed::new()
                .title("Driver Profile Analysis")
                .description(format!(
                    "Driver stats have been scanned and verified for <@{}>:\n\n\
                    • **Driver Name:** `{}`\n\
                    • **Garage Power:** `{} GP`\n\
                    • **Cup Points:** `{}`\n\
                    • **Season Points:** `{}`\n\
                    • **Suggested Team Tier:** **{}**\n\n\
                    *Recruiter action:* Click **[Claim Ticket]** to claim this ticket, or **[Close Ticket]** to select the assigned team outcome.",
                    applicant_id, player_name, gp, cup_pts, season_pts, suggested_team
                ))
                .color(0x0f766e)
                .timestamp(serenity::Timestamp::now());

            let _ = target_channel_id.send_message(
                &http_clone,
                serenity::CreateMessage::new().embed(embed).components(vec![row]),
            ).await;
        });

        return true;
    }

    // Unverified screenshot handling: after first retry, automatically open ticket directly
    let _ = msg.delete(ctx).await;

    static ATTEMPTS: std::sync::LazyLock<std::sync::Mutex<std::collections::HashMap<serenity::UserId, u32>>> =
        std::sync::LazyLock::new(|| std::sync::Mutex::new(std::collections::HashMap::new()));

    let current_attempt = {
        let mut map = ATTEMPTS.lock().unwrap();
        let entry = map.entry(msg.author.id).or_insert(0);
        *entry += 1;
        *entry
    };

    if current_attempt == 1 {
        // First retry prompt: direct message with zero buttons
        let embed = serenity::CreateEmbed::new()
            .title("Unverified Screenshot")
            .description(format!(
                "Hi <@{}>, our automated check could not confirm an in-game driver profile in this screenshot.\n\
                Please upload your driver profile screenshot again (with your Player Name and Garage Power visible).",
                msg.author.id
            ))
            .color(0xf59e0b) // Amber 500
            .timestamp(serenity::Timestamp::now());

        let _ = msg.channel_id.send_message(ctx, serenity::CreateMessage::new().embed(embed)).await;
        return true;
    }

    // After the first retry: directly create a ticket automatically
    {
        let mut map = ATTEMPTS.lock().unwrap();
        map.remove(&msg.author.id);
    }

    let ticket_id = uuid::Uuid::new_v4().to_string().chars().take(8).collect::<String>();

    let thread_opt = msg.channel_id.create_thread(
        ctx,
        serenity::CreateThread::new(format!("ticket-{}", msg.author.name))
            .kind(serenity::ChannelType::PublicThread),
    ).await.ok();

    let target_channel_id = thread_opt.as_ref().map(|t| t.id).unwrap_or(msg.channel_id);
    let thread_id_str = thread_opt.as_ref().map(|t| t.id.to_string());

    let ticket = Ticket {
        id: ticket_id.clone(),
        applicant_id: msg.author.id.to_string(),
        applicant_tag: msg.author.tag(),
        status: "open".to_string(),
        thread_id: thread_id_str,
        channel_id: Some(channel_id_str.clone()),
        claimed_by_id: None,
        claimed_by_tag: None,
        assigned_team: None,
        created_at: chrono::Utc::now().to_rfc3339(),
        closed_at: None,
        license_analysis: None,
        license_attachments: vec![serde_json::json!({
            "url": first_attachment.url,
            "filename": first_attachment.filename
        })],
        event_attachments: Vec::new(),
        screenshot_check: Some(serde_json::json!({
            "verified": false,
            "retry_fallback": true
        })),
        manual_entry: false,
    };

    let _ = RecruitmentStore::save_ticket(ticket);

    let claim_btn = serenity::CreateButton::new(format!("ticket:claim:{}", ticket_id))
        .label("Claim Ticket")
        .style(serenity::ButtonStyle::Primary);
    let close_btn = serenity::CreateButton::new(format!("ticket:close:{}", ticket_id))
        .label("Close Ticket")
        .style(serenity::ButtonStyle::Danger);
    let row = serenity::CreateActionRow::Buttons(vec![claim_btn, close_btn]);

    let recruiter_mention = if !config.recruitment.recruiter_role_id.is_empty() {
        format!("<@&{}>", config.recruitment.recruiter_role_id)
    } else {
        "Recruiters".to_string()
    };

    let embed = serenity::CreateEmbed::new()
        .title("Recruitment Ticket Created")
        .description(format!(
            "Ticket **#{}** has been opened for <@{}>.\n\
            Staff assistance has been requested. {} will assist you directly.\n\n\
            *You do not need to do anything further — our team will review your application here.*",
            ticket_id, msg.author.id, recruiter_mention
        ))
        .color(0x0f766e)
        .timestamp(serenity::Timestamp::now());

    let _ = target_channel_id.send_message(
        ctx,
        serenity::CreateMessage::new().embed(embed).components(vec![row]),
    ).await;

    true
}
