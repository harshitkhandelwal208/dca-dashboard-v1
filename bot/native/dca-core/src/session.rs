//! Turning OCR'd standings pages into a team-event session: merging pages, own/opponent classification,
//! event points, summary stats and staff corrections. (Port of the rule-based half of `raceOcr.js` /
//! `spreadsheetManager.js`; the Gemini half is replaced by the local readers in `standings`.)

use crate::metrics::{normalize_key, player_key};
use crate::standings::{clean_team_label, StandingsPage};
use dca_state::models::*;
use serde_json::json;
use std::collections::BTreeMap;

/// Points per rank in an HCR2 team event, read off the game's own standings screens (the number left of each rank):
/// ranks 1-96 of a real 96-player event, whose two team totals (3312 + 1210) equal the sum of this table over those
/// ranks. Ranks 97-100 (a team has at most 50 drivers) continue the run of single points.
pub const HCR2_TEAM_EVENT_POINTS_BY_RANK: &[i64] = &[
    300, 280, 262, 244, 228, 213, 198, 185, 173, 161, 150, 140, 131, 122, 114, 107, 99, 93, 87, 81, 75, 70, 66, 61, 57, 54, 50, 47, 44, 41, 38, 35, 33, 31, 29, 27, 25, 24, 22, 21, 19, 18, 17, 16, 15,
    14, 13, 12, 11, 11, 10, 9, 9, 8, 8, 7, 7, 6, 6, 6, 5, 5, 5, 4, 4, 4, 4, 3, 3, 3, 3, 3, 3, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
];

pub fn event_points_for_rank(rank: u32) -> i64 {
    HCR2_TEAM_EVENT_POINTS_BY_RANK.get((rank as usize).wrapping_sub(1)).copied().unwrap_or(0)
}

/// What the staff configured for the team this session belongs to.
#[derive(Clone, Debug, Default)]
pub struct TeamContext {
    pub name: String,
    pub own_team_aliases: Vec<String>,
    pub own_player_aliases: Vec<String>,
}

#[derive(Clone, Debug, Default)]
pub struct Parsed {
    pub metadata: Metadata,
    pub players: Vec<Player>,
    pub stats: Stats,
    pub raw_text: String,
}

fn alias_matches(text: &str, aliases: &[String]) -> Option<String> {
    let n = normalize_key(text);
    if n.is_empty() {
        return None;
    }
    aliases
        .iter()
        .find(|a| {
            let k = normalize_key(a);
            !k.is_empty() && n.contains(&k)
        })
        .cloned()
}

fn player_alias_matches(name: &str, aliases: &[String]) -> Option<String> {
    let normalized = normalize_key(name);
    if normalized.is_empty() {
        return None;
    }
    let tokens: Vec<&str> = normalized.split(' ').filter(|t| !t.is_empty()).collect();
    let squashed: String = normalized.split(' ').collect();
    aliases
        .iter()
        .find(|alias| {
            let a = normalize_key(alias);
            if a.is_empty() {
                return false;
            }
            if normalized == a {
                return true;
            }
            if a.len() < 3 {
                return false;
            }
            if tokens.contains(&a.as_str()) {
                return true;
            }
            squashed.ends_with(&a.split(' ').collect::<String>())
        })
        .cloned()
}

fn has_dc_prefix(name: &str) -> bool {
    let lower = name.trim().to_lowercase();
    let squashed: String = lower.chars().filter(|c| !c.is_whitespace()).collect();
    ["dc|", "dc]", "[dc]", "dca|", "dca]", "[dca]", "|dc|"].iter().any(|p| squashed.starts_with(p))
}

/// Own/opponent for one standings row. A confidently sampled band colour wins, exactly like the old sampler did.
fn classify(row_band: u8, band_conf: f32, name: &str, team: &TeamContext) -> (&'static str, String) {
    let aliases: Vec<String> = std::iter::once(team.name.clone()).chain(team.own_team_aliases.iter().cloned()).filter(|a| !a.is_empty()).collect();
    if row_band != 0 && band_conf >= 0.58 {
        let own = row_band == 1;
        return (if own { "own" } else { "opponent" }, format!("image-row-color:{}", if own { "yellow" } else { "blue" }));
    }
    if let Some(a) = alias_matches(name, &aliases) {
        return ("own", format!("alias:{a}"));
    }
    if let Some(p) = player_alias_matches(name, &team.own_player_aliases) {
        return ("own", format!("known-player:{p}"));
    }
    if has_dc_prefix(name) {
        return ("own", "name-prefix".into());
    }
    match row_band {
        1 => ("own", "row-color:yellow".into()),
        2 => ("opponent", "row-color:blue".into()),
        _ => ("opponent", "default-opponent".into()),
    }
}

