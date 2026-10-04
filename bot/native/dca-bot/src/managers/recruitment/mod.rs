//! Recruitment: the Apply panel, the private application flow (screenshot upload + local verification), ticket
//! threads and their controls. Port of `recruitmentManager.js`.

pub mod vision;

use crate::app::{App, ApplySession};
use crate::logs::{log_action, LogEntry};
use crate::managers::member_count::increment_team_count;
use crate::managers::team_roles::queue_team_role_assignment;
use crate::responder::{ReplyData, Responder};
use crate::util::*;
use dca_state::config::{update_config, DashboardConfig};
use dca_state::models::{Attachment, LicenseAnalysis, Ticket, Transcript};
use dca_state::stores::*;
use dca_state::util::{now_iso, random_hex};
use serde_json::json;
use serenity::all::*;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use vision::*;

pub const APPLY_BUTTON_ID: &str = "recruitment:apply";
const EVENT_YES_PREFIX: &str = "recruitment:event:yes:";
const EVENT_NO_PREFIX: &str = "recruitment:event:no:";
const CLAIM_ID: &str = "recruitment:claim";
const CLOSE_ID: &str = "recruitment:close";
const CLOSE_TEAM_PREFIX: &str = "recruitment:close-team:";
const TUTORIAL_PREFIX: &str = "recruitment:tutorial:";
const SESSION_TIMEOUT: Duration = Duration::from_secs(10 * 60);
const MAX_SCREENSHOT_ATTEMPTS: u32 = 3;

pub struct Outcome {
    pub id: &'static str,
    pub team: &'static str,
}

pub const RECRUITMENT_OUTCOMES: [Outcome; 5] = [
    Outcome { id: "discord", team: "Discord" },
    Outcome { id: "discord2", team: "Discord\u{b2}" },
    Outcome { id: "discord3", team: "Discord 3\u{2122}" },
    Outcome { id: "nascar-dc", team: "Nascar DC" },
    Outcome { id: "rejected", team: "" },
];

// ------------------------------------------------------------------------------------------ helpers

pub fn recruiter_role_id(config: &DashboardConfig) -> String {
    [
        config.recruitment.recruiter_role_id.clone(),
        config.bot.recruiter_role_id.clone(),
        std::env::var("RECRUITER_ROLE_ID").unwrap_or_default(),
        std::env::var("RECRUITMENT_RECRUITER_ROLE_ID").unwrap_or_default(),
    ]
    .into_iter()
    .find(|v| !v.is_empty())
    .unwrap_or_default()
}

fn screenshot_dm_user_id(config: &DashboardConfig) -> String {
    if !config.recruitment.screenshot_dm_user_id.is_empty() {
        config.recruitment.screenshot_dm_user_id.clone()
    } else {
        std::env::var("RECRUITMENT_SCREENSHOT_DM_USER_ID").unwrap_or_default()
    }
}

fn dashboard_base_url(config: &DashboardConfig) -> String {
    [config.bot.dashboard_url.clone(), std::env::var("DASHBOARD_BASE_URL").unwrap_or_default(), std::env::var("DASHBOARD_PUBLIC_URL").unwrap_or_default()]
        .into_iter()
        .find(|v| !v.is_empty())
        .unwrap_or_default()
        .trim_end_matches('/')
        .to_string()
}

fn absolute_guide_url(config: &DashboardConfig, value: &str) -> String {
    let text = value.trim();
    if text.is_empty() {
        return String::new();
    }
    if text.starts_with("http://") || text.starts_with("https://") {
        return text.to_string();
    }
    let base = dashboard_base_url(config);
    if base.is_empty() {
        return text.to_string();
    }
    format!("{base}{}{text}", if text.starts_with('/') { "" } else { "/" })
}

fn thread_name(user: &User) -> String {
    let base = user.name.replace(['\r', '\n', '\t'], " ");
    let name = truncate(base.trim_matches('-').trim(), 90);
    if name.is_empty() {
        "applicant".into()
    } else {
        name
    }
}

fn safe_ticket_name(value: &str) -> String {
    let lowered = value.to_lowercase();
    let filtered: String = lowered.chars().filter(|c| c.is_ascii_alphanumeric() || matches!(c, ' ' | '_' | '-')).collect();
    let dashed = filtered.split_whitespace().collect::<Vec<_>>().join("-");
    truncate(dashed.trim_matches('-'), 90)
}

fn attachment_from(a: &serenity::all::Attachment) -> Attachment {
    Attachment {
        id: a.id.to_string(),
        name: a.filename.clone(),
        url: a.url.clone(),
        proxy_url: a.proxy_url.clone(),
        content_type: a.content_type.clone().unwrap_or_default(),
        size: a.size as u64,
        ..Default::default()
    }
}

fn extract_image_attachments(message: &Message) -> Vec<Attachment> {
    message.attachments.iter().filter(|a| is_image_attachment(a)).take(10).map(attachment_from).collect()
}

fn safe_attachment_name(name: &str, fallback: &str) -> String {
    let cleaned = safe_name(name, fallback, 80);
    if cleaned.is_empty() {
        fallback.to_string()
    } else {
        cleaned
    }
}

fn team_outcome_id(index: usize) -> String {
    format!("team:{index}")
}

fn team_from_outcome_id(id: &str, config: &DashboardConfig) -> Option<String> {
    let idx: usize = id.strip_prefix("team:")?.parse().ok()?;
    config.recruitment.teams.get(idx).cloned()
}

struct ResolvedOutcome {
    id: String,
    team: String,
}

fn outcome_from_id(id: &str, config: &DashboardConfig) -> Option<ResolvedOutcome> {
    if id == "rejected" {
        return Some(ResolvedOutcome { id: "rejected".into(), team: String::new() });
    }
    if let Some(team) = team_from_outcome_id(id, config) {
        let team = config.recruitment.teams.iter().find(|t| **t == team).cloned().unwrap_or(team);
        return Some(ResolvedOutcome { id: id.to_string(), team });
    }
    RECRUITMENT_OUTCOMES.iter().find(|o| o.id == id).map(|o| ResolvedOutcome { id: o.id.to_string(), team: o.team.to_string() })
}

// ------------------------------------------------------------------------------------------- panel

pub fn build_panel_payload(config: &DashboardConfig) -> (CreateEmbed, Vec<CreateActionRow>) {
    let r = &config.recruitment;
    let embed = CreateEmbed::new().title(&r.panel_title).description(&r.panel_description).colour(embed_color(&r.panel_color)).timestamp(Timestamp::now());
    let row = CreateActionRow::Buttons(vec![CreateButton::new(APPLY_BUTTON_ID).label("Apply!").style(ButtonStyle::Primary)]);
    (embed, vec![row])
}

pub struct PanelSync {
    pub skipped: bool,
    pub reason: String,
    pub created: bool,
    pub channel_id: String,
    pub message_id: String,
}

fn panel_skipped(reason: &str) -> PanelSync {
    PanelSync { skipped: true, reason: reason.into(), created: false, channel_id: String::new(), message_id: String::new() }
}

fn panel_is_current(message: &Message, config: &DashboardConfig) -> bool {
    let r = &config.recruitment;
    let Some(embed) = message.embeds.first() else { return false };
    embed.title.as_deref() == Some(r.panel_title.as_str())
        && embed.description.as_deref() == Some(r.panel_description.as_str())
        && embed.colour.map(|c| c.0) == Some(color_to_number(&r.panel_color))
        && message
            .components
            .iter()
            .any(|row| row.components.iter().any(|c| matches!(c, ActionRowComponent::Button(b) if matches!(&b.data, ButtonKind::NonLink { custom_id, .. } if custom_id == APPLY_BUTTON_ID))))
}

/// Post or refresh the Apply panel (only edited when it actually differs) and tidy the channel.
pub async fn ensure_recruitment_panel(app: &App) -> PanelSync {
    let config = app.config().await;
    let r = &config.recruitment;
    if !r.enabled {
        return panel_skipped("Recruitment is disabled.");
    }
    let Some(channel) = channel_id(&r.panel_channel_id) else { return panel_skipped("Recruitment panel channel is not configured.") };
    match channel.to_channel(&app.http).await {
        Ok(Channel::Guild(_)) => {}
        _ => return panel_skipped("Recruitment panel channel is not text based."),
    }
    let Ok(bot) = app.ensure_bot_id().await else { return panel_skipped("Bot user is unavailable.") };

    let mut message: Option<Message> = None;
    if let Some(id) = snowflake(&r.panel_message_id) {
        message = channel.message(&app.http, MessageId::new(id)).await.ok().filter(|m| m.author.id == bot);
    }
    let created = message.is_none();
    let (embed, rows) = build_panel_payload(&config);
    let message = match message {
        Some(m) if panel_is_current(&m, &config) => m,
        Some(m) => match channel.edit_message(&app.http, m.id, EditMessage::new().embeds(vec![embed]).components(rows)).await {
            Ok(m) => m,
            Err(error) => return panel_skipped(&format!("Could not edit the panel: {error}")),
        },
        None => match channel.send_message(&app.http, CreateMessage::new().embed(embed).components(rows)).await {
            Ok(m) => m,
            Err(error) => return panel_skipped(&format!("Could not post the panel: {error}")),
        },
    };
    let mut cfg = config.clone();
    if message.id.to_string() != r.panel_message_id {
        let id = message.id.to_string();
        if let Ok((saved, _)) = update_config(&app.store, |c| c.recruitment.panel_message_id = id).await {
            cfg = saved;
        }
    }
    let _ = clean_recruitment_panel_channel(app, Some(&cfg)).await;
    PanelSync { skipped: false, reason: String::new(), created, channel_id: channel.to_string(), message_id: message.id.to_string() }
}

