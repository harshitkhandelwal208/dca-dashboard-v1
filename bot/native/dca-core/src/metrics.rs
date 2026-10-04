//! Port of `spreadsheetMetrics.js`: derived numbers for a processed team-event session.

use chrono::{DateTime, Utc};
use dca_state::models::{Player, SpreadsheetSession};
use regex::Regex;
use std::collections::HashMap;
use std::sync::OnceLock;

pub fn clean_text(value: &str, fallback: &str, max: usize) -> String {
    let collapsed = value.split_whitespace().collect::<Vec<_>>().join(" ");
    let text = if collapsed.is_empty() { fallback.to_string() } else { collapsed };
    text.chars().take(max).collect()
}

pub fn normalize_key(value: &str) -> String {
    let lowered = value.to_lowercase().replace(['\u{ae}', '\u{2122}'], "");
    let mut out = String::new();
    let mut last_space = true;
    for c in lowered.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c);
            last_space = false;
        } else if !last_space {
            out.push(' ');
            last_space = true;
        }
    }
    out.trim().to_string()
}

/// Key that also works for non-Latin names (the JS key dropped them entirely, collapsing e.g. Cyrillic names).
pub fn player_key(name: &str) -> String {
    let ascii = normalize_key(name);
    if !ascii.is_empty() && ascii.chars().any(|c| c.is_ascii_alphanumeric()) && name.chars().all(|c| c.is_ascii() || !c.is_alphanumeric()) {
        return ascii;
    }
    crate::unicode_names::player_key(name)
}

pub fn session_date(session: &SpreadsheetSession) -> DateTime<Utc> {
    for value in [&session.processed_at, &session.updated_at, &session.created_at] {
        if let Ok(d) = DateTime::parse_from_rfc3339(value) {
            return d.with_timezone(&Utc);
        }
    }
    Utc::now()
}

pub fn event_name_for_session(session: &SpreadsheetSession, fallback: &str) -> String {
    for value in [&session.metadata.event_name, &session.metadata.title, &session.team_event_name] {
        let t = clean_text(value, "", 120);
        if !t.is_empty() {
            return t;
        }
    }
    clean_text("", fallback, 120)
}

pub fn normalize_team_type(value: &str) -> &'static str {
    match normalize_key(value).as_str() {
        "own" | "our" | "team" | "teammate" | "ally" | "allied" => "own",
        "opponent" | "enemy" | "opposing" | "rival" | "other" | "blue" => "opponent",
        _ => "unknown",
    }
}

pub fn score_value(p: &Player) -> i64 {
    p.score.unwrap_or(0)
}

pub fn points_value(p: &Player) -> i64 {
    p.points.unwrap_or(0)
}

pub fn own_players(session: &SpreadsheetSession) -> Vec<&Player> {
    session.players.iter().filter(|p| p.team_type == "own").collect()
}

pub fn opponent_players(session: &SpreadsheetSession) -> Vec<&Player> {
    session.players.iter().filter(|p| p.team_type != "own").collect()
}

pub fn top_opponent_rank(session: &SpreadsheetSession) -> Option<u32> {
    opponent_players(session).iter().map(|p| p.rank).filter(|r| *r > 0).min()
}

pub fn opponent_count(session: &SpreadsheetSession) -> usize {
    opponent_players(session).len()
}

pub fn blues_killed_for_rank(session: &SpreadsheetSession, rank: u32) -> usize {
    opponent_players(session).iter().filter(|p| p.rank > rank).count()
}

pub fn percent(value: f64, max: f64) -> f64 {
    if max <= 0.0 || !value.is_finite() || !max.is_finite() {
        return 0.0;
    }
    ((value / max) * 1000.0).round() / 10.0
}

pub fn blue_kill_percent(killed: usize, possible: usize) -> f64 {
    percent(killed as f64, possible as f64)
}

/// `playerName -> 1/0` : did this own player finish above every opponent?
pub fn session_kab_map(session: &SpreadsheetSession) -> HashMap<String, u32> {
    let threshold = top_opponent_rank(session);
    let mut map = HashMap::new();
    for p in own_players(session) {
        let kab = threshold.map_or(false, |t| p.rank > 0 && p.rank < t);
        map.insert(p.player_name.clone(), kab as u32);
    }
    map
}

pub fn event_max_score(session: &SpreadsheetSession) -> i64 {
    session.players.iter().map(score_value).max().unwrap_or(0).max(0)
}

pub fn event_max_points(session: &SpreadsheetSession) -> i64 {
    session.players.iter().map(points_value).max().unwrap_or(0).max(0)
}

pub fn unique_names(names: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for name in names {
        let name = clean_text(&name, "", 80);
        let key = player_key(&name);
        if key.is_empty() || !seen.insert(key) {
            continue;
        }
        out.push(name);
    }
    out
}

