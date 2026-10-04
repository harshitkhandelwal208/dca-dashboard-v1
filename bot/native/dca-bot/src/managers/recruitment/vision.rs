//! Screenshot check for recruitment: is this an in-game HCR2 screenshot (licence / team-event screen)? One cheap pass.

use crate::app::App;
use dca_core::reader::{Reader, ScreenKind};
use dca_core::standings::clean_team_label;
use dca_state::config::DashboardConfig;
use dca_state::models::{Attachment, EventScore, LicenseAnalysis, Ticket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImageKind {
    DriverLicense,
    TeamEventScore,
    Unknown,
}

#[derive(Clone, Debug)]
pub struct ImageCheck {
    pub attachment: Attachment,
    pub kind: ImageKind,
    pub reason: String,
    pub original_field: &'static str,
    pub verification: Verification,
}

/// How an applicant's screenshot was accepted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verification {
    /// Recognised instantly from its text labels.
    Quick,
    /// The check ran out of time: accepted on trust, recruiters are told.
    Unverified,
}

fn seconds_from_env(name: &str, default: u64) -> Duration {
    Duration::from_secs(std::env::var(name).ok().and_then(|v| v.trim().parse().ok()).filter(|v| *v > 0).unwrap_or(default))
}

/// Applicants never wait longer than this for the instant check.
pub fn quick_budget(app: &App) -> Duration {
    match app.quick_secs.load(Ordering::Relaxed) {
        0 => seconds_from_env("DCA_OCR_QUICK_SECS", 20),
        n => Duration::from_secs(n),
    }
}

/// The instant text-label check, on its own lane. `None` when it ran out of time.
async fn quick_kind(app: &App, reader: Arc<Reader>, bytes: Vec<u8>) -> Option<Result<ScreenKind, String>> {
    let budget = quick_budget(app);
    let deadline = Instant::now() + budget;
    let _permit = tokio::time::timeout(budget, app.quick_gate.acquire()).await.ok()?.ok()?;
    let cancel = Arc::new(AtomicBool::new(false));
    let flag = cancel.clone();
    let handle = tokio::task::spawn_blocking(move || reader.classify_quick(&bytes, &flag));
    match tokio::time::timeout(deadline.saturating_duration_since(Instant::now()), handle).await {
        Ok(Ok(result)) => Some(result),
        Ok(Err(e)) => Some(Err(e.to_string())),
        Err(_) => {
            cancel.store(true, Ordering::Relaxed);
            None
        }
    }
}

pub fn kind_of(kind: ScreenKind) -> (ImageKind, &'static str) {
    match kind {
        ScreenKind::DriverLicence => (ImageKind::DriverLicense, "Profile/license labels were visible."),
        ScreenKind::Standings | ScreenKind::Podium => (ImageKind::TeamEventScore, "Team-event result labels were visible."),
        ScreenKind::Unknown => (ImageKind::Unknown, "No HCR2 recruitment screenshot markers were visible."),
    }
}

pub fn known_team_names(config: &DashboardConfig) -> Vec<String> {
    let mut names: Vec<String> = config.recruitment.teams.clone();
    for t in &config.member_counts.teams {
        names.push(t.name.clone());
        names.extend(t.aliases.iter().cloned());
    }
    names.retain(|n| !n.is_empty());
    names.dedup();
    names
}

pub async fn download(app: &App, url: &str) -> Result<Vec<u8>, String> {
    let response = app.web.get(url).send().await.map_err(|e| format!("Could not download the screenshot: {e}"))?;
    if !response.status().is_success() {
        return Err(format!("Could not download the screenshot ({}).", response.status().as_u16()));
    }
    let bytes = response.bytes().await.map_err(|e| e.to_string())?;
    if bytes.len() > 40 * 1024 * 1024 {
        return Err("The screenshot is too large.".into());
    }
    Ok(bytes.to_vec())
}

/// Read one team-event screenshot (one text pass) off the async runtime, one image at a time.
pub async fn read_event_bytes(app: &App, reader: Arc<Reader>, bytes: Vec<u8>, known: Vec<String>) -> Result<(dca_core::standings::StandingsPage, String), String> {
    let _permit = app.ocr_gate.acquire().await.map_err(|e| e.to_string())?;
    tokio::task::spawn_blocking(move || reader.read_event_screenshot(&bytes, &known)).await.map_err(|e| e.to_string())?
}