/// Messages younger than this are left alone so an upload that is being picked up by an Apply session is not removed
/// from under it (the session deletes the upload itself once it has the image).
const PANEL_GRACE_SECS: i64 = 5;

/// Delete everything in the panel channel except the panel itself (screenshots and chatter must not stay visible).
/// Returns the number of messages deleted.
pub async fn clean_recruitment_panel_channel(app: &App, config: Option<&DashboardConfig>) -> usize {
    let owned;
    let config = match config {
        Some(c) => c,
        None => {
            owned = app.config().await;
            &owned
        }
    };
    let (Some(channel), Some(panel)) = (channel_id(&config.recruitment.panel_channel_id), snowflake(&config.recruitment.panel_message_id)) else { return 0 };
    let panel = MessageId::new(panel);
    let now = Timestamp::now().unix_timestamp();
    let (mut deleted, mut scanned, mut before): (usize, usize, Option<MessageId>) = (0, 0, None);
    while scanned < 500 {
        let mut builder = GetMessages::new().limit(100);
        if let Some(b) = before {
            builder = builder.before(b);
        }
        let messages = match channel.messages(&app.http, builder).await {
            Ok(messages) => {
                sweep_recovered();
                messages
            }
            Err(error) => {
                sweep_failed(&format!("could not read the panel channel {channel} to clean it: {error}"));
                break;
            }
        };
        if messages.is_empty() {
            break;
        }
        scanned += messages.len();
        before = messages.last().map(|m| m.id);
        let doomed: Vec<&Message> = messages.iter().filter(|m| m.id != panel && now - m.timestamp.unix_timestamp() >= PANEL_GRACE_SECS).collect();
        // Messages under 14 days old can go in one bulk request; the rest one by one.
        let (recent, old): (Vec<&Message>, Vec<&Message>) = doomed.into_iter().partition(|m| now - m.timestamp.unix_timestamp() < 13 * 86_400);
        let mut singles: Vec<MessageId> = old.iter().map(|m| m.id).collect();
        if recent.len() >= 2 {
            let ids: Vec<MessageId> = recent.iter().map(|m| m.id).collect();
            match channel.delete_messages(&app.http, ids.clone()).await {
                Ok(()) => deleted += ids.len(),
                Err(error) => {
                    tracing::warn!("bulk delete in the panel channel failed ({error}); deleting one by one");
                    singles.extend(ids);
                }
            }
        } else {
            singles.extend(recent.iter().map(|m| m.id));
        }
        for id in singles {
            match channel.delete_message(&app.http, id).await {
                Ok(()) => deleted += 1,
                Err(error) => tracing::warn!("could not delete message {id} in the panel channel: {error}"),
            }
        }
        if messages.len() < 100 {
            break;
        }
    }
    deleted
}

/// The sweep runs every 15 seconds: a problem that persists (say, the bot cannot see the channel) is logged once, and
/// again when it is over, not on every run.
static SWEEP_PROBLEM: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

fn sweep_failed(message: &str) {
    let mut last = SWEEP_PROBLEM.lock().unwrap();
    if last.as_deref() != Some(message) {
        tracing::warn!("{message}");
        *last = Some(message.to_string());
    }
}

fn sweep_recovered() {
    if SWEEP_PROBLEM.lock().unwrap().take().is_some() {
        tracing::info!("The Apply channel can be cleaned again.");
    }
}

/// Keeps the Apply channel empty apart from the panel: a sweep every 15 seconds, independent of the panel refresh.
pub fn start_panel_sweeper(app: Arc<App>) {
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_secs(15));
        tick.tick().await;
        loop {
            tick.tick().await;
            if !app.config().await.recruitment.enabled {
                continue;
            }
            let removed = clean_recruitment_panel_channel(&app, None).await;
            if removed > 0 {
                tracing::info!("Cleaned {removed} message(s) from the Apply channel.");
            }
        }
    });
}

// ----------------------------------------------------------------------------------- permissions

fn member_can_recruit(r: &Responder, config: &DashboardConfig) -> bool {
    let Some(member) = r.member() else { return false };
    let role = recruiter_role_id(config);
    let has_role = role_id(&role).is_some_and(|rid| member.roles.contains(&rid));
    let perms = r.permissions();
    has_role || perms.contains(Permissions::ADMINISTRATOR) || perms.contains(Permissions::MANAGE_GUILD) || perms.contains(Permissions::MANAGE_THREADS)
}

async fn require_recruiter(app: &App, r: &Responder) -> bool {
    let config = app.config().await;
    if member_can_recruit(r, &config) {
        return true;
    }
    let _ = r.reply(ReplyData::text("Only recruiters can use these ticket controls.").ephemeral()).await;
    false
}

pub async fn get_ticket_context(app: &App, r: &Responder) -> Option<(GuildChannel, Ticket)> {
    let channel = r.channel_id().to_channel(&app.http).await.ok().and_then(|c| c.guild());
    let Some(thread) = channel.filter(|c| matches!(c.kind, ChannelType::PublicThread | ChannelType::PrivateThread | ChannelType::NewsThread)) else {
        let _ = r.reply(ReplyData::text("Recruitment ticket commands must be used inside a ticket thread.").ephemeral()).await;
        return None;
    };
    let Some(ticket) = get_ticket(&app.store, &thread.id.to_string()).await else {
        let _ = r.reply(ReplyData::text("I could not find a ticket record for this thread.").ephemeral()).await;
        return None;
    };
    Some((thread, ticket))
}

// ----------------------------------------------------------------------------- uploading screenshots

struct CollectorGuard<'a> {
    app: &'a App,
    key: (ChannelId, UserId),
}

impl Drop for CollectorGuard<'_> {
    fn drop(&mut self) {
        self.app.collectors.lock().unwrap().remove(&self.key);
    }
}

/// Wait for the applicant's next message with an image in the channel (non-image messages are removed).
async fn wait_for_image_message(app: &App, r: &Responder, channel: ChannelId, user: UserId, invalid_notice: &str) -> Result<(Message, Vec<Attachment>), String> {
    let (tx, mut rx) = mpsc::unbounded_channel::<Message>();
    app.collectors.lock().unwrap().insert((channel, user), tx);
    let _guard = CollectorGuard { app, key: (channel, user) };
    let deadline = Instant::now() + SESSION_TIMEOUT;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        let received = tokio::time::timeout(remaining, rx.recv()).await;
        let message = match received {
            Ok(Some(m)) => m,
            _ => return Err("Timed out waiting for screenshots.".into()),
        };
        let attachments = extract_image_attachments(&message);
        if attachments.is_empty() {
            let _ = message.delete(&app.http).await;
            let _ = r.follow_up(ReplyData::text(invalid_notice).ephemeral()).await;
            continue;
        }
        return Ok((message, attachments));
    }
}

/// Copy screenshots into the configured DM so they outlive the (deleted) public upload.
async fn mirror_attachments_to_dm(app: &App, config: &DashboardConfig, attachments: &[Attachment], kind: &str, user: &User, guild: Option<GuildId>) -> Result<Vec<Attachment>, String> {
    let recipient = screenshot_dm_user_id(config);
    let Some(recipient_id) = user_id(&recipient) else { return Err("Screenshot DM user is not configured in the dashboard.".into()) };
    let recipient = app.http.get_user(recipient_id).await.map_err(|_| "I could not find the configured screenshot DM user.".to_string())?;
    let dm = recipient.create_dm_channel(&app.http).await.map_err(|e| e.to_string())?;
    let mut mirrored = Vec::new();
    for (index, attachment) in attachments.iter().enumerate() {
        let bytes = download(app, &attachment.url).await.map_err(|_| format!("Could not download {} (404).", if attachment.name.is_empty() { "screenshot" } else { &attachment.name }))?;
        let name = safe_attachment_name(&attachment.name, &format!("{}-{}-{}.png", kind, user.id, index + 1));
        let content = [format!("Recruitment {kind} upload"), format!("Applicant: <@{}> ({})", user.id, display_tag(user)), guild.map(|g| format!("Guild: {g}")).unwrap_or_default()]
            .into_iter()
            .filter(|l| !l.is_empty())
            .collect::<Vec<_>>()
            .join("\n");
        let sent = dm
            .send_message(&app.http, CreateMessage::new().content(content).add_file(CreateAttachment::bytes(bytes.clone(), name.clone())).allowed_mentions(CreateAllowedMentions::new()))
            .await
            .map_err(|e| format!("Could not store the screenshot: {e}"))?;
        let Some(copy) = sent.attachments.first() else { return Err("Discord did not return a URL for the mirrored screenshot.".into()) };
        let mut m = attachment.clone();
        m.name = name;
        m.url = copy.url.clone();
        m.proxy_url = copy.proxy_url.clone();
        m.content_type = copy.content_type.clone().unwrap_or_else(|| attachment.content_type.clone());
        m.size = copy.size as u64;
        m.source = "dm-mirror".into();
        m.dm_user_id = recipient_id.to_string();
        m.dm_message_id = sent.id.to_string();
        m.original_url = attachment.url.clone();
        mirrored.push(m);
    }
    Ok(mirrored)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ShotType {
    License,
    Event,
}

impl ShotType {
    fn parse(value: &str) -> Option<ShotType> {
        match value {
            "license" => Some(ShotType::License),
            "event" => Some(ShotType::Event),
            _ => None,
        }
    }
    fn label(self) -> &'static str {
        match self {
            ShotType::License => "driver's license",
            ShotType::Event => "team event scores",
        }
    }
    fn plural(self) -> &'static str {
        match self {
            ShotType::License => "driver's license screenshots",
            ShotType::Event => "team event score screenshots",
        }
    }
    fn guide_kind(self) -> &'static str {
        match self {
            ShotType::License => "driver-license",
            ShotType::Event => "team-event-score",
        }
    }
    fn guide_label(self) -> &'static str {
        match self {
            ShotType::License => "driver's license screenshot",
            ShotType::Event => "team event score screenshot",
        }
    }
}

