//! Dashboard-managed configuration (`dashboardConfig` scope).
//!
//! This is a faithful port of the Node `dashboardConfig.js`: same defaults, same normalisation rules, same JSON
//! shape (camelCase), so the dashboard front-end and any config already stored in Firebase keep working.
//! The Gemini related settings were dropped together with Gemini - all reading is done locally now.

use crate::state_store::StateStore;
use crate::util::{is_snowflake, now_iso};
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::OnceLock;

pub const CONFIG_SCOPE: &str = "dashboardConfig";
pub const CONFIG_VERSION: u32 = 11;

pub const DEFAULT_RECRUITMENT_QUESTIONS: &str = "Which team do you want to join?\nDo you have other accounts, if yes, in which team?\nWere you ever flagged/banned, had double VIP?\nDo you want to stay long-term or just visit?\nHow many km's do you usually drive per week?";

pub fn recruitment_teams() -> Vec<String> {
    vec!["Discord".into(), "Discord\u{b2}".into(), "Discord 3\u{2122}".into(), "Nascar DC".into()]
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct BotConfig {
    pub guild_id: String,
    pub community_guild_id: String,
    pub recruitment_guild_id: String,
    pub dashboard_allowed_role_id: String,
    pub recruiter_role_id: String,
    pub manager_role_id: String,
    pub locale: String,
    pub command_log_channel_id: String,
    pub dashboard_url: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct LoggingEvents {
    pub tickets: bool,
    pub member_counts: bool,
    pub youtube: bool,
    pub reaction_roles: bool,
    pub system: bool,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct LoggingConfig {
    pub enabled: bool,
    pub channel_id: String,
    pub events: LoggingEvents,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct MessageSection {
    pub enabled: bool,
    pub channel_id: String,
    pub message: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Tutorial {
    pub id: String,
    pub label: String,
    pub description: String,
    pub video_url: String,
    pub guide_image_url: String,
    pub image_kind: String,
    pub enabled: bool,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct RecruitmentConfig {
    pub enabled: bool,
    pub panel_channel_id: String,
    pub panel_message_id: String,
    pub panel_title: String,
    pub panel_description: String,
    pub panel_color: String,
    pub log_channel_id: String,
    pub recruiter_alert_channel_id: String,
    pub tutorial_upload_channel_id: String,
    pub screenshot_dm_user_id: String,
    pub recruiter_role_id: String,
    pub invite_guild_id: String,
    pub invite_channel_id: String,
    pub invite_message: String,
    pub community_rules_role_id: String,
    pub ban_list_channel_id: String,
    pub ban_list_message_ids: Vec<String>,
    pub private_threads: bool,
    pub thread_auto_archive_minutes: u32,
    pub max_open_tickets_per_user: u32,
    pub transcript_on_close: bool,
    pub delete_on_close: bool,
    pub questions_intro: String,
    pub questions: String,
    pub teams: Vec<String>,
    pub tutorials: Vec<Tutorial>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct MemberTeam {
    pub id: String,
    pub name: String,
    pub division: String,
    pub players: u32,
    pub recruitment_status: String,
    pub recruitment_role_id: String,
    pub recruitment_role_auto_assign_enabled: bool,
    pub recruitment_role_delay_minutes: u32,
    pub community_role_id: String,
    pub community_role_auto_assign_enabled: bool,
    pub community_role_delay_minutes: u32,
    pub role_id: String,
    pub auto_assign_enabled: bool,
    pub auto_assign_delay_minutes: u32,
    pub aliases: Vec<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct MemberCountsConfig {
    pub enabled: bool,
    pub channel_id: String,
    pub message_id: String,
    pub title: String,
    pub update_on_recruitment_close: bool,
    pub teams: Vec<MemberTeam>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SpreadsheetTeam {
    pub id: String,
    pub name: String,
    pub enabled: bool,
    pub monitored_channel_id: String,
    pub output_channel_id: String,
    pub access_role_id: String,
    pub own_team_aliases: Vec<String>,
    pub own_player_aliases: Vec<String>,
    pub auto_process: bool,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SpreadsheetsConfig {
    pub enabled: bool,
    pub session_window_minutes: u32,
    pub output_format: String,
    pub libre_office_path: String,
    pub raw_data_retention_days: u32,
    pub image_retention_days: u32,
    pub teams: Vec<SpreadsheetTeam>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct YoutubeFeed {
    pub id: String,
    pub name: String,
    pub channel_id: String,
    pub enabled: bool,
    pub last_video_id: String,
    pub last_published_at: String,
    pub last_checked_at: String,
    pub sent_video_ids: Vec<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct YoutubeConfig {
    pub enabled: bool,
    pub check_interval_minutes: u32,
    pub max_announcement_age_hours: u32,
    pub default_channel_id: String,
    pub announcement_template: String,
    pub feeds: Vec<YoutubeFeed>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ReactionOption {
    pub emoji: String,
    pub role_id: String,
    pub label: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ReactionRoleGroup {
    pub id: String,
    pub name: String,
    pub enabled: bool,
    pub channel_id: String,
    pub message_id: String,
    pub message: String,
    pub options: Vec<ReactionOption>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct DashboardConfig {
    pub version: u32,
    pub updated_at: String,
    pub bot: BotConfig,
    pub logging: LoggingConfig,
    pub welcome: MessageSection,
    pub leave: MessageSection,
    pub recruitment: RecruitmentConfig,
    pub member_counts: MemberCountsConfig,
    pub spreadsheets: SpreadsheetsConfig,
    pub youtube: YoutubeConfig,
    pub reaction_roles: Vec<ReactionRoleGroup>,
}

// ---------------------------------------------------------------------------------------------
// Defaults
// ---------------------------------------------------------------------------------------------

fn default_member_teams() -> Vec<MemberTeam> {
    let mk = |id: &str, name: &str, division: &str, players: u32, status: &str, aliases: &[&str]| MemberTeam {
        id: id.into(),
        name: name.into(),
        division: division.into(),
        players,
        recruitment_status: status.into(),
        aliases: aliases.iter().map(|a| a.to_string()).collect(),
        ..Default::default()
    };
    vec![
        mk("discord", "Discord", "Division-CC", 43, "Closed", &["Discord"]),
        mk("discord2", "Discord\u{b2}", "Division-CC", 49, "Closed", &["Discord\u{b2}", "Discord2"]),
        mk("discord3", "Discord 3\u{2122}", "Division-I", 49, "Closed", &["Discord 3\u{2122}", "Discord3"]),
        mk("nascar-dc", "Nascar DC", "Division-II", 0, "Open", &["Nascar DC"]),
        mk("baja-dc", "Baja DC", "Division-II", 50, "Open", &["Baja DC"]),
        mk("formula-dcx", "Formula DCx", "Division-VI", 50, "Open", &["Formula DCx"]),
        mk("rally-dcy", "Rally DCy", "Division-IV", 49, "Closed", &["Rally DCy"]),
    ]
}

fn default_youtube_feeds() -> Vec<YoutubeFeed> {
    let mk = |id: &str, name: &str, channel: &str| YoutubeFeed {
        id: id.into(),
        name: name.into(),
        channel_id: channel.into(),
        enabled: true,
        ..Default::default()
    };
    vec![
        mk("UCyL-QGEkA1r7R7U5rN_Yonw", "Vereshchak", "1341719063780393031"),
        mk("UC16xML3oyIZDeF3g8nnV6MA", "Vokope", "1341719063780393031"),
        mk("UCBrnPp4lpRukfuvXUiRz6_A", "Soulis HCR2", "1341719134135779389"),
        mk("UCwxuNdbZ-nK5oUEeY1tY9CQ", "tas HCR2", "1341719134135779389"),
        mk("UCBHmJJ0PN-efNW5PFdJ4EDQ", "PROJECT GER", "1341719134135779389"),
        mk("UCv_5HRU2ctFoYNeWFGLNoXw", "Exodus Hcr2", "1341719134135779389"),
        mk("UCF0iJo2klF-QGxzDDmOkQbQ", "Zorro HCR2", "1341719134135779389"),
        mk("UCnCaLcVf4YsPcsvi6PE4m6A", "ChillHcr2Guy", "1341733821707452437"),
    ]
}

pub fn default_config() -> DashboardConfig {
    let member_teams = default_member_teams();
    let spreadsheet_teams = member_teams
        .iter()
        .map(|team| {
            let mut aliases: Vec<String> = vec![team.name.clone()];
            for alias in &team.aliases {
                if !aliases.contains(alias) {
                    aliases.push(alias.clone());
                }
            }
            SpreadsheetTeam {
                id: team.id.clone(),
                name: team.name.clone(),
                enabled: false,
                own_team_aliases: aliases,
                auto_process: true,
                ..Default::default()
            }
        })
        .collect();

    DashboardConfig {
        version: CONFIG_VERSION,
        updated_at: "1970-01-01T00:00:00.000Z".into(),
        bot: BotConfig { locale: "en-US".into(), ..Default::default() },
        logging: LoggingConfig {
            enabled: true,
            channel_id: String::new(),
            events: LoggingEvents { tickets: true, member_counts: true, youtube: true, reaction_roles: true, system: true },
        },
        welcome: MessageSection {
            enabled: true,
            channel_id: "916042813425201152".into(),
            message: [
                "Hey {member}! Welcome to the **Discord Alliance server**!",
                "",
                "> 1. **Head to <#839605609027600415>**",
                "> Read the server rules carefully, and once done, press the confirmation reaction to get access.",
                ">",
                "> 2. **Unlock More Channels**",
                "> Go to <#840310137390104627> and select your desired option to access more channels of this server.",
                ">",
                "> 3. **Name Policy**",
                "> Please make sure your in-game name and your Discord display name matches in this server.",
                "> This helps leaders identify you easily.",
                "",
                "**Have fun and enjoy your time here!**",
            ]
            .join("\n"),
        },
        leave: MessageSection {
            enabled: true,
            channel_id: "839905184154517597".into(),
            message: "{member} has left the server.".into(),
        },
        recruitment: RecruitmentConfig {
            enabled: true,
            panel_title: "DCA Team Recruitment".into(),
            panel_description: "Ready to apply for the DCA teams? Press **Apply!** and follow the private prompts.".into(),
            panel_color: "#0f766e".into(),
            invite_message: "{user} Your application was accepted. Join **{server}** here: {invite}".into(),
            private_threads: true,
            thread_auto_archive_minutes: 10080,
            max_open_tickets_per_user: 1,
            transcript_on_close: true,
            delete_on_close: false,
            questions_intro: "Great that you want to join us! Please answer the following questions so that we can find out what team fits you the best.".into(),
            questions: DEFAULT_RECRUITMENT_QUESTIONS.into(),
            teams: recruitment_teams(),
            tutorials: vec![
                Tutorial {
                    id: "license-screenshot".into(),
                    label: "License Screenshot".into(),
                    description: "How to upload an uncropped driver's license screenshot with coins and gems visible.".into(),
                    guide_image_url: "/guides/driver-license.jpg".into(),
                    image_kind: "driver-license".into(),
                    enabled: true,
                    ..Default::default()
                },
                Tutorial {
                    id: "team-event-scores".into(),
                    label: "Team Event Scores".into(),
                    description: "How to upload the final standings or team-event score screenshot.".into(),
                    guide_image_url: "/guides/team-event-score.jpg".into(),
                    image_kind: "team-event-score".into(),
                    enabled: true,
                    ..Default::default()
                },
            ],
            ..Default::default()
        },
        member_counts: MemberCountsConfig {
            enabled: true,
            channel_id: "1341563215611433035".into(),
            message_id: String::new(),
            title: "Member Count".into(),
            update_on_recruitment_close: true,
            teams: member_teams,
        },
        spreadsheets: SpreadsheetsConfig {
            enabled: false,
            session_window_minutes: 1,
            output_format: "xlsx".into(),
            libre_office_path: String::new(),
            raw_data_retention_days: 31,
            image_retention_days: 7,
            teams: spreadsheet_teams,
        },
        youtube: YoutubeConfig {
            enabled: true,
            check_interval_minutes: 5,
            max_announcement_age_hours: 72,
            default_channel_id: String::new(),
            announcement_template: "**{name}** uploaded a new video!\n{url}".into(),
            feeds: default_youtube_feeds(),
        },
        reaction_roles: vec![
            ReactionRoleGroup {
                id: "language-unlock".into(),
                name: "Other languages".into(),
                enabled: true,
                channel_id: "840310137390104627".into(),
                message_id: String::new(),
                message: "If you want to speak in other language choose the confirmation reaction to select that.".into(),
                options: vec![ReactionOption {
                    emoji: "\u{2611}\u{fe0f}".into(),
                    role_id: "842089922768797726".into(),
                    label: "Other language".into(),
                }],
            },
            ReactionRoleGroup {
                id: "event-pings".into(),
                name: "Organized event pings".into(),
                enabled: true,
                channel_id: "839907517663936612".into(),
                message_id: String::new(),
                message: "React with thumbsup if you want ping everytime there is a organized event.".into(),
                options: vec![ReactionOption {
                    emoji: "\u{1f44d}".into(),
                    role_id: "840250757235212339".into(),
                    label: "PE call".into(),
                }],
            },
        ],
    }
}

// ---------------------------------------------------------------------------------------------
// Cleaners (ports of the JS helpers; "raw" is whatever JSON the dashboard / database handed us)
// ---------------------------------------------------------------------------------------------

type Raw<'a> = Option<&'a Value>;

fn obj<'a>(raw: Raw<'a>) -> Raw<'a> {
    raw.filter(|v| v.is_object())
}

fn get<'a>(raw: Raw<'a>, key: &str) -> Raw<'a> {
    raw.and_then(|v| v.get(key)).filter(|v| !v.is_null())
}

fn env(key: &str) -> String {
    std::env::var(key).unwrap_or_default()
}

fn first_non_empty(values: &[String]) -> String {
    values.iter().find(|v| !v.is_empty()).cloned().unwrap_or_default()
}

fn clean_snowflake(value: Raw, fallback: &str) -> String {
    match value.and_then(Value::as_str) {
        Some(text) if is_snowflake(text) => text.trim().to_string(),
        _ => fallback.to_string(),
    }
}

fn clean_bool(value: Raw, fallback: bool) -> bool {
    value.and_then(Value::as_bool).unwrap_or(fallback)
}

fn slice(text: &str, max: usize) -> String {
    text.chars().take(max).collect()
}

fn clean_text(value: Raw, fallback: &str, max: usize) -> String {
    match value.and_then(Value::as_str) {
        Some(text) => slice(if text.trim().is_empty() { fallback } else { text }, max),
        None => fallback.to_string(),
    }
}

fn clean_optional_text(value: Raw, fallback: &str, max: usize) -> String {
    match value.and_then(Value::as_str) {
        Some(text) => slice(text, max),
        None => fallback.to_string(),
    }
}

fn clean_name(value: Raw, fallback: &str, max: usize) -> String {
    match value.and_then(Value::as_str) {
        Some(text) => {
            let trimmed = text.trim();
            slice(if trimmed.is_empty() { fallback } else { trimmed }, max)
        }
        None => fallback.to_string(),
    }
}

fn id_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^[a-zA-Z0-9_-]{1,80}$").unwrap())
}

fn hex_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?i)^#?[0-9a-f]{6}$").unwrap())
}

fn short_id(prefix: &str) -> String {
    format!("{prefix}-{}", crate::util::random_hex(12))
}

fn clean_id(value: Raw, fallback: &str, prefix: &str) -> String {
    if let Some(text) = value.and_then(Value::as_str) {
        if id_re().is_match(text) {
            return text.to_string();
        }
    }
    if fallback.is_empty() {
        short_id(prefix)
    } else {
        fallback.to_string()
    }
}

fn clean_color(value: Raw, fallback: &str) -> String {
    match value.and_then(Value::as_str) {
        Some(text) => {
            let text = text.trim();
            if !hex_re().is_match(text) {
                return fallback.to_string();
            }
            if text.starts_with('#') {
                text.to_string()
            } else {
                format!("#{text}")
            }
        }
        None => fallback.to_string(),
    }
}

fn number_of(value: Raw) -> Option<f64> {
    match value? {
        Value::Number(n) => n.as_f64().filter(|f| f.is_finite()),
        Value::String(s) => s.trim().parse::<f64>().ok().filter(|f| f.is_finite()),
        Value::Bool(b) => Some(if *b { 1.0 } else { 0.0 }),
        _ => None,
    }
}

fn clean_number(value: Raw, fallback: f64, min: f64, max: f64) -> u32 {
    match number_of(value) {
        Some(n) => n.round().clamp(min, max) as u32,
        None => fallback as u32,
    }
}

fn value_to_string(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        Value::Bool(true) => "true".into(),
        _ => String::new(),
    }
}

fn clean_string_list(value: Raw, fallback: &[String], max_items: usize, max_len: usize) -> Vec<String> {
    let raw: Vec<String> = match value {
        Some(Value::Array(items)) => items.iter().map(value_to_string).collect(),
        Some(Value::String(text)) => text.split(|c| c == '\n' || c == ',').map(str::to_string).collect(),
        _ => fallback.to_vec(),
    };
    clean_string_items(raw, max_items, max_len)
}

fn clean_string_items(raw: Vec<String>, max_items: usize, max_len: usize) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for item in raw {
        let name = clean_name(Some(&Value::String(item)), "", max_len);
        if !name.is_empty() && !out.contains(&name) {
            out.push(name);
        }
    }
    out.truncate(max_items);
    out
}

fn clean_tutorial_kind(value: Raw, fallback: &str) -> String {
    let text = clean_name(value, fallback, 40).to_lowercase();
    if ["driver-license", "team-event-score", "general"].contains(&text.as_str()) {
        text
    } else {
        fallback.to_string()
    }
}

fn array<'a>(raw: Raw<'a>, key: &str) -> Option<&'a Vec<Value>> {
    get(raw, key).and_then(Value::as_array)
}

// ---------------------------------------------------------------------------------------------
// Section normalisers
// ---------------------------------------------------------------------------------------------

fn normalize_bot(input: Raw, fb: &BotConfig) -> BotConfig {
    let raw = obj(input);
    let guild_id = clean_snowflake(get(raw, "guildId"), &first_non_empty(&[env("DISCORD_GUILD_ID"), fb.guild_id.clone()]));
    BotConfig {
        guild_id: guild_id.clone(),
        community_guild_id: clean_snowflake(
            get(raw, "communityGuildId"),
            &first_non_empty(&[get(raw, "guildId").and_then(Value::as_str).filter(|s| is_snowflake(s)).unwrap_or("").trim().to_string(), env("COMMUNITY_GUILD_ID"), fb.community_guild_id.clone(), fb.guild_id.clone()]),
        ),
        recruitment_guild_id: clean_snowflake(
            get(raw, "recruitmentGuildId"),
            &first_non_empty(&[env("RECRUITMENT_GUILD_ID"), fb.recruitment_guild_id.clone(), fb.guild_id.clone()]),
        ),
        dashboard_allowed_role_id: clean_snowflake(
            get(raw, "dashboardAllowedRoleId"),
            &first_non_empty(&[env("DASHBOARD_ALLOWED_ROLE_ID"), env("DISCORD_DASHBOARD_ROLE_ID"), fb.dashboard_allowed_role_id.clone()]),
        ),
        recruiter_role_id: clean_snowflake(
            get(raw, "recruiterRoleId"),
            &first_non_empty(&[env("RECRUITER_ROLE_ID"), fb.recruiter_role_id.clone()]),
        ),
        manager_role_id: clean_snowflake(get(raw, "managerRoleId"), &fb.manager_role_id),
        locale: clean_name(get(raw, "locale"), if fb.locale.is_empty() { "en-US" } else { &fb.locale }, 20),
        command_log_channel_id: clean_snowflake(get(raw, "commandLogChannelId"), &fb.command_log_channel_id),
        // The address of this deployment (DASHBOARD_BASE_URL) wins over what an earlier host saved in the stored config,
        // so /dashboard, /help and the guide links never point at a previous host.
        dashboard_url: {
            let deployed = env("DASHBOARD_BASE_URL");
            if deployed.is_empty() {
                clean_optional_text(get(raw, "dashboardUrl"), &first_non_empty(&[fb.dashboard_url.clone()]), 300)
            } else {
                slice(deployed.trim_end_matches('/'), 300)
            }
        },
    }
}

fn normalize_logging(input: Raw, fb: &LoggingConfig) -> LoggingConfig {
    let raw = obj(input);
    let events = obj(get(raw, "events"));
    LoggingConfig {
        enabled: clean_bool(get(raw, "enabled"), fb.enabled),
        channel_id: clean_snowflake(get(raw, "channelId"), &fb.channel_id),
        events: LoggingEvents {
            tickets: clean_bool(get(events, "tickets"), fb.events.tickets),
            member_counts: clean_bool(get(events, "memberCounts"), fb.events.member_counts),
            youtube: clean_bool(get(events, "youtube"), fb.events.youtube),
            reaction_roles: clean_bool(get(events, "reactionRoles"), fb.events.reaction_roles),
            system: clean_bool(get(events, "system"), fb.events.system),
        },
    }
}

fn normalize_message_section(input: Raw, fb: &MessageSection) -> MessageSection {
    let raw = obj(input);
    MessageSection {
        enabled: clean_bool(get(raw, "enabled"), fb.enabled),
        channel_id: clean_snowflake(get(raw, "channelId"), &fb.channel_id),
        message: clean_text(get(raw, "message"), &fb.message, 2000),
    }
}

fn normalize_tutorial(input: Raw, base: Option<&Tutorial>, index: usize) -> Tutorial {
    let raw = obj(input);
    let b = base.cloned().unwrap_or_default();
    Tutorial {
        id: clean_id(get(raw, "id"), &if b.id.is_empty() { format!("tutorial-{}", index + 1) } else { b.id.clone() }, "tutorial"),
        label: clean_name(get(raw, "label"), &if b.label.is_empty() { format!("Tutorial {}", index + 1) } else { b.label.clone() }, 40),
        description: clean_optional_text(get(raw, "description"), &b.description, 500),
        video_url: clean_optional_text(get(raw, "videoUrl"), &b.video_url, 1000),
        guide_image_url: clean_optional_text(get(raw, "guideImageUrl"), &b.guide_image_url, 1000),
        image_kind: clean_tutorial_kind(get(raw, "imageKind"), if b.image_kind.is_empty() { "general" } else { &b.image_kind }),
        enabled: clean_bool(get(raw, "enabled"), base.map(|t| t.enabled).unwrap_or(true)),
    }
}

fn normalize_recruitment(input: Raw, base: &RecruitmentConfig) -> RecruitmentConfig {
    let raw = obj(input);
    let raw_tutorials: Vec<Value> = array(raw, "tutorials").cloned().unwrap_or_else(|| base.tutorials.iter().map(|t| serde_json::to_value(t).unwrap()).collect());
    let raw_teams: Vec<Value> = array(raw, "teams").cloned().unwrap_or_else(|| base.teams.iter().map(|t| Value::String(t.clone())).collect());
    let ban_list: Vec<Value> = array(raw, "banListMessageIds").cloned().unwrap_or_else(|| base.ban_list_message_ids.iter().map(|t| Value::String(t.clone())).collect());

    RecruitmentConfig {
        enabled: clean_bool(get(raw, "enabled"), base.enabled),
        panel_channel_id: clean_snowflake(get(raw, "panelChannelId"), &base.panel_channel_id),
        panel_message_id: clean_snowflake(get(raw, "panelMessageId"), &base.panel_message_id),
        panel_title: clean_name(get(raw, "panelTitle"), &base.panel_title, 120),
        panel_description: clean_text(get(raw, "panelDescription"), &base.panel_description, 2000),
        panel_color: clean_color(get(raw, "panelColor"), if base.panel_color.is_empty() { "#0f766e" } else { &base.panel_color }),
        log_channel_id: clean_snowflake(get(raw, "logChannelId"), &base.log_channel_id),
        recruiter_alert_channel_id: clean_snowflake(get(raw, "recruiterAlertChannelId"), &base.recruiter_alert_channel_id),
        tutorial_upload_channel_id: clean_snowflake(get(raw, "tutorialUploadChannelId"), &base.tutorial_upload_channel_id),
        screenshot_dm_user_id: clean_snowflake(
            get(raw, "screenshotDmUserId"),
            &first_non_empty(&[env("RECRUITMENT_SCREENSHOT_DM_USER_ID"), base.screenshot_dm_user_id.clone()]),
        ),
        recruiter_role_id: clean_snowflake(
            get(raw, "recruiterRoleId"),
            &first_non_empty(&[env("RECRUITER_ROLE_ID"), base.recruiter_role_id.clone()]),
        ),
        invite_guild_id: clean_snowflake(get(raw, "inviteGuildId"), &base.invite_guild_id),
        invite_channel_id: clean_snowflake(get(raw, "inviteChannelId"), &base.invite_channel_id),
        invite_message: clean_text(
            get(raw, "inviteMessage"),
            if base.invite_message.is_empty() { "{user} Your application was accepted. Join **{server}** here: {invite}" } else { &base.invite_message },
            1200,
        ),
        community_rules_role_id: clean_snowflake(get(raw, "communityRulesRoleId"), &base.community_rules_role_id),
        ban_list_channel_id: clean_snowflake(get(raw, "banListChannelId"), &base.ban_list_channel_id),
        ban_list_message_ids: ban_list
            .iter()
            .take(20)
            .map(|v| clean_snowflake(Some(v), ""))
            .filter(|s| !s.is_empty())
            .collect(),
        private_threads: clean_bool(get(raw, "privateThreads"), base.private_threads),
        thread_auto_archive_minutes: clean_number(get(raw, "threadAutoArchiveMinutes"), base.thread_auto_archive_minutes.max(1) as f64, 60.0, 10080.0),
        max_open_tickets_per_user: clean_number(get(raw, "maxOpenTicketsPerUser"), base.max_open_tickets_per_user.max(1) as f64, 1.0, 10.0),
        transcript_on_close: clean_bool(get(raw, "transcriptOnClose"), base.transcript_on_close),
        delete_on_close: false,
        questions_intro: clean_text(get(raw, "questionsIntro"), &base.questions_intro, 800),
        questions: clean_text(
            get(raw, "questions"),
            if base.questions.is_empty() { DEFAULT_RECRUITMENT_QUESTIONS } else { &base.questions },
            2000,
        ),
        teams: raw_teams
            .iter()
            .take(12)
            .map(|team| clean_name(Some(team), "", 40))
            .filter(|t| !t.is_empty())
            .collect(),
        tutorials: raw_tutorials
            .iter()
            .take(10)
            .enumerate()
            .map(|(index, tutorial)| normalize_tutorial(Some(tutorial), base.tutorials.get(index), index))
            .collect(),
    }
}

fn normalize_member_team(input: Raw, base: Option<&MemberTeam>, index: usize) -> MemberTeam {
    let raw = obj(input);
    let b = base.cloned().unwrap_or_default();
    let name = clean_name(get(raw, "name"), &if b.name.is_empty() { format!("Team {}", index + 1) } else { b.name.clone() }, 60);

    let community_role_fallback = {
        let from_raw_role = get(raw, "roleId").and_then(Value::as_str).unwrap_or("").to_string();
        first_non_empty(&[from_raw_role, b.community_role_id.clone(), b.role_id.clone()])
    };
    let community_role_id = clean_snowflake(get(raw, "communityRoleId"), &community_role_fallback);
    let community_auto_fallback = get(raw, "autoAssignEnabled")
        .and_then(Value::as_bool)
        .unwrap_or(b.community_role_auto_assign_enabled || b.auto_assign_enabled);
    let community_auto = clean_bool(get(raw, "communityRoleAutoAssignEnabled"), community_auto_fallback);
    let community_delay_fallback = number_of(get(raw, "autoAssignDelayMinutes"))
        .unwrap_or(if b.community_role_delay_minutes > 0 { b.community_role_delay_minutes as f64 } else { b.auto_assign_delay_minutes as f64 });
    let community_delay = clean_number(get(raw, "communityRoleDelayMinutes"), community_delay_fallback, 0.0, 43200.0);

    let aliases_raw: Vec<Value> = array(raw, "aliases").cloned().unwrap_or_else(|| b.aliases.iter().map(|a| Value::String(a.clone())).collect());
    let default_id = {
        let lowered = name.to_lowercase();
        let mut id = String::new();
        let mut last_dash = true;
        for ch in lowered.chars() {
            if ch.is_ascii_alphanumeric() {
                id.push(ch);
                last_dash = false;
            } else if !last_dash {
                id.push('-');
                last_dash = true;
            }
        }
        id
    };

    MemberTeam {
        id: clean_id(get(raw, "id"), &if b.id.is_empty() { default_id } else { b.id.clone() }, "team"),
        name,
        division: clean_optional_text(get(raw, "division"), &b.division, 80),
        players: clean_number(get(raw, "players"), b.players as f64, 0.0, 999.0),
        recruitment_status: clean_name(get(raw, "recruitmentStatus"), if b.recruitment_status.is_empty() { "Open" } else { &b.recruitment_status }, 40),
        recruitment_role_id: clean_snowflake(get(raw, "recruitmentRoleId"), &b.recruitment_role_id),
        recruitment_role_auto_assign_enabled: clean_bool(get(raw, "recruitmentRoleAutoAssignEnabled"), b.recruitment_role_auto_assign_enabled),
        recruitment_role_delay_minutes: clean_number(get(raw, "recruitmentRoleDelayMinutes"), b.recruitment_role_delay_minutes as f64, 0.0, 43200.0),
        community_role_id: community_role_id.clone(),
        community_role_auto_assign_enabled: community_auto,
        community_role_delay_minutes: community_delay,
        role_id: community_role_id,
        auto_assign_enabled: community_auto,
        auto_assign_delay_minutes: community_delay,
        aliases: aliases_raw
            .iter()
            .take(10)
            .map(|alias| clean_name(Some(alias), "", 60))
            .filter(|a| !a.is_empty())
            .collect(),
    }
}

fn normalize_member_counts(input: Raw, base: &MemberCountsConfig) -> MemberCountsConfig {
    let raw = obj(input);
    let teams: Vec<Value> = array(raw, "teams").cloned().unwrap_or_else(|| base.teams.iter().map(|t| serde_json::to_value(t).unwrap()).collect());
    MemberCountsConfig {
        enabled: clean_bool(get(raw, "enabled"), base.enabled),
        channel_id: clean_snowflake(get(raw, "channelId"), &base.channel_id),
        message_id: clean_snowflake(get(raw, "messageId"), &base.message_id),
        title: clean_name(get(raw, "title"), if base.title.is_empty() { "Member Count" } else { &base.title }, 100),
        update_on_recruitment_close: clean_bool(get(raw, "updateOnRecruitmentClose"), base.update_on_recruitment_close),
        teams: teams
            .iter()
            .take(25)
            .enumerate()
            .map(|(index, team)| normalize_member_team(Some(team), base.teams.get(index), index))
            .collect(),
    }
}

fn normalize_spreadsheet_team(input: Raw, base: Option<&SpreadsheetTeam>, index: usize) -> SpreadsheetTeam {
    let raw = obj(input);
    let b = base.cloned().unwrap_or_default();
    let name = clean_name(get(raw, "name"), &if b.name.is_empty() { format!("Team {}", index + 1) } else { b.name.clone() }, 60);
    let default_id = {
        let lowered = name.to_lowercase();
        let mut id = String::new();
        let mut last_dash = true;
        for ch in lowered.chars() {
            if ch.is_ascii_alphanumeric() {
                id.push(ch);
                last_dash = false;
            } else if !last_dash {
                id.push('-');
                last_dash = true;
            }
        }
        id
    };
    let team_alias_fallback = if b.own_team_aliases.is_empty() { vec![name.clone()] } else { b.own_team_aliases.clone() };

    SpreadsheetTeam {
        id: clean_id(get(raw, "id"), &if b.id.is_empty() { default_id } else { b.id.clone() }, "spreadsheet-team"),
        name,
        enabled: clean_bool(get(raw, "enabled"), base.map(|t| t.enabled).unwrap_or(false)),
        monitored_channel_id: clean_snowflake(get(raw, "monitoredChannelId"), &b.monitored_channel_id),
        output_channel_id: clean_snowflake(get(raw, "outputChannelId"), &b.output_channel_id),
        access_role_id: clean_snowflake(get(raw, "accessRoleId"), &b.access_role_id),
        own_team_aliases: clean_string_list(get(raw, "ownTeamAliases"), &team_alias_fallback, 25, 80),
        own_player_aliases: clean_string_list(get(raw, "ownPlayerAliases"), &b.own_player_aliases, 250, 80),
        auto_process: clean_bool(get(raw, "autoProcess"), base.map(|t| t.auto_process).unwrap_or(true)),
    }
}

fn normalize_spreadsheets(input: Raw, base: &SpreadsheetsConfig) -> SpreadsheetsConfig {
    let raw = obj(input);
    let raw_teams: Vec<Value> = array(raw, "teams").cloned().unwrap_or_else(|| base.teams.iter().map(|t| serde_json::to_value(t).unwrap()).collect());
    let output_format = clean_name(get(raw, "outputFormat"), if base.output_format.is_empty() { "xlsx" } else { &base.output_format }, 10).to_lowercase();

    SpreadsheetsConfig {
        enabled: clean_bool(get(raw, "enabled"), base.enabled),
        session_window_minutes: clean_number(get(raw, "sessionWindowMinutes"), base.session_window_minutes.max(1) as f64, 1.0, 30.0),
        output_format: if ["xlsx", "fods"].contains(&output_format.as_str()) { output_format } else { "xlsx".into() },
        libre_office_path: clean_optional_text(
            get(raw, "libreOfficePath"),
            &first_non_empty(&[env("LIBREOFFICE_PATH"), base.libre_office_path.clone()]),
            500,
        ),
        raw_data_retention_days: clean_number(get(raw, "rawDataRetentionDays"), base.raw_data_retention_days.max(1) as f64, 1.0, 370.0),
        image_retention_days: clean_number(
            get(raw, "imageRetentionDays"),
            env("SPREADSHEET_IMAGE_RETENTION_DAYS").parse::<f64>().unwrap_or(if base.image_retention_days > 0 { base.image_retention_days as f64 } else { 7.0 }),
            1.0,
            90.0,
        ),
        teams: raw_teams
            .iter()
            .take(30)
            .enumerate()
            .map(|(index, team)| normalize_spreadsheet_team(Some(team), base.teams.get(index), index))
            .collect(),
    }
}

fn normalize_youtube_feed(input: Raw, base: Option<&YoutubeFeed>, index: usize) -> YoutubeFeed {
    let raw = obj(input);
    let b = base.cloned().unwrap_or_default();
    let mut sources: Vec<String> = Vec::new();
    for key in ["sentVideoIds", "postedVideoIds", "seenVideoIds"] {
        if let Some(items) = array(raw, key) {
            sources.extend(items.iter().map(value_to_string));
        }
    }
    sources.extend(b.sent_video_ids.iter().cloned());
    sources.push(get(raw, "lastVideoId").map(value_to_string).unwrap_or_default());
    sources.push(b.last_video_id.clone());
    let sent = clean_string_items(sources, 50, 80);

    YoutubeFeed {
        id: clean_name(get(raw, "id"), &b.id, 120),
        name: clean_name(get(raw, "name"), &if b.name.is_empty() { format!("Feed {}", index + 1) } else { b.name.clone() }, 80),
        channel_id: clean_snowflake(get(raw, "channelId"), &b.channel_id),
        enabled: clean_bool(get(raw, "enabled"), base.map(|f| f.enabled).unwrap_or(true)),
        last_video_id: clean_optional_text(
            get(raw, "lastVideoId"),
            &if b.last_video_id.is_empty() { sent.first().cloned().unwrap_or_default() } else { b.last_video_id.clone() },
            80,
        ),
        last_published_at: clean_optional_text(get(raw, "lastPublishedAt"), &b.last_published_at, 80),
        last_checked_at: clean_optional_text(get(raw, "lastCheckedAt"), &b.last_checked_at, 80),
        sent_video_ids: sent,
    }
}

fn normalize_youtube(input: Raw, base: &YoutubeConfig) -> YoutubeConfig {
    let raw = obj(input);
    let feeds: Vec<Value> = array(raw, "feeds").cloned().unwrap_or_else(|| base.feeds.iter().map(|f| serde_json::to_value(f).unwrap()).collect());
    YoutubeConfig {
        enabled: clean_bool(get(raw, "enabled"), base.enabled),
        check_interval_minutes: clean_number(get(raw, "checkIntervalMinutes"), base.check_interval_minutes.max(1) as f64, 1.0, 1440.0),
        max_announcement_age_hours: clean_number(get(raw, "maxAnnouncementAgeHours"), base.max_announcement_age_hours.max(1) as f64, 1.0, 720.0),
        default_channel_id: clean_snowflake(get(raw, "defaultChannelId"), &base.default_channel_id),
        announcement_template: clean_text(get(raw, "announcementTemplate"), &base.announcement_template, 1200),
        feeds: feeds
            .iter()
            .take(50)
            .enumerate()
            .map(|(index, feed)| normalize_youtube_feed(Some(feed), base.feeds.get(index), index))
            .filter(|feed| !feed.id.is_empty())
            .collect(),
    }
}

fn normalize_reaction_option(input: Raw) -> Option<ReactionOption> {
    let raw = obj(input)?;
    let emoji = clean_name(raw.get("emoji"), "", 128);
    let role_id = clean_snowflake(raw.get("roleId"), "");
    if emoji.is_empty() || role_id.is_empty() {
        return None;
    }
    Some(ReactionOption { emoji, role_id, label: clean_name(raw.get("label"), "", 80) })
}

fn normalize_reaction_group(input: Raw, base: Option<&ReactionRoleGroup>, index: usize) -> ReactionRoleGroup {
    let raw = obj(input);
    let b = base.cloned().unwrap_or_default();
    let options: Vec<Value> = array(raw, "options").cloned().unwrap_or_else(|| b.options.iter().map(|o| serde_json::to_value(o).unwrap()).collect());
    ReactionRoleGroup {
        id: clean_id(get(raw, "id"), &if b.id.is_empty() { format!("reaction-{}", index + 1) } else { b.id.clone() }, "reaction"),
        name: clean_name(get(raw, "name"), &if b.name.is_empty() { format!("Reaction message {}", index + 1) } else { b.name.clone() }, 80),
        enabled: clean_bool(get(raw, "enabled"), base.map(|g| g.enabled).unwrap_or(true)),
        channel_id: clean_snowflake(get(raw, "channelId"), &b.channel_id),
        message_id: clean_snowflake(get(raw, "messageId"), &b.message_id),
        message: clean_text(get(raw, "message"), if b.message.is_empty() { "React below to choose a role." } else { &b.message }, 2000),
        options: options.iter().take(25).filter_map(|o| normalize_reaction_option(Some(o))).collect(),
    }
}

/// Port of `normalizeDashboardConfig`.
pub fn normalize_config(input: &Value, preserve_updated_at: bool) -> DashboardConfig {
    let defaults = default_config();
    let raw = if input.is_object() { Some(input) } else { None };
    let reaction_roles: Vec<Value> = array(raw, "reactionRoles")
        .cloned()
        .unwrap_or_else(|| defaults.reaction_roles.iter().map(|g| serde_json::to_value(g).unwrap()).collect());

    let version = number_of(get(raw, "version")).unwrap_or(0.0);
    let legacy_window = version < 7.0 && number_of(get(raw.and_then(|r| r.get("spreadsheets")), "sessionWindowMinutes")) == Some(5.0);
    let spreadsheets_input: Option<Value> = if legacy_window {
        let mut copy = raw.and_then(|r| r.get("spreadsheets")).cloned().unwrap_or(json!({}));
        copy["sessionWindowMinutes"] = json!(1);
        Some(copy)
    } else {
        raw.and_then(|r| r.get("spreadsheets")).cloned()
    };

    DashboardConfig {
        version: CONFIG_VERSION,
        updated_at: if preserve_updated_at {
            get(raw, "updatedAt").and_then(Value::as_str).map(str::to_string).unwrap_or_else(now_iso)
        } else {
            now_iso()
        },
        bot: normalize_bot(get(raw, "bot"), &defaults.bot),
        logging: normalize_logging(get(raw, "logging"), &defaults.logging),
        welcome: normalize_message_section(get(raw, "welcome"), &defaults.welcome),
        leave: normalize_message_section(get(raw, "leave"), &defaults.leave),
        recruitment: normalize_recruitment(get(raw, "recruitment"), &defaults.recruitment),
        member_counts: normalize_member_counts(get(raw, "memberCounts"), &defaults.member_counts),
        spreadsheets: normalize_spreadsheets(spreadsheets_input.as_ref(), &defaults.spreadsheets),
        youtube: normalize_youtube(get(raw, "youtube"), &defaults.youtube),
        reaction_roles: reaction_roles
            .iter()
            .take(25)
            .enumerate()
            .map(|(index, group)| normalize_reaction_group(Some(group), defaults.reaction_roles.get(index), index))
            .collect(),
    }
}

// ---------------------------------------------------------------------------------------------
// Persistence
// ---------------------------------------------------------------------------------------------

fn default_value() -> Value {
    serde_json::to_value(default_config()).unwrap()
}

pub async fn load_config(store: &StateStore) -> DashboardConfig {
    let raw = store.read(CONFIG_SCOPE, default_value()).await;
    normalize_config(&raw, true)
}

pub async fn save_config(store: &StateStore, config: &DashboardConfig) -> Result<DashboardConfig, String> {
    let normalized = normalize_config(&serde_json::to_value(config).map_err(|e| e.to_string())?, false);
    store.write(CONFIG_SCOPE, serde_json::to_value(&normalized).map_err(|e| e.to_string())?).await?;
    Ok(normalized)
}

pub async fn save_config_value(store: &StateStore, raw: &Value) -> Result<DashboardConfig, String> {
    let normalized = normalize_config(raw, false);
    store.write(CONFIG_SCOPE, serde_json::to_value(&normalized).map_err(|e| e.to_string())?).await?;
    Ok(normalized)
}

/// Atomically load, change and save the config (no other writer can interleave).
pub async fn update_config<R>(
    store: &StateStore,
    f: impl FnOnce(&mut DashboardConfig) -> R,
) -> Result<(DashboardConfig, R), String> {
    store
        .mutate(CONFIG_SCOPE, default_value(), |value| {
            let mut config = normalize_config(value, true);
            let result = f(&mut config);
            let normalized = normalize_config(&serde_json::to_value(&config).unwrap_or(Value::Null), false);
            *value = serde_json::to_value(&normalized).unwrap_or(Value::Null);
            (normalized, result)
        })
        .await
}

#[cfg(test)]
mod dump_tests {
    /// `DCA_DUMP_CONFIG=<file>` writes the default config (diffed against the old bot's `normalizeDashboardConfig({})`).
    #[test]
    fn dump_default_config() {
        let value = serde_json::to_value(super::default_config()).unwrap();
        if let Ok(path) = std::env::var("DCA_DUMP_CONFIG") {
            std::fs::write(path, serde_json::to_string_pretty(&value).unwrap()).unwrap();
        }
        assert!(value.is_object());
    }
}

#[cfg(test)]
mod differential_tests {
    /// `DCA_NORMALIZE_IN=<file>` (JSON array of raw configs) -> `DCA_NORMALIZE_OUT` (their normalised form).
    #[test]
    fn normalise_inputs() {
        let (Ok(input), Ok(output)) = (std::env::var("DCA_NORMALIZE_IN"), std::env::var("DCA_NORMALIZE_OUT")) else { return };
        let raws: Vec<serde_json::Value> = serde_json::from_str(&std::fs::read_to_string(input).unwrap()).unwrap();
        let out: Vec<serde_json::Value> = raws.iter().map(|r| serde_json::to_value(super::normalize_config(r, true)).unwrap()).collect();
        std::fs::write(output, serde_json::to_string_pretty(&out).unwrap()).unwrap();
    }
}