fn re(pattern: &'static str, cell: &'static OnceLock<Regex>) -> &'static Regex {
    cell.get_or_init(|| Regex::new(pattern).unwrap())
}

pub fn clean_team_name(value: &str, own_team_name: &str) -> String {
    static WORDS: OnceLock<Regex> = OnceLock::new();
    static SCORE_FRAG: OnceLock<Regex> = OnceLock::new();
    static NUMBERS: OnceLock<Regex> = OnceLock::new();
    static SPLIT: OnceLock<Regex> = OnceLock::new();
    static TOKENS: OnceLock<Regex> = OnceLock::new();

    let mut text = clean_text(value, "", 100);
    text = re(r"(?i)\b(?:winner|team|score|points?)\b", &WORDS).replace_all(&text, " ").to_string();
    text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let own = clean_text(own_team_name, "", 80);
    let own_key = normalize_key(&own);
    let number_tokens = re(r"\b\d+\b", &TOKENS).find_iter(&text).count();
    let has_score_fragments = (!own_key.is_empty() && normalize_key(&text).contains(&own_key))
        || re(r"\b\d{1,3}[\s,.]\d{3}\b", &SCORE_FRAG).is_match(&text)
        || number_tokens >= 2;

    if !own.is_empty() {
        if let Ok(r) = Regex::new(&format!(r"(?i)\b{}\b", regex::escape(&own))) {
            text = r.replace_all(&text, " ").to_string();
        }
    }
    if has_score_fragments {
        text = re(r"\b\d[\d\s,.]*\b", &NUMBERS).replace_all(&text, " ").to_string();
    }
    let pieces: Vec<String> = re(r"(?i)\bvs\b|,|;|\||/", &SPLIT)
        .split(&text)
        .map(|part| clean_text(part, "", 80))
        .filter(|part| !part.is_empty())
        .filter(|part| normalize_key(part) != own_key)
        .collect();
    if let Some(last) = pieces.last() {
        text = last.clone();
    }
    clean_text(&text, "", 80)
}

fn names_from_score_line(raw_line: &str, own_team_name: &str) -> Vec<String> {
    static SPLIT: OnceLock<Regex> = OnceLock::new();
    static TRAIL: OnceLock<Regex> = OnceLock::new();
    let own_key = normalize_key(own_team_name);
    re(r"(?i)\bvs\b|,|;", &SPLIT)
        .split(raw_line)
        // The own team's own half of "<own> 2963 vs <enemy> 1514" is not an enemy.
        .filter(|part| own_key.is_empty() || !normalize_key(part).contains(&own_key))
        .map(|part| clean_team_name(&re(r"\d[\d\s,.]*$", &TRAIL).replace(part, ""), own_team_name))
        .filter(|name| !name.is_empty() && normalize_key(name) != own_key)
        .collect()
}

pub fn session_opponent_teams(session: &SpreadsheetSession) -> Vec<String> {
    let own_team_name = if !session.metadata.own_team_name.is_empty() {
        session.metadata.own_team_name.clone()
    } else if !session.team_name.is_empty() {
        session.team_name.clone()
    } else {
        session.team_id.clone()
    };
    let own_key = normalize_key(&own_team_name);

    let mut explicit: Vec<String> = Vec::new();
    for team in &session.metadata.teams {
        let label = clean_team_name(
            team.get("label").or_else(|| team.get("name")).or_else(|| team.get("teamName")).and_then(|v| v.as_str()).unwrap_or(""),
            &own_team_name,
        );
        let kind = normalize_team_type(team.get("teamType").or_else(|| team.get("type")).or_else(|| team.get("classification")).and_then(|v| v.as_str()).unwrap_or(""));
        if label.is_empty() {
            continue;
        }
        if kind == "opponent" || (kind != "own" && normalize_key(&label) != own_key) {
            explicit.push(label);
        }
    }

    let from_players: Vec<String> = opponent_players(session)
        .iter()
        .map(|p| clean_team_name(&p.team_label, &own_team_name))
        .filter(|label| {
            let key = normalize_key(label);
            !key.is_empty() && key != own_key && !["opponent", "unknown", "blue", "none"].contains(&key.as_str())
        })
        .collect();
    let raw_line = session.metadata.team_scores.as_ref().map(|t| t.raw_line.clone()).unwrap_or_default();
    let from_line = names_from_score_line(&raw_line, &own_team_name);

    let names = unique_names(explicit.into_iter().chain(from_players).chain(from_line));
    if names.is_empty() {
        vec!["Opponent".to_string()]
    } else {
        names
    }
}

pub fn team_score_from_metadata(session: &SpreadsheetSession, own: bool) -> i64 {
    session
        .metadata
        .team_scores
        .as_ref()
        .and_then(|t| if own { t.own } else { t.opponent })
        .filter(|v| v.is_finite())
        .map(|v| v as i64)
        .unwrap_or(0)
}
