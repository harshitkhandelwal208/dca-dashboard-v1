//! Persisted record types. Field names/shape match the original Node bot's JSON so old data keeps loading
//! (unknown fields are preserved through `extra`).

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

// ------------------------------------------------------------------------------------------ tickets

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Attachment {
    pub id: String,
    pub name: String,
    pub url: String,
    pub proxy_url: String,
    pub content_type: String,
    pub size: u64,
    pub source: String,
    pub dm_user_id: String,
    pub dm_message_id: String,
    pub original_url: String,
    pub uploaded_by_id: String,
    pub uploaded_by_tag: String,
    pub uploaded_at: String,
    pub managed_type: String,
    pub message_id: String,
    pub author_id: String,
    pub created_at: String,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Transcript {
    pub text: String,
    pub created_at: String,
    pub line_count: u64,
    pub source: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct EventScore {
    pub event_name: String,
    pub rank: String,
    pub event_points: String,
    pub score: String,
    pub raw_text: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct LicenseAnalysis {
    pub discord_id: String,
    pub in_game_name: String,
    pub source_team: String,
    pub previous_team: String,
    pub accepted_team: String,
    pub garage_power: String,
    pub cup_points: String,
    pub season_points: String,
    pub adventurer_rank: String,
    pub event_scores: Vec<EventScore>,
    pub raw_text: String,
    pub error: String,
    pub confidence: f64,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Ticket {
    pub thread_id: String,
    pub thread_name: String,
    pub channel_id: String,
    pub guild_id: String,
    pub applicant_id: String,
    pub applicant_tag: String,
    pub applicant_username: String,
    pub status: String,
    pub claimed_by_id: String,
    pub claimed_by_tag: String,
    pub added_user_ids: Vec<String>,
    pub license_attachments: Vec<Attachment>,
    pub event_attachments: Vec<Attachment>,
    pub created_at: String,
    pub updated_at: String,
    pub closed_at: String,
    pub closed_by_id: String,
    pub closed_by_tag: String,
    pub archived_at: String,
    pub archived_by_id: String,
    pub archived_by_tag: String,
    pub deleted_at: String,
    pub deleted_by_id: String,
    pub deleted_by_tag: String,
    pub outcome: String,
    pub team: String,
    pub transcript_saved: bool,
    pub transcript: Option<Transcript>,
    pub transcript_preview: String,
    pub applicant_thread_images: Vec<Attachment>,
    pub license_analysis: Option<LicenseAnalysis>,
    pub screenshots_updated_at: String,
    pub screenshots_updated_by_id: String,
    pub screenshots_updated_by_tag: String,
    pub license_analysis_invalidated_at: String,
    pub renamed_at: String,
    pub renamed_by_id: String,
    pub last_invite_at: String,
    pub last_invite_by_id: String,
    pub last_invite_by_tag: String,
    pub last_invite_guild_id: String,
    pub last_invite_channel_id: String,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct RecruitmentBan {
    pub user_id: String,
    pub user_tag: String,
    pub reason: String,
    pub banned_by_id: String,
    pub banned_by_tag: String,
    pub guild_id: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct BotLog {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub title: String,
    pub message: String,
    pub guild_id: String,
    pub actor_id: String,
    pub actor_tag: String,
    pub target_id: String,
    pub target_tag: String,
    pub metadata: Value,
    pub created_at: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Warning {
    pub id: String,
    pub user_id: String,
    pub guild_id: String,
    pub reason: String,
    pub created_at: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Reminder {
    pub id: u64,
    pub user_id: String,
    pub channel_id: String,
    pub guild_id: String,
    pub text: String,
    pub due_at: i64,
    pub created_at: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TeamRoleAssignment {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub status: String,
    pub user_id: String,
    pub team_id: String,
    pub team_name: String,
    pub role_id: String,
    pub guild_id: String,
    pub requires_role_id: String,
    pub ticket_id: String,
    pub attempts: u32,
    pub created_at: String,
    pub not_before_at: String,
    pub run_at: String,
    pub last_attempt_at: String,
    pub last_reason: String,
    pub completed_at: String,
    pub failed_reason: String,
}

// ------------------------------------------------------------------------------------- spreadsheets

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Correction {
    pub row: i64,
    pub field: String,
    pub value: String,
    pub actor_id: String,
    pub actor_tag: String,
    pub created_at: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Player {
    pub rank: u32,
    pub placement: u32,
    pub player_name: String,
    pub team_label: String,
    pub team_type: String,
    pub team_color: String,
    pub row_color: String,
    pub row_color_confidence: Option<f64>,
    pub row_color_source: String,
    pub points: Option<i64>,
    pub points_source: String,
    pub score: Option<i64>,
    pub source_image: String,
    pub raw_line: String,
    pub classification_reason: String,
    pub classification_source: String,
    pub confidence: Option<f64>,
    pub flagged: bool,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Bucket {
    pub id: String,
    pub label: String,
    pub min: u32,
    pub max: u32,
    pub own: u32,
    pub opponents: u32,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct OpponentsBelow {
    pub player_name: String,
    pub rank: u32,
    pub opponents_below: u32,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Stats {
    pub total_players: u32,
    pub own_players: u32,
    pub opponents: u32,
    pub own_average_rank: Option<f64>,
    pub opponent_average_rank: Option<f64>,
    pub own_points: i64,
    pub opponent_points: i64,
    pub own_score: i64,
    pub opponent_score: i64,
    pub podium: Vec<Player>,
    pub own_top10: u32,
    pub opponent_top10: u32,
    pub top_opponent_rank: Option<u32>,
    pub kab_players: Vec<String>,
    pub kab_count: u32,
    pub buckets: Vec<Bucket>,
    pub opponents_below_by_player: Vec<OpponentsBelow>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TeamScores {
    pub own: Option<f64>,
    pub opponent: Option<f64>,
    pub raw_line: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Metadata {
    pub title: String,
    pub event_name: String,
    pub game: String,
    pub own_team_name: String,
    pub opponent_team_name: String,
    pub screenshot_types: Vec<String>,
    pub teams: Vec<Value>,
    pub team_scores: Option<TeamScores>,
    pub winner: String,
    pub notes: String,
    pub extraction_type: String,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Attendance {
    pub roster: Vec<String>,
    pub attended_players: Vec<String>,
    pub missing_players: Vec<String>,
    pub zero_score_policy: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Outputs {
    pub fods_path: String,
    pub spreadsheet_path: String,
    pub spreadsheet_image_path: String,
    pub chart_path: String,
    pub conversion_error: String,
    pub generated_at: String,
    pub fods_url: String,
    pub spreadsheet_url: String,
    pub spreadsheet_image_url: String,
    pub chart_url: String,
    pub file_path: String,
    pub file_url: String,
    pub table_image_path: String,
    pub table_image_url: String,
}

/// One OCR'd standings row of a screenshot, kept so a session can be rebuilt (and corrected) without reading again.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PageRow {
    pub rank: u32,
    pub rank_read: bool,
    pub name: String,
    pub name_confidence: f32,
    pub name_flagged: bool,
    pub score: Option<u64>,
    pub band: u8,
    pub band_confidence: f32,
    pub gold_text: bool,
    pub cy: f32,
    pub raw: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PageHeader {
    pub title: String,
    pub left_team: String,
    pub right_team: String,
    pub left_score: Option<u64>,
    pub right_score: Option<u64>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PageReading {
    pub image: String,
    pub method: String,
    pub rows: Vec<PageRow>,
    pub header: PageHeader,
    pub raw_text: String,
    pub error: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SpreadsheetSession {
    pub id: String,
    pub team_id: String,
    pub team_name: String,
    pub guild_id: String,
    pub channel_id: String,
    pub author_id: String,
    pub author_tag: String,
    pub message_ids: Vec<String>,
    pub images: Vec<Attachment>,
    pub corrections: Vec<Correction>,
    pub status: String,
    pub created_at: String,
    pub updated_at: String,
    pub last_image_at: String,
    pub processed_at: String,
    pub error: String,
    pub metadata: Metadata,
    pub team_event_name: String,
    pub players: Vec<Player>,
    pub stats: Stats,
    pub raw_ocr_text: String,
    /// Local OCR readings per screenshot (replaces the Gemini `ocrResults`).
    pub readings: Vec<PageReading>,
    pub outputs: Outputs,
    pub attendance: Attendance,
    pub raw_data_cleaned_at: String,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ReportEmission {
    pub team_id: String,
    pub period: String,
    pub period_key: String,
    pub emitted_at: String,
    pub file_path: String,
    pub attachment_urls: Vec<String>,
    pub event_count: u32,
    pub reason: String,
    pub channel_id: String,
    pub message_id: String,
}
