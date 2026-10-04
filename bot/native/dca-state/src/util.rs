//! Small helpers shared by the state modules (JS-compatible cleaning/ISO time helpers).

use serde_json::Value;

/// `new Date().toISOString()` format.
pub fn now_iso() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

pub fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

/// Parse an ISO/RFC3339 timestamp to epoch milliseconds (like `Date.parse`).
pub fn parse_ms(value: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(value.trim()).ok().map(|d| d.timestamp_millis())
}

/// 12-hex-char random suffix (`Math.random().toString(16).slice(2)` style).
pub fn random_hex(len: usize) -> String {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    (0..len).map(|_| std::char::from_digit(rng.gen_range(0..16), 16).unwrap()).collect()
}

pub fn is_snowflake(value: &str) -> bool {
    let text = value.trim();
    (10..=25).contains(&text.len()) && text.bytes().all(|b| b.is_ascii_digit())
}

pub fn truncate_chars(value: &str, max: usize) -> String {
    value.chars().take(max).collect()
}

pub fn clone_json<T: Clone>(value: &T) -> T {
    value.clone()
}

pub fn str_field<'a>(value: &'a Value, key: &str) -> &'a str {
    value.get(key).and_then(Value::as_str).unwrap_or("")
}
