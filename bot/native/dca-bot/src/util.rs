//! Small helpers shared across the bot.

use regex::Regex;
use serenity::all::*;
use std::sync::OnceLock;

pub type BotResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

pub fn err<T>(message: impl Into<String>) -> BotResult<T> {
    Err(message.into().into())
}

pub fn display_tag(user: &User) -> String {
    let tag = user.tag();
    if tag.is_empty() {
        user.id.to_string()
    } else {
        tag
    }
}

pub fn color_to_number(color: &str) -> u32 {
    u32::from_str_radix(color.trim().trim_start_matches('#'), 16).unwrap_or(0x0f766e)
}

pub fn truncate(text: &str, max: usize) -> String {
    text.chars().take(max).collect()
}

pub fn snowflake(value: &str) -> Option<u64> {
    let t = value.trim();
    if (10..=25).contains(&t.len()) && t.bytes().all(|b| b.is_ascii_digit()) {
        t.parse().ok()
    } else {
        None
    }
}

pub fn channel_id(value: &str) -> Option<ChannelId> {
    snowflake(value).map(ChannelId::new)
}

pub fn role_id(value: &str) -> Option<RoleId> {
    snowflake(value).map(RoleId::new)
}

pub fn guild_id(value: &str) -> Option<GuildId> {
    snowflake(value).map(GuildId::new)
}

pub fn user_id(value: &str) -> Option<UserId> {
    snowflake(value).map(UserId::new)
}

pub fn all_ids(text: &str) -> Vec<u64> {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r"\d{10,25}").unwrap());
    let mut out: Vec<u64> = Vec::new();
    for m in re.find_iter(text) {
        if let Ok(id) = m.as_str().parse::<u64>() {
            if !out.contains(&id) {
                out.push(id);
            }
        }
    }
    out
}

/// Port of the `ms` package for the formats the bot accepted ("10m", "1h", "2 days", "1.5h", "5000").
pub fn parse_duration_ms(input: &str) -> Option<u64> {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r"(?i)^(-?(?:\d+)?\.?\d+) *(milliseconds?|msecs?|ms|seconds?|secs?|s|minutes?|mins?|m|hours?|hrs?|h|days?|d|weeks?|w|years?|yrs?|y)?$").unwrap());
    let caps = re.captures(input.trim())?;
    let n: f64 = caps.get(1)?.as_str().parse().ok()?;
    let unit = caps.get(2).map(|u| u.as_str().to_lowercase()).unwrap_or_else(|| "ms".into());
    let mult: f64 = match unit.as_str() {
        "years" | "year" | "yrs" | "yr" | "y" => 31_557_600_000.0,
        "weeks" | "week" | "w" => 604_800_000.0,
        "days" | "day" | "d" => 86_400_000.0,
        "hours" | "hour" | "hrs" | "hr" | "h" => 3_600_000.0,
        "minutes" | "minute" | "mins" | "min" | "m" => 60_000.0,
        "seconds" | "second" | "secs" | "sec" | "s" => 1000.0,
        _ => 1.0,
    };
    let ms = n * mult;
    if ms.is_finite() && ms > 0.0 {
        Some(ms as u64)
    } else {
        None
    }
}

pub fn split_args(text: &str) -> Vec<String> {
    text.split(' ').filter(|s| !s.is_empty()).map(str::to_string).collect()
}

pub fn unix_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

pub fn discord_timestamp(ms: i64, style: char) -> String {
    format!("<t:{}:{style}>", ms / 1000)
}

pub fn safe_name(value: &str, fallback: &str, max: usize) -> String {
    let cleaned: String = value.chars().map(|c| if matches!(c, '/' | '\\' | '?' | '%' | '*' | ':' | '|' | '"' | '<' | '>') { '-' } else { c }).collect();
    let dashed = cleaned.split_whitespace().collect::<Vec<_>>().join("-");
    let trimmed = dashed.trim_matches('-');
    let out = truncate(trimmed, max);
    if out.is_empty() {
        fallback.to_string()
    } else {
        out
    }
}

pub fn is_image_attachment(a: &Attachment) -> bool {
    let ct = a.content_type.clone().unwrap_or_default();
    if ct.starts_with("image/") {
        return true;
    }
    let name = a.filename.to_lowercase();
    [".png", ".jpg", ".jpeg", ".webp", ".gif", ".bmp"].iter().any(|ext| name.ends_with(ext) || a.url.to_lowercase().split('?').next().unwrap_or("").ends_with(ext))
}

pub fn embed_color(color: &str) -> Colour {
    Colour::new(color_to_number(color))
}