/// Is each upload a screenshot of the game? One text pass per image, on its own lane and within a deadline: the
/// profile card, the team-event table and the result screen are recognised from their labels. Anything else is not
/// accepted; an upload that cannot be checked in time is accepted on trust (the recruiters are told). Nothing is read
/// from the screenshots beyond that. `None` means the OCR models are unavailable (callers fail open).
pub async fn classify(app: &App, inputs: &[(Attachment, &'static str)], _config: &DashboardConfig) -> Option<Vec<ImageCheck>> {
    let reader = app.reader()?;
    let mut out = Vec::new();
    for (attachment, field) in inputs {
        let slot_kind = if *field == "licenseAttachments" { ImageKind::DriverLicense } else { ImageKind::TeamEventScore };
        let (kind, reason, verification) = match download(app, &attachment.url).await {
            Err(e) => (ImageKind::Unknown, e, Verification::Quick),
            Ok(bytes) => match quick_kind(app, reader.clone(), bytes).await {
                Some(Ok(k)) if k != ScreenKind::Unknown => {
                    let (kind, why) = kind_of(k);
                    (kind, why.to_string(), Verification::Quick)
                }
                Some(Ok(_)) => (ImageKind::Unknown, "No HCR2 screenshot markers were visible.".to_string(), Verification::Quick),
                Some(Err(e)) => (ImageKind::Unknown, e, Verification::Quick),
                None => (slot_kind, "The check ran out of time, so the screenshot was accepted without being read.".to_string(), Verification::Unverified),
            },
        };
        out.push(ImageCheck { attachment: attachment.clone(), kind, reason, original_field: field, verification });
    }
    Some(out)
}

/// The one reading a recruitment gets, run in the background when a ticket is closed: the in-game name, previous team
/// and garage power from the first licence screenshot, and the applicant's (gold) row from the first team-event
/// screenshot. One text pass per image, nothing else.
pub async fn read_ticket_stats(app: &App, ticket: &Ticket, config: &DashboardConfig) -> LicenseAnalysis {
    let mut analysis = LicenseAnalysis { discord_id: ticket.applicant_id.clone(), accepted_team: ticket.team.clone(), ..Default::default() };
    let Some(reader) = app.reader() else {
        analysis.error = "The local OCR models are not available, so the screenshots were not read.".into();
        return analysis;
    };
    let known = known_team_names(config);
    let licence = fresh_attachments(app, &ticket.license_attachments).await.into_iter().find(|a| !a.url.is_empty());
    match licence {
        None => analysis.error = "No license image was attached to this ticket.".into(),
        Some(att) => match download(app, &att.url).await {
            Err(e) => analysis.error = e,
            Ok(bytes) => {
                let _permit = app.ocr_gate.acquire().await;
                let (r, k) = (reader.clone(), known.clone());
                match tokio::task::spawn_blocking(move || r.read_licence_once(&bytes, &k)).await {
                    Ok(Ok(l)) => {
                        analysis.in_game_name = l.name.clone();
                        let team = clean_team_label(&l.team, &known);
                        analysis.source_team = team.clone();
                        analysis.previous_team = team;
                        analysis.garage_power = l.garage_power.map(|v| v.to_string()).unwrap_or_default();
                        analysis.cup_points = l.cup_points.map(|v| v.to_string()).unwrap_or_default();
                        analysis.season_points = l.season_points.map(|v| v.to_string()).unwrap_or_default();
                        analysis.adventurer_rank = l.adventurer_rank.clone();
                        analysis.raw_text = l.raw_text.clone();
                        analysis.confidence = l.name_confidence as f64;
                    }
                    Ok(Err(e)) => analysis.error = e,
                    Err(e) => analysis.error = e.to_string(),
                }
            }
        },
    }
    let event = fresh_attachments(app, &ticket.event_attachments).await.into_iter().find(|a| !a.url.is_empty());
    if let Some(att) = event {
        if let Ok(bytes) = download(app, &att.url).await {
            if let Ok((page, _)) = read_event_bytes(app, reader.clone(), bytes, known.clone()).await {
                let title = page.header.title.clone();
                // The applicant is the player whose row is drawn in gold (the reader's own row).
                if let Some(row) = page.rows.iter().find(|r| r.gold_text) {
                    analysis.event_scores.push(EventScore {
                        event_name: if title.is_empty() { "Team Event".into() } else { title },
                        rank: if row.rank > 0 { row.rank.to_string() } else { String::new() },
                        event_points: if row.rank > 0 { dca_core::session::event_points_for_rank(row.rank).to_string() } else { String::new() },
                        score: row.score.map(|s| s.to_string()).unwrap_or_default(),
                        raw_text: row.raw.clone(),
                    });
                }
            }
        }
    }
    analysis
}

/// Attachment URLs expire after ~24h; DM-mirrored copies are re-fetched from their message for a fresh signed URL.
pub async fn fresh_url(app: &App, att: &Attachment) -> String {
    use serenity::all::*;
    if att.dm_user_id.is_empty() || att.dm_message_id.is_empty() {
        return att.url.clone();
    }
    let (Some(user), Some(message)) = (crate::util::user_id(&att.dm_user_id), crate::util::snowflake(&att.dm_message_id)) else { return att.url.clone() };
    let Ok(dm) = user.create_dm_channel(&app.http).await else { return att.url.clone() };
    match dm.id.message(&app.http, MessageId::new(message)).await {
        Ok(m) => m.attachments.first().map(|a| a.url.clone()).unwrap_or_else(|| att.url.clone()),
        Err(_) => att.url.clone(),
    }
}

pub async fn fresh_attachments(app: &App, list: &[Attachment]) -> Vec<Attachment> {
    let mut out = Vec::new();
    for a in list {
        let mut copy = a.clone();
        copy.url = fresh_url(app, a).await;
        out.push(copy);
    }
    out
}