fn clean_screenshot_list(items: Vec<Attachment>) -> Vec<Attachment> {
    let mut seen = std::collections::HashSet::new();
    items.into_iter().filter(|i| !i.url.is_empty() && seen.insert(i.url.clone())).collect()
}

struct Classified {
    licence: Vec<Attachment>,
    events: Vec<Attachment>,
}

/// Port of `classifyApplicationScreenshots`: sort uploads into licence / event lists and reject wrong ones.
async fn classify_application_screenshots(app: &App, session: &mut ApplySession, config: &DashboardConfig, shot: ShotType, require_event: bool) -> Result<(), (ShotType, String)> {
    let inputs: Vec<(Attachment, &'static str)> = session.license.iter().map(|a| (a.clone(), "licenseAttachments")).chain(session.events.iter().map(|a| (a.clone(), "eventAttachments"))).collect();
    let Some(checks) = classify(app, &inputs, config).await else {
        // No OCR models available: accept the uploads unverified rather than blocking applicants.
        if session.license.is_empty() {
            return Err((ShotType::License, "I could not find a valid HCR2 driver's license/profile screenshot. Upload the profile screen with Garage power visible.".into()));
        }
        return Ok(());
    };
    if checks.iter().any(|c| c.verification == Verification::Unverified) {
        session.unverified = true;
    }
    let classified = Classified {
        licence: clean_screenshot_list(checks.iter().filter(|c| c.kind == ImageKind::DriverLicense).map(|c| c.attachment.clone()).collect()),
        events: clean_screenshot_list(checks.iter().filter(|c| c.kind == ImageKind::TeamEventScore).map(|c| c.attachment.clone()).collect()),
    };
    let accepted: std::collections::HashSet<&str> = classified.licence.iter().chain(classified.events.iter()).map(|a| a.url.as_str()).collect();
    let invalid: Vec<&ImageCheck> = checks.iter().filter(|c| c.kind == ImageKind::Unknown && !accepted.contains(c.attachment.url.as_str())).collect();

    if classified.licence.is_empty() {
        return Err((ShotType::License, "I could not find a valid HCR2 driver's license/profile screenshot. Upload the profile screen with Garage power visible.".into()));
    }
    if (require_event || !session.events.is_empty()) && classified.events.is_empty() {
        return Err((ShotType::Event, "I could not find a valid HCR2 team-event result, score summary, or final standings screenshot in the team event upload.".into()));
    }
    if !invalid.is_empty() {
        let names = invalid.iter().take(3).map(|c| if c.attachment.name.is_empty() { "screenshot".to_string() } else { c.attachment.name.clone() }).collect::<Vec<_>>().join(", ");
        return Err((shot, format!("{names} did not look like a valid HCR2 driver's license or team-event score screenshot.")));
    }
    session.license = classified.licence;
    session.events = classified.events;
    Ok(())
}

async fn send_screenshot_guide(app: &App, r: &Responder, config: &DashboardConfig, shot: ShotType, reason: &str) {
    let kind = shot.guide_kind();
    let tutorials = &config.recruitment.tutorials;
    let tutorial = tutorials.iter().find(|t| t.image_kind == kind && !t.guide_image_url.is_empty()).or_else(|| tutorials.iter().find(|t| !t.guide_image_url.is_empty()));
    let guide_url = absolute_guide_url(config, tutorial.map(|t| t.guide_image_url.as_str()).unwrap_or(""));
    let description = truncate(
        &[reason.to_string(), format!("Please upload a correct {} in this channel.", shot.guide_label()), tutorial.map(|t| t.description.clone()).unwrap_or_default()]
            .into_iter()
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join("\n\n"),
        4000,
    );
    let mut embed = CreateEmbed::new().title(format!("Correct {}", shot.guide_label())).description(description).colour(embed_color(&config.recruitment.panel_color));
    if !guide_url.is_empty() {
        embed = embed.image(guide_url.clone());
    }
    let content = match tutorial {
        Some(t) if !t.video_url.is_empty() => format!("Guide video: {}", t.video_url),
        _ if !guide_url.is_empty() => "Use the guide image below as the expected screenshot format.".to_string(),
        _ => "Please upload the correct screenshot format and try again.".to_string(),
    };
    let _ = app;
    let _ = r.reply(ReplyData::text(content).embed(embed).ephemeral()).await;
}

async fn collect_validated_screenshot_upload(app: &App, r: &Responder, session: &mut ApplySession, config: &DashboardConfig, shot: ShotType, invalid_notice: &str) -> Result<(), String> {
    let require_event = shot == ShotType::Event;
    for attempt in 1..=MAX_SCREENSHOT_ATTEMPTS {
        let (message, attachments) = wait_for_image_message(app, r, r.channel_id(), r.user().id, invalid_notice).await?;
        let mirrored = mirror_attachments_to_dm(app, config, &attachments, shot.label(), r.user(), r.guild_id()).await;
        let _ = message.delete(&app.http).await;
        let _ = clean_recruitment_panel_channel(app, Some(config)).await;
        let mirrored = mirrored?;

        match shot {
            ShotType::License => session.license = mirrored,
            ShotType::Event => session.events = mirrored,
        }
        match classify_application_screenshots(app, session, config, shot, require_event).await {
            Ok(()) => return Ok(()),
            Err((guide_type, message)) => {
                match shot {
                    ShotType::License => session.license = Vec::new(),
                    ShotType::Event => session.events = Vec::new(),
                }
                send_screenshot_guide(app, r, config, guide_type, &format!("{message}\n\nAttempt {attempt}/{MAX_SCREENSHOT_ATTEMPTS}.")).await;
            }
        }
    }
    Err(format!("Too many invalid {}. Press **Apply!** again when you have the correct image ready.", shot.plural()))
}

// ------------------------------------------------------------------------------------ apply flow

fn find_active_session(app: &App, guild: GuildId, user: UserId) -> bool {
    app.apply_sessions.lock().unwrap().values().any(|s| s.guild_id == guild && s.user_id == user)
}

fn event_decision_row(token: &str) -> CreateActionRow {
    CreateActionRow::Buttons(vec![
        CreateButton::new(format!("{EVENT_YES_PREFIX}{token}")).label("Yes").style(ButtonStyle::Success),
        CreateButton::new(format!("{EVENT_NO_PREFIX}{token}")).label("No").style(ButtonStyle::Secondary),
    ])
}

fn forget_session(app: &App, token: &str) {
    app.apply_sessions.lock().unwrap().remove(token);
}

pub async fn collect_license(app: Arc<App>, r: Responder) {
    let mut config = app.config().await;
    let (Some(guild), user) = (r.guild_id(), r.user().clone()) else {
        let _ = r.reply(ReplyData::text("Applications can only be started in a server.").ephemeral()).await;
        return;
    };
    // Open-ticket limit (the old bot allowed the setting but only ever checked one).
    let open = list_tickets(&app.store, &TicketFilter { status: Some("open".into()), applicant_id: Some(user.id.to_string()) }).await;
    let max_open = config.recruitment.max_open_tickets_per_user.max(1) as usize;
    if open.len() >= max_open {
        let _ = r.reply(ReplyData::text(format!("You already have an open application ticket: <#{}>", open[0].thread_id)).ephemeral()).await;
        return;
    }
    if find_active_session(&app, guild, user.id) {
        let _ = r.reply(ReplyData::text("You already have an application upload in progress. Finish that upload or wait for it to time out before pressing **Apply!** again.").ephemeral()).await;
        return;
    }
    if screenshot_dm_user_id(&config).is_empty() {
        let _ = r.reply(ReplyData::text("Recruitment screenshot storage is not configured yet. Ask a manager to set the screenshot DM user in the dashboard.").ephemeral()).await;
        return;
    }
    if config.recruitment.panel_message_id.is_empty() {
        if let Some(m) = r.component_message() {
            let id = m.id.to_string();
            if let Ok((saved, _)) = update_config(&app.store, |c| c.recruitment.panel_message_id = id).await {
                config = saved;
            }
        }
    }

    let token = random_hex(16);
    let mut session = ApplySession {
        token: token.clone(),
        user_id: user.id,
        user_tag: display_tag(&user),
        username: user.name.clone(),
        guild_id: guild,
        channel_id: r.channel_id(),
        license: Vec::new(),
        events: Vec::new(),
        unverified: false,
    };
    app.apply_sessions.lock().unwrap().insert(token.clone(), session.clone());
    {
        let app2 = app.clone();
        let t = token.clone();
        tokio::spawn(async move {
            tokio::time::sleep(SESSION_TIMEOUT * 3).await;
            forget_session(&app2, &t);
        });
    }

    let _ = r.reply(ReplyData::text("Upload your in-game driver's license screenshot in this channel now. I will store it privately and remove your visible upload right away.").ephemeral()).await;

    let result = collect_validated_screenshot_upload(
        &app,
        &r,
        &mut session,
        &config,
        ShotType::License,
        "That message did not include an image attachment, so I removed it. Please upload the driver's license screenshot as an image file.",
    )
    .await;
    match result {
        Ok(()) if !session.events.is_empty() => {
            // The upload already holds the licence and team-event screenshots, and both were recognised: nothing left to ask.
            let _ = r.edit_reply(ReplyData::text("Driver's license and team event screenshots captured. Creating your application ticket now.").components(vec![])).await;
            let outcome = create_application_thread(&app, &session).await;
            forget_session(&app, &token);
            match outcome {
                Ok((thread, _)) => {
                    let _ = r.follow_up(ReplyData::text(format!("Your application ticket has been created: <#{}>", thread.id)).ephemeral()).await;
                }
                Err(error) => {
                    let _ = r.follow_up(ReplyData::text(format!("I could not create the ticket: {error}")).ephemeral()).await;
                }
            }
        }
        Ok(()) => {
            app.apply_sessions.lock().unwrap().insert(token.clone(), session.clone());
            let _ = r.edit_reply(ReplyData::text("Driver's license captured. Do you have team event score screenshots to add?").components(vec![event_decision_row(&token)])).await;
        }
        Err(error) => {
            forget_session(&app, &token);
            let content = if error.contains("Timed out") {
                "Application timed out because no driver's license image was uploaded in time. Press **Apply!** again when you are ready.".to_string()
            } else {
                format!("I could not process that screenshot: {error}")
            };
            let _ = r.edit_reply(ReplyData::text(content).components(vec![])).await;
        }
    }
}

fn session_for_button(app: &App, custom_id: &str, prefix: &str, user: UserId) -> Option<ApplySession> {
    let token = custom_id.strip_prefix(prefix)?;
    let s = app.apply_sessions.lock().unwrap().get(token).cloned()?;
    (s.user_id == user).then_some(s)
}

async fn complete_without_events(app: Arc<App>, r: Responder, custom_id: String) {
    let Some(session) = session_for_button(&app, &custom_id, EVENT_NO_PREFIX, r.user().id) else {
        let _ = r.reply(ReplyData::text("That application prompt expired. Press **Apply!** again when you are ready.").ephemeral()).await;
        return;
    };
    let _ = r.update(ReplyData::text("Got it. Creating your application ticket now.").components(vec![])).await;
    // The licence was already checked when it was uploaded: nothing is read again, the ticket is created right away.
    let outcome: Result<GuildChannel, String> = create_application_thread(&app, &session).await.map(|(t, _)| t);
    forget_session(&app, &session.token);
    match outcome {
        Ok(thread) => {
            let _ = r.follow_up(ReplyData::text(format!("Your application ticket has been created: <#{}>", thread.id)).ephemeral()).await;
        }
        Err(error) => {
            let _ = r.follow_up(ReplyData::text(format!("I could not create the ticket: {error}")).ephemeral()).await;
        }
    }
}

async fn collect_event_screenshots(app: Arc<App>, r: Responder, custom_id: String) {
    let Some(mut session) = session_for_button(&app, &custom_id, EVENT_YES_PREFIX, r.user().id) else {
        let _ = r.reply(ReplyData::text("That application prompt expired. Press **Apply!** again when you are ready.").ephemeral()).await;
        return;
    };
    let _ = r.update(ReplyData::text("Upload the team event score screenshots in this channel now. I will store them privately and remove the visible upload right away.").components(vec![])).await;
    let config = app.config().await;
    let result = collect_validated_screenshot_upload(
        &app,
        &r,
        &mut session,
        &config,
        ShotType::Event,
        "That message did not include an image attachment, so I removed it. Please upload the team event scores as image files.",
    )
    .await;
    let outcome = match result {
        Ok(()) => create_application_thread(&app, &session).await.map(|(t, _)| t),
        Err(e) => Err(e),
    };
    forget_session(&app, &session.token);
    match outcome {
        Ok(thread) => {
            let _ = r.follow_up(ReplyData::text(format!("Your application ticket has been created: <#{}>", thread.id)).ephemeral()).await;
        }
        Err(error) => {
            let content = if error.contains("Timed out") {
                "Application timed out because no team event screenshot was uploaded in time. Press **Apply!** again when you are ready.".to_string()
            } else {
                format!("I could not process those screenshots: {error}")
            };
            let _ = r.follow_up(ReplyData::text(content).ephemeral()).await;
        }
    }
}

fn build_application_embeds(config: &DashboardConfig, session: &ApplySession) -> Vec<CreateEmbed> {
    let screenshots: Vec<&Attachment> = session.license.iter().chain(session.events.iter()).collect();
    let links = truncate(
        &screenshots.iter().enumerate().map(|(i, a)| format!("[{}]({})", if a.name.is_empty() { format!("Screenshot {}", i + 1) } else { a.name.clone() }, a.url)).collect::<Vec<_>>().join("\n"),
        1000,
    );
    let color = embed_color(&config.recruitment.panel_color);
    let mut first = CreateEmbed::new()
        .title("Recruitment Application")
        .description(format!("{}\n\n**Questions:**\n\n{}", config.recruitment.questions_intro, config.recruitment.questions))
        .colour(color)
        .field("Applicant", format!("<@{}>", session.user_id), true)
        .field("Team Event Screenshots", if session.events.is_empty() { "Not provided" } else { "Provided" }, true)
        .field("Screenshots", if links.is_empty() { "No screenshots captured.".to_string() } else { links }, false)
        .timestamp(Timestamp::now());
    if session.unverified {
        first = first.field(
            "Screenshot check",
            "\u{26a0}\u{fe0f} The automatic check ran out of time, so these screenshots were accepted without being read. Please confirm they are real HCR2 screenshots.",
            false,
        );
    }
    if let Some(a) = screenshots.first() {
        first = first.image(a.url.clone());
    }
    let mut embeds = vec![first];
    for (i, a) in screenshots.iter().enumerate().skip(1).take(8) {
        embeds.push(CreateEmbed::new().title(format!("Screenshot {}", i + 1)).url(a.url.clone()).image(a.url.clone()).colour(color));
    }
    embeds
}

fn ticket_controls() -> Vec<CreateActionRow> {
    vec![CreateActionRow::Buttons(vec![CreateButton::new(CLAIM_ID).label("Claim Ticket").style(ButtonStyle::Secondary), CreateButton::new(CLOSE_ID).label("Close Ticket").style(ButtonStyle::Danger)])]
}

fn auto_archive(minutes: u32) -> AutoArchiveDuration {
    match minutes {
        0..=1439 => AutoArchiveDuration::OneHour,
        1440..=4319 => AutoArchiveDuration::OneDay,
        4320..=10079 => AutoArchiveDuration::ThreeDays,
        _ => AutoArchiveDuration::OneWeek,
    }
}

pub async fn create_application_thread(app: &App, session: &ApplySession) -> Result<(GuildChannel, Ticket), String> {
    let config = app.config().await;
    let channel = session.channel_id.to_channel(&app.http).await.map_err(|_| "This channel does not support threads.".to_string())?;
    let Some(guild_channel) = channel.guild() else { return Err("This channel does not support threads.".into()) };
    let user = app.http.get_user(session.user_id).await.ok();
    let name = user.as_ref().map(thread_name).unwrap_or_else(|| truncate(&session.username, 90));

    let kind = if guild_channel.kind == ChannelType::News {
        ChannelType::NewsThread
    } else if config.recruitment.private_threads {
        ChannelType::PrivateThread
    } else {
        ChannelType::PublicThread
    };
    let make = |kind: ChannelType| {
        let mut b = CreateThread::new(name.clone()).kind(kind).auto_archive_duration(auto_archive(config.recruitment.thread_auto_archive_minutes));
        if kind == ChannelType::PrivateThread {
            b = b.invitable(false);
        }
        b
    };
    let thread = match guild_channel.id.create_thread(&app.http, make(kind)).await {
        Ok(t) => t,
        Err(error) if kind == ChannelType::PrivateThread => {
            // Servers without private threads (boost level) fall back to a public thread.
            tracing::warn!("private thread failed ({error}); falling back to a public thread");
            guild_channel.id.create_thread(&app.http, make(ChannelType::PublicThread)).await.map_err(|e| e.to_string())?
        }
        Err(error) => return Err(error.to_string()),
    };
    if let Err(error) = thread.id.add_thread_member(&app.http, session.user_id).await {
        tracing::warn!("could not add the applicant {} to thread {}: {error}", session.user_id, thread.id);
    }

    let role = recruiter_role_id(&config);
    let intro = if role.is_empty() {
        format!("New recruitment application from <@{}>. Configure the recruiter role in the dashboard to ping recruiters.", session.user_id)
    } else {
        format!("<@&{role}> New recruitment application from <@{}>.", session.user_id)
    };
    let mut message = CreateMessage::new().content(intro).embeds(build_application_embeds(&config, session)).components(ticket_controls());
    if let Some(rid) = role_id(&role) {
        message = message.allowed_mentions(CreateAllowedMentions::new().roles(vec![rid]).users(vec![session.user_id]));
    }
    thread.id.send_message(&app.http, message).await.map_err(|e| e.to_string())?;

    let ticket = save_ticket(
        &app.store,
        Ticket {
            thread_id: thread.id.to_string(),
            thread_name: thread.name.clone(),
            channel_id: guild_channel.id.to_string(),
            guild_id: session.guild_id.to_string(),
            applicant_id: session.user_id.to_string(),
            applicant_tag: user.as_ref().map(display_tag).unwrap_or_else(|| session.user_tag.clone()),
            applicant_username: user.as_ref().map(|u| u.name.clone()).unwrap_or_else(|| session.username.clone()),
            status: "open".into(),
            added_user_ids: vec![],
            license_attachments: session.license.clone(),
            event_attachments: session.events.clone(),
            created_at: now_iso(),
            ..Default::default()
        },
    )
    .await?;

    log_action(
        app,
        LogEntry::new("ticket", "Recruitment Ticket Created", format!("<@{}> opened a recruitment application thread.", session.user_id))
            .guild(session.guild_id)
            .actor(session.user_id, session.user_tag.clone())
            .meta(json!({ "threadId": thread.id.to_string(), "channelId": guild_channel.id.to_string() })),
    )
    .await;
    Ok((thread, ticket))
}

// ------------------------------------------------------------------------------- ticket controls

pub async fn claim_ticket(app: &App, r: &Responder) {
    if !require_recruiter(app, r).await {
        return;
    }
    let Some((thread, ticket)) = get_ticket_context(app, r).await else { return };
    if ticket.status == "closed" {
        let _ = r.ephemeral("This ticket is already closed.").await;
        return;
    }
    if !ticket.claimed_by_id.is_empty() {
        let _ = r
            .ephemeral(if ticket.claimed_by_id == r.user().id.to_string() {
                "You have already claimed this ticket.".to_string()
            } else {
                format!("This ticket is already claimed by <@{}>.", ticket.claimed_by_id)
            })
            .await;
        return;
    }
    let tag = display_tag(r.user());
    let uid = r.user().id.to_string();
    let updated = update_ticket(&app.store, &thread.id.to_string(), |t| {
        t.claimed_by_id = uid;
        t.claimed_by_tag = tag.clone();
    })
    .await
    .ok()
    .flatten();
    let _ = thread.id.say(&app.http, format!("Ticket claimed by <@{}>.", r.user().id)).await;
    log_action(
        app,
        LogEntry::new("ticket", "Recruitment Ticket Claimed", format!("<@{}> claimed the ticket for <@{}>.", r.user().id, ticket.applicant_id))
            .guild(&ticket.guild_id)
            .actor(r.user().id, display_tag(r.user()))
            .target(&ticket.applicant_id, &ticket.applicant_tag)
            .meta(json!({ "threadId": thread.id.to_string() })),
    )
    .await;
    let _ = r.ephemeral(format!("Ticket claimed for {}.", updated.map(|t| t.applicant_tag).unwrap_or(ticket.applicant_tag))).await;
}

fn build_close_outcome_rows(config: &DashboardConfig) -> Vec<CreateActionRow> {
    let mut outcomes: Vec<(String, String, ButtonStyle)> =
        config.recruitment.teams.iter().filter(|t| !t.is_empty()).take(24).enumerate().map(|(i, t)| (team_outcome_id(i), t.clone(), ButtonStyle::Success)).collect();
    outcomes.push(("rejected".into(), "Rejected".into(), ButtonStyle::Danger));
    outcomes
        .chunks(5)
        .map(|chunk| CreateActionRow::Buttons(chunk.iter().map(|(id, label, style)| CreateButton::new(format!("{CLOSE_TEAM_PREFIX}{id}")).label(label.clone()).style(*style)).collect()))
        .collect()
}

pub async fn start_close(app: &App, r: &Responder) {
    if !require_recruiter(app, r).await {
        return;
    }
    let config = app.config().await;
    let _ = r.reply(ReplyData::text("Select the final recruitment outcome for this applicant.").components(build_close_outcome_rows(&config)).ephemeral()).await;
}

pub async fn fetch_thread_messages(app: &App, thread: ChannelId, limit: usize) -> Vec<Message> {
    let mut collected: Vec<Message> = Vec::new();
    let mut before: Option<MessageId> = None;
    while collected.len() < limit {
        let mut b = GetMessages::new().limit(100.min((limit - collected.len()) as u8));
        if let Some(id) = before {
            b = b.before(id);
        }
        let Ok(batch) = thread.messages(&app.http, b).await else { break };
        if batch.is_empty() {
            break;
        }
        before = batch.last().map(|m| m.id);
        let n = batch.len();
        collected.extend(batch);
        if n < 100 {
            break;
        }
    }
    collected.sort_by_key(|m| m.timestamp);
    collected
}

pub fn transcript_line(m: &Message) -> String {
    let author = display_tag(&m.author);
    let mut extras: Vec<String> = m.attachments.iter().map(|a| a.url.clone()).collect();
    for e in &m.embeds {
        extras.extend(e.title.clone());
        extras.extend(e.description.clone());
        extras.extend(e.url.clone());
        extras.extend(e.image.as_ref().map(|i| i.url.clone()));
        extras.extend(e.thumbnail.as_ref().map(|i| i.url.clone()));
        for f in &e.fields {
            extras.push(format!("{}: {}", f.name, f.value));
        }
    }
    let extras: Vec<String> = extras.into_iter().filter(|s| !s.is_empty()).collect();
    let content = m.content.split_whitespace().collect::<Vec<_>>().join(" ");
    let tail = if extras.is_empty() { String::new() } else { format!(" {}", extras.join(" ")) };
    format!("[{}] {}: {}{}", m.timestamp, author, content, tail).trim().to_string()
}

pub async fn create_transcript(app: &App, thread: ChannelId, limit: usize) -> String {
    let messages = fetch_thread_messages(app, thread, limit).await;
    truncate(&messages.iter().map(transcript_line).collect::<Vec<_>>().join("\n"), 300_000)
}

async fn collect_applicant_thread_images(app: &App, thread: ChannelId, applicant: &str, limit: usize) -> Vec<Attachment> {
    let messages = fetch_thread_messages(app, thread, limit).await;
    let mut seen = std::collections::HashSet::new();
    let mut images = Vec::new();
    for m in messages.iter().filter(|m| m.author.id.to_string() == applicant) {
        let mut candidates: Vec<Attachment> = extract_image_attachments(m);
        for (i, e) in m.embeds.iter().enumerate() {
            for url in [e.image.as_ref().map(|x| x.url.clone()), e.thumbnail.as_ref().map(|x| x.url.clone())].into_iter().flatten() {
                candidates.push(Attachment { id: format!("{}-embed-{i}", m.id), name: "embedded-image".into(), url, ..Default::default() });
            }
        }
        for mut c in candidates {
            if c.url.is_empty() || !seen.insert(c.url.clone()) {
                continue;
            }
            c.message_id = m.id.to_string();
            c.author_id = m.author.id.to_string();
            c.created_at = m.timestamp.to_string();
            images.push(c);
        }
    }
    images.truncate(50);
    images
}

fn embed_field_value(value: &str, fallback: &str, max: usize) -> String {
    let t = value.split_whitespace().collect::<Vec<_>>().join(" ");
    truncate(if t.is_empty() { fallback } else { &t }, max)
}

fn format_event_scores(analysis: &LicenseAnalysis) -> String {
    let lines: Vec<String> = analysis
        .event_scores
        .iter()
        .take(8)
        .map(|e| {
            let name = embed_field_value(&e.event_name, "Team Event", 120);
            let details = [
                if e.rank.is_empty() { String::new() } else { format!("rank {}", e.rank) },
                if e.event_points.is_empty() { String::new() } else { format!("points {}", e.event_points) },
                if e.score.is_empty() { String::new() } else { format!("score {}", e.score) },
            ]
            .into_iter()
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(", ");
            if details.is_empty() {
                format!("**{name}** - score not detected")
            } else {
                format!("**{name}** - {details}")
            }
        })
        .collect();
    if lines.is_empty() {
        "Not detected".into()
    } else {
        truncate(&lines.join("\n"), 1024)
    }
}

/// The closing log. `stats` is `None` while the one background reading is still running.
fn build_log_embed(config: &DashboardConfig, ticket: &Ticket, licence_image: Option<&Attachment>, accepted_team: &str, stats: Option<&LicenseAnalysis>) -> CreateEmbed {
    let mut embed = CreateEmbed::new()
        .title("Recruitment Closing Log")
        .colour(embed_color(&config.recruitment.panel_color))
        .field("Discord user ID", embed_field_value(&ticket.applicant_id, "Unknown", 1024), true)
        .field("Applicant", if ticket.applicant_id.is_empty() { "Unknown".to_string() } else { format!("<@{}>", ticket.applicant_id) }, true)
        .field("Team joined", embed_field_value(if accepted_team.is_empty() { "Rejected" } else { accepted_team }, "Rejected", 1024), true);
    match stats {
        None => embed = embed.field("Screenshot stats", "\u{23f3} Reading the screenshots...", false),
        Some(a) => {
            let previous = if a.previous_team.is_empty() { a.source_team.clone() } else { a.previous_team.clone() };
            embed = embed
                .field("In-game name", embed_field_value(&a.in_game_name, "Not detected", 1024), true)
                .field("Previous team", embed_field_value(&previous, "Not detected", 1024), true)
                .field("Garage power", embed_field_value(&a.garage_power, "Not detected", 1024), true)
                .field("Team event scores", format_event_scores(a), false);
            if !a.error.is_empty() {
                embed = embed.field("Reading note", truncate(&a.error, 1024), false);
            }
        }
    }
    embed = embed
        .field("Screenshots", format!("{} licence, {} team event", ticket.license_attachments.len(), ticket.event_attachments.len()), true)
        .field("Closed by", if ticket.closed_by_id.is_empty() { "Unknown".to_string() } else { format!("<@{}>", ticket.closed_by_id) }, true)
        .timestamp(Timestamp::now());
    if let Some(img) = licence_image {
        embed = embed.image(img.url.clone());
    }
    embed
}

/// Post the closing log at once, then read the screenshots once in the background and complete the embed (and the
/// stored ticket / log entry) with the stats. This is the only OCR a recruitment ever needs.
async fn send_recruitment_log(app: Arc<App>, config: DashboardConfig, ticket: Ticket, applicant_images: Vec<Attachment>, accepted_team: String, log_id: String) {
    let target = if config.recruitment.log_channel_id.is_empty() { config.logging.channel_id.clone() } else { config.recruitment.log_channel_id.clone() };
    let Some(channel) = channel_id(&target) else { return };
    let licence_image = fresh_attachments(&app, &ticket.license_attachments).await.into_iter().find(|a| !a.url.is_empty()).or_else(|| applicant_images.iter().find(|a| !a.url.is_empty()).cloned());
    let sent = channel
        .send_message(&app.http, CreateMessage::new().embed(build_log_embed(&config, &ticket, licence_image.as_ref(), &accepted_team, None)).allowed_mentions(CreateAllowedMentions::new()))
        .await;
    let message = sent.ok();
    tokio::spawn(async move {
        let mut for_reading = ticket.clone();
        for_reading.team = accepted_team.clone();
        let analysis = read_ticket_stats(&app, &for_reading, &config).await;
        if let Some(m) = message {
            let embed = build_log_embed(&config, &ticket, licence_image.as_ref(), &accepted_team, Some(&analysis));
            let _ = channel.edit_message(&app.http, m.id, EditMessage::new().embed(embed)).await;
        }
        let stored = analysis.clone();
        let _ = update_ticket(&app.store, &ticket.thread_id, |t| t.license_analysis = Some(stored.clone())).await;
        if !log_id.is_empty() {
            let value = serde_json::to_value(&analysis).unwrap_or_default();
            let _ = update_recruitment_log(&app.store, &log_id, |entry| {
                entry["licenseAnalysis"] = value.clone();
            })
            .await;
        }
    });
}

/// Close a ticket with its final outcome (accepted into a team, or rejected).
pub async fn finish_close(app: &App, r: &Responder, outcome_id: &str) {
    if matches!(r.ix, crate::responder::Ix::Comp(_)) && !r.is_deferred() && !r.is_replied() {
        let _ = r.defer_update().await;
    }
    if !require_recruiter(app, r).await {
        return;
    }
    let config = app.config().await;
    let Some(outcome) = outcome_from_id(outcome_id, &config) else {
        let _ = r.ephemeral("Unknown recruitment outcome.").await;
        return;
    };
    let Some((thread, ticket)) = get_ticket_context(app, r).await else { return };
    if ticket.status == "closed" {
        let _ = r.ephemeral("This ticket is already closed.").await;
        return;
    }
    let accepted = !outcome.team.is_empty();
    let closed_at = now_iso();
    let transcript_text = if config.recruitment.transcript_on_close { create_transcript(app, thread.id, 250).await } else { String::new() };
    let transcript_lines = transcript_text.split('\n').filter(|l| !l.is_empty()).count() as u64;

    let mut applicant_images: Vec<Attachment> = ticket.license_attachments.iter().chain(ticket.event_attachments.iter()).cloned().collect();
    applicant_images.extend(collect_applicant_thread_images(app, thread.id, &ticket.applicant_id, 250).await);
    let mut seen = std::collections::HashSet::new();
    applicant_images.retain(|i| !i.url.is_empty() && seen.insert(i.url.clone()));

    let tag = display_tag(r.user());
    let uid = r.user().id.to_string();
    let (a2, tr) = (applicant_images.clone(), transcript_text.clone());
    let team_value = outcome.team.clone();
    let updated = update_ticket(&app.store, &thread.id.to_string(), |t| {
        t.status = "closed".into();
        t.closed_at = closed_at.clone();
        t.closed_by_id = uid.clone();
        t.closed_by_tag = tag.clone();
        t.archived_at = closed_at.clone();
        t.archived_by_id = uid.clone();
        t.archived_by_tag = tag.clone();
        t.outcome = if accepted { "accepted".into() } else { "rejected".into() };
        t.team = team_value.clone();
        t.transcript_saved = !tr.is_empty();
        t.transcript = if tr.is_empty() { None } else { Some(Transcript { text: tr.clone(), created_at: closed_at.clone(), line_count: transcript_lines, source: String::new() }) };
        t.transcript_preview = truncate(&tr, 10_000);
        t.applicant_thread_images = a2.clone();
    })
    .await
    .ok()
    .flatten();

    let mut recruitment_log_id = String::new();
    if accepted {
        let entry = json!({
            "guildId": ticket.guild_id,
            "channelId": ticket.channel_id,
            "threadId": ticket.thread_id,
            "applicantId": ticket.applicant_id,
            "applicantTag": ticket.applicant_tag,
            "closedById": r.user().id.to_string(),
            "closedByTag": display_tag(r.user()),
            "outcome": "accepted",
            "team": outcome.team,
            "closedAt": closed_at,
            "createdAt": ticket.created_at,
            "licenseAttachments": ticket.license_attachments,
            "eventAttachments": ticket.event_attachments,
            "applicantThreadImages": applicant_images,
            "transcriptSaved": !transcript_text.is_empty(),
            "transcriptLineCount": transcript_lines,
            "transcriptPreview": truncate(&transcript_text, 10_000),
        });
        if let Ok(saved) = append_recruitment_log(&app.store, entry).await {
            recruitment_log_id = saved.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
        }
        let t = updated.clone().unwrap_or(ticket.clone());
        tokio::spawn(send_recruitment_log(app.arc(), config.clone(), t, applicant_images.clone(), outcome.team.clone(), recruitment_log_id.clone()));
    }

    log_action(
        app,
        LogEntry::new(
            "ticket",
            if accepted { "Recruitment Ticket Accepted" } else { "Recruitment Ticket Rejected" },
            if accepted { format!("<@{}> was recruited to **{}**.", ticket.applicant_id, outcome.team) } else { format!("<@{}> was rejected.", ticket.applicant_id) },
        )
        .guild(&ticket.guild_id)
        .actor(r.user().id, display_tag(r.user()))
        .target(&ticket.applicant_id, &ticket.applicant_tag)
        .meta(json!({ "threadId": thread.id.to_string(), "outcome": if accepted { "accepted" } else { "rejected" }, "team": outcome.team, "recruitmentLogId": recruitment_log_id })),
    )
    .await;

    if accepted && config.member_counts.update_on_recruitment_close {
        increment_team_count(app, &outcome.team, 1, Some(r.user())).await;
    }
    if accepted {
        if let Some(applicant) = user_id(&ticket.applicant_id) {
            if let Err(error) = queue_team_role_assignment(app, applicant, &outcome.team, &ticket.thread_id, Some(r.user())).await {
                tracing::debug!("team role assignment not queued: {error}");
            }
        }
    }

    let _ =
        thread.id.say(&app.http, format!("Ticket closed by <@{}>. Applicant was {}.", r.user().id, if accepted { format!("recruited to **{}**", outcome.team) } else { "rejected".to_string() })).await;

    let done = format!("Ticket closed. Outcome: {}.", if accepted { outcome.team.clone() } else { "Rejected".into() });
    if matches!(r.ix, crate::responder::Ix::Comp(_)) && !r.is_deferred() && !r.is_replied() {
        let _ = r.update(ReplyData::text(done).components(vec![])).await;
    } else if r.is_deferred() && !r.is_replied() {
        let _ = r.edit_reply(ReplyData::text(done).components(vec![])).await;
    } else {
        let _ = r.follow_up(ReplyData::text(done).ephemeral()).await;
    }
    let _ = outcome.id;
    let _ = thread.id.edit_thread(&app.http, EditThread::new().locked(true).archived(true)).await;
}

// ------------------------------------------------------------------------------------------ invite

fn render_invite_message(template: &str, applicant_id: &str, invite_url: &str, server: &str) -> String {
    let mention = format!("<@{applicant_id}>");
    let mut content = (if template.is_empty() { "{user} Join **{server}** here: {invite}" } else { template })
        .replace("{user}", &mention)
        .replace("{invite}", invite_url)
        .replace("{server}", if server.is_empty() { "the server" } else { server });
    if !content.contains(&mention) {
        content = format!("{mention} {content}");
    }
    if !content.contains(invite_url) {
        content = format!("{content}\n{invite_url}");
    }
    truncate(&content, 2000)
}

pub async fn send_invite_to_applicant(app: &App, r: &Responder) {
    if !require_recruiter(app, r).await {
        return;
    }
    let Some((thread, ticket)) = get_ticket_context(app, r).await else { return };
    if matches!(ticket.status.as_str(), "closed" | "archived" | "deleted") {
        let _ = r.ephemeral("This ticket is already closed.").await;
        return;
    }
    let config = app.config().await;
    let Some(invite_channel) = channel_id(&config.recruitment.invite_channel_id) else {
        let _ = r.ephemeral("Configure the destination invite channel in the dashboard first.").await;
        return;
    };
    let Ok(info) = invite_channel.to_channel(&app.http).await else {
        let _ = r.ephemeral("I could not access an invite-capable channel for the destination server.").await;
        return;
    };
    let Some(gc) = info.guild() else {
        let _ = r.ephemeral("I could not access an invite-capable channel for the destination server.").await;
        return;
    };
    if !config.recruitment.invite_guild_id.is_empty() && gc.guild_id.to_string() != config.recruitment.invite_guild_id {
        let _ = r.ephemeral("The configured invite channel does not belong to the configured destination server.").await;
        return;
    }
    let invite = match gc.id.create_invite(&app.http, CreateInvite::new().max_age(0).max_uses(1).unique(true)).await {
        Ok(i) => i,
        Err(error) => {
            let _ = r.ephemeral(format!("I could not create the invite: {error}")).await;
            return;
        }
    };
    let invite_url = format!("https://discord.gg/{}", invite.code);
    let server_name = app.http.get_guild(gc.guild_id).await.map(|g| g.name).unwrap_or_else(|_| "the server".into());
    let content = render_invite_message(&config.recruitment.invite_message, &ticket.applicant_id, &invite_url, &server_name);
    let mentions = user_id(&ticket.applicant_id).map(|u| CreateAllowedMentions::new().users(vec![u])).unwrap_or_default();
    let _ = thread.id.send_message(&app.http, CreateMessage::new().content(content).allowed_mentions(mentions)).await;

    let (uid, tag, gid, cid) = (r.user().id.to_string(), display_tag(r.user()), gc.guild_id.to_string(), gc.id.to_string());
    let _ = update_ticket(&app.store, &thread.id.to_string(), |t| {
        t.last_invite_at = now_iso();
        t.last_invite_by_id = uid.clone();
        t.last_invite_by_tag = tag.clone();
        t.last_invite_guild_id = gid.clone();
        t.last_invite_channel_id = cid.clone();
    })
    .await;
    log_action(
        app,
        LogEntry::new("ticket", "Recruitment Invite Sent", format!("<@{}> sent a single-use invite to <@{}>.", r.user().id, ticket.applicant_id))
            .guild(&ticket.guild_id)
            .actor(r.user().id, display_tag(r.user()))
            .target(&ticket.applicant_id, &ticket.applicant_tag)
            .meta(json!({ "threadId": thread.id.to_string(), "inviteGuildId": gc.guild_id.to_string(), "inviteChannelId": gc.id.to_string() })),
    )
    .await;
    let _ = r.ephemeral(format!("Sent a single-use invite to {}.", if ticket.applicant_tag.is_empty() { &ticket.applicant_id } else { &ticket.applicant_tag })).await;
}

// ----------------------------------------------------------------------- ticket management commands

async fn add_users_to_ticket(app: &App, r: &Responder, users: Vec<User>) {
    if !require_recruiter(app, r).await {
        return;
    }
    let Some((thread, ticket)) = get_ticket_context(app, r).await else { return };
    let mut unique: Vec<User> = Vec::new();
    for u in users {
        if !unique.iter().any(|x| x.id == u.id) {
            unique.push(u);
        }
    }
    unique.truncate(25);
    if unique.is_empty() {
        let _ = r.ephemeral("No valid users were provided.").await;
        return;
    }
    let (mut added, mut failed) = (Vec::new(), Vec::new());
    for u in &unique {
        match thread.id.add_thread_member(&app.http, u.id).await {
            Ok(()) => added.push(u.id.to_string()),
            Err(e) => failed.push(format!("{} ({e})", display_tag(u))),
        }
    }
    let mut next: Vec<String> = ticket.added_user_ids.clone();
    for a in &added {
        if !next.contains(a) {
            next.push(a.clone());
        }
    }
    let _ = update_ticket(&app.store, &thread.id.to_string(), |t| t.added_user_ids = next.clone()).await;
    if !added.is_empty() {
        let _ = thread.id.say(&app.http, format!("Added {} to the ticket.", added.iter().map(|id| format!("<@{id}>")).collect::<Vec<_>>().join(", "))).await;
        log_action(
            app,
            LogEntry::new("ticket", "Users Added To Recruitment Ticket", format!("<@{}> added {} user(s) to a ticket.", r.user().id, added.len()))
                .guild(&ticket.guild_id)
                .actor(r.user().id, display_tag(r.user()))
                .target(&ticket.applicant_id, &ticket.applicant_tag)
                .meta(json!({ "threadId": thread.id.to_string(), "addedUserIds": added })),
        )
        .await;
    }
    let _ = r
        .ephemeral(
            [
                if added.is_empty() { "No users were added.".to_string() } else { format!("Added: {}", added.iter().map(|id| format!("<@{id}>")).collect::<Vec<_>>().join(", ")) },
                if failed.is_empty() { String::new() } else { format!("Failed: {}", failed.join("; ")) },
            ]
            .into_iter()
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join("\n"),
        )
        .await;
}

pub async fn add_user_to_ticket(app: &App, r: &Responder, user: User) {
    add_users_to_ticket(app, r, vec![user]).await;
}

pub async fn mass_add_users_to_ticket(app: &App, r: &Responder, raw: &str) {
    let mut users = Vec::new();
    for id in all_ids(raw).into_iter().take(25) {
        if let Ok(u) = app.http.get_user(UserId::new(id)).await {
            users.push(u);
        }
    }
    add_users_to_ticket(app, r, users).await;
}

pub async fn remove_user_from_ticket(app: &App, r: &Responder, user: User) {
    if !require_recruiter(app, r).await {
        return;
    }
    let Some((thread, ticket)) = get_ticket_context(app, r).await else { return };
    let _ = thread.id.remove_thread_member(&app.http, user.id).await;
    let uid = user.id.to_string();
    let _ = update_ticket(&app.store, &thread.id.to_string(), |t| t.added_user_ids.retain(|id| *id != uid)).await;
    let _ = thread.id.say(&app.http, format!("Removed <@{}> from the ticket.", user.id)).await;
    log_action(
        app,
        LogEntry::new("ticket", "User Removed From Recruitment Ticket", format!("<@{}> removed <@{}> from a ticket.", r.user().id, user.id))
            .guild(&ticket.guild_id)
            .actor(r.user().id, display_tag(r.user()))
            .target(user.id, display_tag(&user))
            .meta(json!({ "threadId": thread.id.to_string(), "applicantId": ticket.applicant_id })),
    )
    .await;
    let _ = r.ephemeral(format!("Removed <@{}> from this ticket.", user.id)).await;
}

pub async fn rename_ticket(app: &App, r: &Responder, raw: &str) {
    if !require_recruiter(app, r).await {
        return;
    }
    let Some((thread, ticket)) = get_ticket_context(app, r).await else { return };
    let name = safe_ticket_name(raw);
    if name.is_empty() {
        let _ = r.ephemeral("Please provide a valid thread name.").await;
        return;
    }
    if let Err(e) = thread.id.edit_thread(&app.http, EditThread::new().name(name.clone())).await {
        let _ = r.ephemeral(format!("Could not rename the thread: {e}")).await;
        return;
    }
    let uid = r.user().id.to_string();
    let _ = update_ticket(&app.store, &thread.id.to_string(), |t| {
        t.renamed_at = now_iso();
        t.renamed_by_id = uid.clone();
    })
    .await;
    let _ = thread.id.say(&app.http, format!("Ticket renamed to **{name}** by <@{}>.", r.user().id)).await;
    log_action(
        app,
        LogEntry::new("ticket", "Recruitment Ticket Renamed", format!("<@{}> renamed a ticket to **{name}**.", r.user().id))
            .guild(&ticket.guild_id)
            .actor(r.user().id, display_tag(r.user()))
            .target(&ticket.applicant_id, &ticket.applicant_tag)
            .meta(json!({ "threadId": thread.id.to_string(), "name": name })),
    )
    .await;
    let _ = r.ephemeral(format!("Renamed this ticket to **{name}**.")).await;
}

pub async fn delete_ticket(app: &App, r: &Responder) {
    if !require_recruiter(app, r).await {
        return;
    }
    let Some((thread, ticket)) = get_ticket_context(app, r).await else { return };
    let (uid, tag) = (r.user().id.to_string(), display_tag(r.user()));
    let _ = update_ticket(&app.store, &thread.id.to_string(), |t| {
        t.status = "deleted".into();
        t.deleted_at = now_iso();
        t.deleted_by_id = uid.clone();
        t.deleted_by_tag = tag.clone();
    })
    .await;
    log_action(
        app,
        LogEntry::new("ticket", "Recruitment Ticket Deleted", format!("<@{}> deleted the ticket for <@{}>.", r.user().id, ticket.applicant_id))
            .guild(&ticket.guild_id)
            .actor(r.user().id, display_tag(r.user()))
            .target(&ticket.applicant_id, &ticket.applicant_tag)
            .meta(json!({ "threadId": thread.id.to_string() })),
    )
    .await;
    let _ = r.ephemeral("Deleting this ticket thread now.").await;
    if thread.id.delete(&app.http).await.is_err() {
        let _ = thread.id.edit_thread(&app.http, EditThread::new().locked(true).archived(true)).await;
    }
}

pub async fn archive_ticket(app: &App, r: &Responder) {
    if !require_recruiter(app, r).await {
        return;
    }
    let Some((thread, ticket)) = get_ticket_context(app, r).await else { return };
    let (uid, tag) = (r.user().id.to_string(), display_tag(r.user()));
    let keep_closed = ticket.status == "closed";
    let _ = update_ticket(&app.store, &thread.id.to_string(), |t| {
        t.status = if keep_closed { "closed".into() } else { "archived".into() };
        t.archived_at = now_iso();
        t.archived_by_id = uid.clone();
        t.archived_by_tag = tag.clone();
    })
    .await;
    log_action(
        app,
        LogEntry::new("ticket", "Recruitment Ticket Archived", format!("<@{}> archived the ticket for <@{}>.", r.user().id, ticket.applicant_id))
            .guild(&ticket.guild_id)
            .actor(r.user().id, display_tag(r.user()))
            .target(&ticket.applicant_id, &ticket.applicant_tag)
            .meta(json!({ "threadId": thread.id.to_string() })),
    )
    .await;
    let _ = r.ephemeral("Archiving this ticket thread now.").await;
    let _ = thread.id.edit_thread(&app.http, EditThread::new().locked(true).archived(true)).await;
}

// ------------------------------------------------------------------------------- screenshot editing

fn screenshot_list_text(ticket: &Ticket, shot: ShotType) -> String {
    let list = match shot {
        ShotType::License => &ticket.license_attachments,
        ShotType::Event => &ticket.event_attachments,
    };
    if list.is_empty() {
        return format!("No {} are saved for this ticket.", shot.plural());
    }
    truncate(
        &list
            .iter()
            .enumerate()
            .map(|(i, s)| {
                let label = if s.name.is_empty() { format!("Screenshot {}", i + 1) } else { s.name.clone() };
                let actor = if s.uploaded_by_id.is_empty() { String::new() } else { format!(" added by <@{}>", s.uploaded_by_id) };
                format!("{}. [{}]({}){}", i + 1, label, s.url, actor)
            })
            .collect::<Vec<_>>()
            .join("\n"),
        1900,
    )
}

pub async fn list_ticket_screenshots(app: &App, r: &Responder, kind: &str) {
    if !require_recruiter(app, r).await {
        return;
    }
    let Some((_, ticket)) = get_ticket_context(app, r).await else { return };
    let Some(shot) = ShotType::parse(kind) else {
        let _ = r.ephemeral("Unknown screenshot type.").await;
        return;
    };
    let _ = r.ephemeral(screenshot_list_text(&ticket, shot)).await;
}

fn refresh_applicant_images(ticket: &Ticket, next_license: &[Attachment], next_events: &[Attachment]) -> Vec<Attachment> {
    let previous: std::collections::HashSet<&str> = ticket.license_attachments.iter().chain(ticket.event_attachments.iter()).map(|a| a.url.as_str()).collect();
    let managed = clean_screenshot_list(next_license.iter().chain(next_events.iter()).cloned().collect());
    let retained: Vec<Attachment> = ticket.applicant_thread_images.iter().filter(|a| !a.url.is_empty() && !previous.contains(a.url.as_str())).cloned().collect();
    clean_screenshot_list(managed.into_iter().chain(retained).collect())
}

async fn update_ticket_screenshots(
    app: &App,
    r: &Responder,
    thread: GuildChannel,
    ticket: Ticket,
    shot: ShotType,
    action: &str,
    updater: impl FnOnce(Vec<Attachment>) -> Result<Vec<Attachment>, String>,
) {
    if ticket.status == "deleted" {
        let _ = r.ephemeral("This ticket was deleted.").await;
        return;
    }
    let current = clean_screenshot_list(match shot {
        ShotType::License => ticket.license_attachments.clone(),
        ShotType::Event => ticket.event_attachments.clone(),
    });
    let next = match updater(current) {
        Ok(n) => clean_screenshot_list(n),
        Err(e) => {
            let _ = r.ephemeral(e).await;
            return;
        }
    };
    let next_license = if shot == ShotType::License { next.clone() } else { clean_screenshot_list(ticket.license_attachments.clone()) };
    let next_events = if shot == ShotType::Event { next.clone() } else { clean_screenshot_list(ticket.event_attachments.clone()) };
    let applicant_images = refresh_applicant_images(&ticket, &next_license, &next_events);
    let (uid, tag) = (r.user().id.to_string(), display_tag(r.user()));
    let had_analysis = ticket.license_analysis.is_some();
    let (nl, ne) = (next_license.clone(), next_events.clone());
    let updated = update_ticket(&app.store, &thread.id.to_string(), |t| {
        t.license_attachments = nl.clone();
        t.event_attachments = ne.clone();
        t.applicant_thread_images = applicant_images.clone();
        t.screenshots_updated_at = now_iso();
        t.screenshots_updated_by_id = uid.clone();
        t.screenshots_updated_by_tag = tag.clone();
        if had_analysis {
            t.license_analysis = None;
            t.license_analysis_invalidated_at = now_iso();
        }
    })
    .await
    .ok()
    .flatten();
    let _ = thread.id.say(&app.http, format!("{} {} by <@{}>.", shot.plural(), action, r.user().id)).await;
    log_action(
        app,
        LogEntry::new("ticket", "Recruitment Ticket Screenshots Updated", format!("<@{}> {} {} for <@{}>.", r.user().id, action, shot.plural(), ticket.applicant_id))
            .guild(&ticket.guild_id)
            .actor(r.user().id, display_tag(r.user()))
            .target(&ticket.applicant_id, &ticket.applicant_tag)
            .meta(json!({ "threadId": thread.id.to_string(), "screenshotType": if shot == ShotType::License { "license" } else { "event" }, "screenshotCount": next.len() })),
    )
    .await;
    let current_ticket = updated.unwrap_or(ticket);
    let _ = r.ephemeral(format!("{} {}.\n\n{}", shot.plural(), action, screenshot_list_text(&current_ticket, shot))).await;
}

async fn normalize_ticket_screenshots(app: &App, r: &Responder, shot: ShotType, atts: Vec<&serenity::all::Attachment>, ticket: &Ticket) -> Result<Vec<Attachment>, String> {
    let normalized: Vec<Attachment> = atts.iter().map(|a| attachment_from(a)).collect();
    if normalized.is_empty() {
        return Err("Attach at least one image.".into());
    }
    for (a, orig) in normalized.iter().zip(atts.iter()) {
        if !is_image_attachment(orig) {
            return Err(format!("{} is not an image.", if a.name.is_empty() { "Attachment" } else { &a.name }));
        }
    }
    let config = app.config().await;
    let applicant = user_id(&ticket.applicant_id);
    let owner = match applicant {
        Some(u) => app.http.get_user(u).await.unwrap_or_else(|_| r.user().clone()),
        None => r.user().clone(),
    };
    let mirrored = mirror_attachments_to_dm(app, &config, &normalized, shot.label(), &owner, r.guild_id()).await?;
    let now = now_iso();
    Ok(mirrored
        .into_iter()
        .map(|mut a| {
            a.uploaded_by_id = r.user().id.to_string();
            a.uploaded_by_tag = display_tag(r.user());
            a.uploaded_at = now.clone();
            a.managed_type = if shot == ShotType::License { "license".into() } else { "event".into() };
            a
        })
        .collect())
}

pub async fn add_ticket_screenshots(app: &App, r: &Responder, kind: &str, atts: Vec<&serenity::all::Attachment>) {
    if !require_recruiter(app, r).await {
        return;
    }
    let Some((thread, ticket)) = get_ticket_context(app, r).await else { return };
    let Some(shot) = ShotType::parse(kind) else {
        let _ = r.ephemeral("Unknown screenshot type.").await;
        return;
    };
    let additions = match normalize_ticket_screenshots(app, r, shot, atts, &ticket).await {
        Ok(a) => a,
        Err(e) => {
            let _ = r.ephemeral(e).await;
            return;
        }
    };
    let action = if additions.len() == 1 { "added".to_string() } else { format!("added ({})", additions.len()) };
    update_ticket_screenshots(app, r, thread, ticket, shot, &action, |mut current| {
        current.extend(additions);
        Ok(current)
    })
    .await;
}

pub async fn remove_ticket_screenshot(app: &App, r: &Responder, kind: &str, index: i64) {
    if !require_recruiter(app, r).await {
        return;
    }
    let Some((thread, ticket)) = get_ticket_context(app, r).await else { return };
    let Some(shot) = ShotType::parse(kind) else {
        let _ = r.ephemeral("Unknown screenshot type.").await;
        return;
    };
    update_ticket_screenshots(app, r, thread, ticket, shot, "removed", |mut current| {
        let target = index - 1;
        if target < 0 || target as usize >= current.len() {
            return Err(format!("Pick an index between 1 and {}.", current.len().max(1)));
        }
        current.remove(target as usize);
        Ok(current)
    })
    .await;
}

pub async fn change_ticket_screenshot(app: &App, r: &Responder, kind: &str, index: i64, attachment: &serenity::all::Attachment) {
    if !require_recruiter(app, r).await {
        return;
    }
    let Some((thread, ticket)) = get_ticket_context(app, r).await else { return };
    let Some(shot) = ShotType::parse(kind) else {
        let _ = r.ephemeral("Unknown screenshot type.").await;
        return;
    };
    let replacement = match normalize_ticket_screenshots(app, r, shot, vec![attachment], &ticket).await {
        Ok(mut a) => a.remove(0),
        Err(e) => {
            let _ = r.ephemeral(e).await;
            return;
        }
    };
    update_ticket_screenshots(app, r, thread, ticket, shot, "changed", |mut current| {
        let target = index - 1;
        if target < 0 || target as usize >= current.len() {
            return Err(format!("Pick an index between 1 and {}.", current.len().max(1)));
        }
        current[target as usize] = replacement;
        Ok(current)
    })
    .await;
}

// --------------------------------------------------------------------------------- interactions

/// Entry point for `recruitment:*` buttons. Returns true when the interaction was handled.
pub async fn handle_component(app: Arc<App>, r: Responder, custom_id: String) -> bool {
    if !custom_id.starts_with("recruitment:") {
        return false;
    }
    let config = app.config().await;
    if !config.recruitment.enabled && custom_id == APPLY_BUTTON_ID {
        let _ = r.reply(ReplyData::text("Recruitment is currently closed.").ephemeral()).await;
        return true;
    }
    if custom_id == APPLY_BUTTON_ID {
        // The upload wait can last 10 minutes: run it detached so the gateway handler is not held up.
        tokio::spawn(collect_license(app, r));
    } else if custom_id.starts_with(EVENT_YES_PREFIX) {
        tokio::spawn(collect_event_screenshots(app, r, custom_id));
    } else if custom_id.starts_with(EVENT_NO_PREFIX) {
        tokio::spawn(complete_without_events(app, r, custom_id));
    } else if custom_id == CLAIM_ID {
        claim_ticket(&app, &r).await;
    } else if custom_id == CLOSE_ID {
        start_close(&app, &r).await;
    } else if custom_id.starts_with(TUTORIAL_PREFIX) {
        let _ = r.ephemeral("Manual tutorial buttons have been removed. Screenshot guidance is sent automatically when an uploaded image fails validation.").await;
    } else if let Some(id) = custom_id.strip_prefix(CLOSE_TEAM_PREFIX) {
        finish_close(&app, &r, id).await;
    }
    true
}