/// Merge several pages into one event.
pub fn parse_pages(pages: &[StandingsPage], team: &TeamContext, known_team_names: &[String]) -> Parsed {
    let mut ranked: BTreeMap<u32, (Player, f32)> = BTreeMap::new();
    let mut unranked: Vec<(Player, f32)> = Vec::new();

    // Podium rows (no score) only confirm names; they never replace a scored row.
    let mut podium_names: BTreeMap<u32, (String, f32)> = BTreeMap::new();
    for (page_index, page) in pages.iter().enumerate() {
        for row in &page.rows {
            if row.name.trim().is_empty() {
                continue;
            }
            if row.score.is_none() && row.raw.starts_with("podium ") {
                if row.rank > 0 && podium_names.get(&row.rank).is_none_or(|(_, c)| *c < row.name_confidence) {
                    podium_names.insert(row.rank, (row.name.clone(), row.name_confidence));
                }
                continue;
            }
            let (kind, source) = classify(row.band, row.band_confidence, &row.name, team);
            let own = kind == "own";
            let player = Player {
                rank: row.rank,
                placement: row.rank,
                player_name: row.name.chars().take(80).collect(),
                team_label: if own { team.name.clone() } else { "Opponent".into() },
                team_type: kind.into(),
                team_color: if own { "yellow".into() } else { "blue".into() },
                row_color: if row.band == 1 {
                    "yellow".into()
                } else if row.band == 2 {
                    "blue".into()
                } else {
                    String::new()
                },
                row_color_confidence: Some(row.band_confidence as f64),
                row_color_source: "image-sampler".into(),
                points: Some(event_points_for_rank(row.rank)),
                points_source: "rank-table".into(),
                score: row.score.map(|s| s as i64),
                source_image: format!("image-{}", page_index + 1),
                raw_line: row.raw.clone(),
                classification_source: source,
                confidence: Some(row.name_confidence as f64),
                flagged: row.name_flagged || row.name_confidence < 0.6,
                ..Default::default()
            };
            if row.rank == 0 || row.rank > 100 {
                unranked.push((player, row.name_confidence));
                continue;
            }
            let quality = row.name_confidence + row.band_confidence * 0.1 + if row.score.is_some() { 0.05 } else { 0.0 };
            match ranked.get(&row.rank) {
                Some((_, q)) if *q >= quality => {}
                _ => {
                    ranked.insert(row.rank, (player, quality));
                }
            }
        }
    }
    // The podium shows the top names large and clean: prefer them when they are the same name as the table's reading.
    for (rank, (name, confidence)) in &podium_names {
        if let Some((player, _)) = ranked.get_mut(rank) {
            if *confidence >= 0.8 && *confidence + 0.02 >= player.confidence.unwrap_or(0.0) as f32 && similar_names(&player.player_name, name) {
                player.player_name = name.chars().take(80).collect();
                player.confidence = Some(*confidence as f64);
                player.flagged = false;
            }
        }
    }

    // Rows from pages whose rank column was cut off: place them between ranked neighbours by score.
    unranked.sort_by_key(|a| std::cmp::Reverse(a.0.score.unwrap_or(0)));
    for (mut player, quality) in unranked {
        let Some(score) = player.score else { continue };
        let above = ranked.values().filter(|(p, _)| p.score.is_some_and(|s| s >= score)).map(|(p, _)| p.rank).max();
        let below = ranked.values().filter(|(p, _)| p.score.is_some_and(|s| s <= score)).map(|(p, _)| p.rank).min();
        let candidate = match (above, below) {
            (Some(a), Some(b)) if b > a + 1 => (a + 1..b).find(|r| !ranked.contains_key(r)),
            (Some(a), None) => (a + 1..=100).find(|r| !ranked.contains_key(r)),
            (None, Some(b)) if b > 1 => (1..b).rev().find(|r| !ranked.contains_key(r)),
            _ => None,
        };
        if let Some(rank) = candidate {
            player.rank = rank;
            player.placement = rank;
            player.points = Some(event_points_for_rank(rank));
            player.flagged = true;
            ranked.insert(rank, (player, quality));
        }
    }

    let mut players: Vec<Player> = ranked.into_values().map(|(p, _)| p).collect();

    // Header metadata from the first page that has any.
    let header = pages.iter().map(|p| &p.header).find(|h| !h.title.is_empty() || !h.left_team.is_empty());
    let title = header.map(|h| h.title.clone()).filter(|t| !t.is_empty()).unwrap_or_else(|| "Team Event".into());
    let own_aliases: Vec<String> = std::iter::once(team.name.clone()).chain(team.own_team_aliases.iter().cloned()).filter(|a| !a.is_empty()).collect();

    let mut metadata = Metadata {
        title: title.clone(),
        event_name: title,
        game: "Hill Climb Racing 2".into(),
        own_team_name: team.name.clone(),
        extraction_type: "local-ocr".into(),
        screenshot_types: vec!["standings".into()],
        ..Default::default()
    };

    if let Some(h) = header {
        let left = clean_team_label(&h.left_team, known_team_names);
        let right = clean_team_label(&h.right_team, known_team_names);
        let left_own = alias_matches(&left, &own_aliases).is_some();
        let right_own = alias_matches(&right, &own_aliases).is_some();
        // The reading player's own team is drawn on the left unless the right-hand name is clearly ours.
        let swap = right_own && !left_own;
        let (own_name, own_score, opp_name, opp_score) = if swap { (right, h.right_score, left, h.left_score) } else { (left, h.left_score, right, h.right_score) };
        if own_score.is_some() || opp_score.is_some() {
            metadata.team_scores = Some(TeamScores {
                own: own_score.map(|v| v as f64),
                opponent: opp_score.map(|v| v as f64),
                raw_line: format!("{} {} vs {} {}", own_name, own_score.map(|v| v.to_string()).unwrap_or_default(), opp_name, opp_score.map(|v| v.to_string()).unwrap_or_default())
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" "),
            });
        }
        if !own_name.is_empty() {
            metadata.teams.push(json!({ "label": own_name, "teamType": "own", "score": own_score, "side": if swap { "right" } else { "left" } }));
        }
        if !opp_name.is_empty() {
            metadata.teams.push(json!({ "label": opp_name, "teamType": "opponent", "score": opp_score, "side": if swap { "left" } else { "right" } }));
            metadata.opponent_team_name = opp_name.clone();
            for p in players.iter_mut().filter(|p| p.team_type != "own") {
                p.team_label = opp_name.clone();
            }
        }
    }

    let stats = summarize(&players);
    Parsed { metadata, players, stats, raw_text: pages.iter().enumerate().map(|(i, p)| format!("--- Image {} ---\n{}\n{}", i + 1, p.raw_text, p.header_raw())).collect::<Vec<_>>().join("\n\n") }
}

impl StandingsPage {
    fn header_raw(&self) -> String {
        format!("title: {} | left: {} {:?} | right: {} {:?}", self.header.title, self.header.left_team, self.header.left_score, self.header.right_team, self.header.right_score)
    }
}

fn sum_i(items: &[&Player], f: impl Fn(&Player) -> i64) -> i64 {
    items.iter().map(|p| f(p)).sum()
}

fn average_rank(items: &[&Player]) -> Option<f64> {
    if items.is_empty() {
        return None;
    }
    let sum: f64 = items.iter().map(|p| p.rank as f64).sum();
    Some(((sum / items.len() as f64) * 100.0).round() / 100.0)
}

/// Port of `summarize`.
pub fn summarize(players: &[Player]) -> Stats {
    let mut sorted: Vec<&Player> = players.iter().collect();
    sorted.sort_by(|a, b| a.rank.cmp(&b.rank).then_with(|| a.player_name.cmp(&b.player_name)));
    let own: Vec<&Player> = sorted.iter().copied().filter(|p| p.team_type == "own").collect();
    let opponents: Vec<&Player> = sorted.iter().copied().filter(|p| p.team_type != "own").collect();
    let top_opponent_rank = opponents.iter().map(|p| p.rank).filter(|r| *r > 0).min();
    let kab_players: Vec<String> = match top_opponent_rank {
        Some(t) => own.iter().filter(|p| p.rank < t).map(|p| p.player_name.clone()).collect(),
        None => Vec::new(),
    };
    let bucket_defs: [(&str, &str, u32, u32); 4] = [("podium", "1-3", 1, 3), ("top10", "4-10", 4, 10), ("mid", "11-20", 11, 20), ("lower", "21+", 21, 999)];
    let buckets = bucket_defs
        .iter()
        .map(|(id, label, min, max)| Bucket {
            id: id.to_string(),
            label: label.to_string(),
            min: *min,
            max: *max,
            own: own.iter().filter(|p| p.rank >= *min && p.rank <= *max).count() as u32,
            opponents: opponents.iter().filter(|p| p.rank >= *min && p.rank <= *max).count() as u32,
        })
        .collect();

    Stats {
        total_players: sorted.len() as u32,
        own_players: own.len() as u32,
        opponents: opponents.len() as u32,
        own_average_rank: average_rank(&own),
        opponent_average_rank: average_rank(&opponents),
        own_points: sum_i(&own, |p| p.points.unwrap_or(0)),
        opponent_points: sum_i(&opponents, |p| p.points.unwrap_or(0)),
        own_score: sum_i(&own, |p| p.score.unwrap_or(0)),
        opponent_score: sum_i(&opponents, |p| p.score.unwrap_or(0)),
        podium: sorted.iter().filter(|p| p.rank <= 3).map(|p| (*p).clone()).collect(),
        own_top10: own.iter().filter(|p| p.rank <= 10).count() as u32,
        opponent_top10: opponents.iter().filter(|p| p.rank <= 10).count() as u32,
        top_opponent_rank,
        kab_count: kab_players.len() as u32,
        kab_players,
        buckets,
        opponents_below_by_player: own
            .iter()
            .map(|p| OpponentsBelow { player_name: p.player_name.clone(), rank: p.rank, opponents_below: opponents.iter().filter(|o| o.rank > p.rank).count() as u32 })
            .collect(),
    }
}

fn parse_score_value(value: &str) -> Option<i64> {
    let text: String = value.chars().filter(|c| !c.is_whitespace()).collect();
    if text.is_empty() {
        return None;
    }
    let digits: String = text.chars().filter(|c| c.is_ascii_digit()).collect();
    digits.parse().ok()
}

/// Staff corrections (`/spreadsheets correct-*`), applied in order on top of a fresh parse.
pub fn apply_corrections(mut parsed: Parsed, corrections: &[Correction]) -> Parsed {
    for c in corrections {
        if c.field == "event_name" {
            let value: String = c.value.chars().take(120).collect();
            parsed.metadata.title = if value.is_empty() { parsed.metadata.title.clone() } else { value };
            if parsed.metadata.title.is_empty() {
                parsed.metadata.title = "Team Event".into();
            }
            parsed.metadata.event_name = parsed.metadata.title.clone();
            continue;
        }
        let row = c.row;
        let index = parsed.players.iter().position(|p| p.rank as i64 == row).or_else(|| if row >= 1 && (row as usize) <= parsed.players.len() { Some(row as usize - 1) } else { None });
        let Some(index) = index else { continue };
        let player = &mut parsed.players[index];
        match c.field.as_str() {
            "player_name" => player.player_name = c.value.chars().take(80).collect(),
            "team_name" => player.team_label = c.value.chars().take(80).collect(),
            "placement" => {
                if let Ok(next) = c.value.trim().parse::<u32>() {
                    if next > 0 && next <= 80 {
                        player.rank = next;
                        player.placement = next;
                    }
                }
            }
            "points" => player.points = parse_score_value(&c.value),
            "score" => player.score = parse_score_value(&c.value),
            "team_type" => {
                let own = matches!(c.value.to_lowercase().as_str(), "own" | "yellow" | "team");
                player.team_type = if own { "own".into() } else { "opponent".into() };
                player.team_color = if own { "yellow".into() } else { "blue".into() };
                if !own {
                    player.team_label = "Opponent".into();
                }
                player.classification_source = "staff-correction".into();
            }
            _ => {}
        }
    }
    parsed.players.sort_by(|a, b| a.rank.cmp(&b.rank).then_with(|| a.player_name.cmp(&b.player_name)));
    parsed.stats = summarize(&parsed.players);
    parsed
}

/// Similarity-tolerant comparison of two event names (OCR can drop a letter between sessions of one event).
pub fn same_event_name(a: &str, b: &str) -> bool {
    let (ka, kb) = (normalize_key(a).replace(' ', ""), normalize_key(b).replace(' ', ""));
    if ka.is_empty() || kb.is_empty() {
        return ka == kb;
    }
    if ka == kb {
        return true;
    }
    let (la, lb) = (ka.chars().count(), kb.chars().count());
    if la.abs_diff(lb) > 3 {
        return false;
    }
    let dist = levenshtein(&ka, &kb);
    dist as f32 <= (la.max(lb) as f32 * 0.2).floor().max(1.0)
}

fn levenshtein(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for i in 1..=a.len() {
        let mut cur = vec![i; b.len() + 1];
        for j in 1..=b.len() {
            let cost = if a[i - 1] == b[j - 1] { 0 } else { 1 };
            cur[j] = (prev[j] + 1).min(cur[j - 1] + 1).min(prev[j - 1] + cost);
        }
        prev = cur;
    }
    prev[b.len()]
}

/// Two readings of one name (confusable letters folded, a couple of edits allowed).
fn similar_names(a: &str, b: &str) -> bool {
    let fold = |t: &str| -> String {
        t.chars()
            .filter(|c| c.is_alphanumeric())
            .flat_map(|c| c.to_lowercase())
            .map(|c| match c {
                'l' | 'i' | '1' | '|' | 'j' => 'i',
                '0' => 'o',
                '5' => 's',
                other => other,
            })
            .collect()
    };
    let (ka, kb) = (fold(a), fold(b));
    if ka.is_empty() || kb.is_empty() {
        return false;
    }
    let longest = ka.chars().count().max(kb.chars().count());
    levenshtein(&ka, &kb) <= (longest / 3).max(2)
}

/// Convenience used by tests / the bot: does the player key of a roster name match a row.
pub fn same_player(a: &str, b: &str) -> bool {
    let (ka, kb) = (player_key(a), player_key(b));
    !ka.is_empty() && ka == kb
}

// ------------------------------------------------------------------------------ stored readings

pub fn to_reading(page: &StandingsPage, image: &str, method: &str) -> PageReading {
    PageReading {
        image: image.to_string(),
        method: method.to_string(),
        rows: page
            .rows
            .iter()
            .map(|r| PageRow {
                rank: r.rank,
                rank_read: r.rank_read,
                name: r.name.clone(),
                name_confidence: r.name_confidence,
                name_flagged: r.name_flagged,
                score: r.score,
                band: r.band,
                band_confidence: r.band_confidence,
                gold_text: r.gold_text,
                cy: r.cy,
                raw: r.raw.clone(),
            })
            .collect(),
        header: PageHeader {
            title: page.header.title.clone(),
            left_team: page.header.left_team.clone(),
            right_team: page.header.right_team.clone(),
            left_score: page.header.left_score,
            right_score: page.header.right_score,
        },
        raw_text: page.raw_text.clone(),
        error: String::new(),
    }
}

pub fn from_reading(reading: &PageReading) -> StandingsPage {
    use crate::standings::{StandingsHeader, StandingsRow};
    StandingsPage {
        rows: reading
            .rows
            .iter()
            .map(|r| StandingsRow {
                rank: r.rank,
                rank_read: r.rank_read,
                rank_options: Vec::new(),
                name: r.name.clone(),
                name_confidence: r.name_confidence,
                name_flagged: r.name_flagged,
                score: r.score,
                band: r.band,
                band_confidence: r.band_confidence,
                gold_text: r.gold_text,
                cy: r.cy,
                raw: r.raw.clone(),
            })
            .collect(),
        header: StandingsHeader {
            title: reading.header.title.clone(),
            left_team: reading.header.left_team.clone(),
            right_team: reading.header.right_team.clone(),
            left_score: reading.header.left_score,
            right_score: reading.header.right_score,
        },
        raw_text: reading.raw_text.clone(),
        has_final_standings_banner: false,
        deskew_degrees: 0.0,
    }
}
