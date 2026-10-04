//! Reader for the Hill Climb Racing 2 driver's licence (player profile) screen.

use crate::imaging::Rgb;
use crate::names::NameReader;
use crate::ocr::TextLine;
use crate::standings::{is_numeric_token, norm, numeric_value};

#[derive(Clone, Debug, Default)]
pub struct LicenceReading {
    pub name: String,
    pub name_confidence: f32,
    pub team: String,
    pub garage_power: Option<u64>,
    pub cup_points: Option<u64>,
    pub season_points: Option<u64>,
    pub adventurer_rank: String,
    pub rank_points: Option<u64>,
    pub best_season: Option<u64>,
    pub best_win_streak: Option<u64>,
    pub achievements: String,
    /// How many of the licence screen's fixed labels were found (>= 2 means "this is a licence").
    pub anchors: u32,
    pub raw_text: String,
}

const LABELS: [(&str, &[&str]); 8] = [
    ("garage", &["garagepower", "garagepowe"]),
    ("cup", &["cuppoints", "cuppoint"]),
    ("season", &["seasonpoints", "seasonpoint"]),
    ("adventurer", &["adventurerrank", "adventurer"]),
    ("bestseason", &["bestseason"]),
    ("streak", &["bestwinstreak", "winstreak"]),
    ("achievements", &["achievements", "achievement"]),
    ("copyid", &["copymyid", "copyid"]),
];

fn label<'a>(lines: &'a [TextLine], key: &str) -> Option<&'a TextLine> {
    let options = LABELS.iter().find(|(k, _)| *k == key)?.1;
    lines
        .iter()
        .filter(|l| {
            let n = norm(&l.text);
            n.len() >= 5 && options.iter().any(|o| n == *o || (n.len() <= o.len() + 3 && n.contains(o)))
        })
        .max_by(|a, b| a.confidence.partial_cmp(&b.confidence).unwrap())
}

/// The numeric line closest below a label, in (roughly) the same column.
fn value_below<'a>(lines: &'a [TextLine], label: &TextLine, min_digits: usize, reach: f32) -> Option<&'a TextLine> {
    lines
        .iter()
        .filter(|l| {
            l.cy() > label.cy() + label.h() * 0.3
                && l.cy() < label.cy() + label.h() * reach
                && (l.cx() - label.cx()).abs() < (label.w() * 0.9).max(label.h() * 3.0)
                && is_numeric_token(&l.text)
                && l.text.chars().filter(|c| c.is_ascii_digit()).count() >= min_digits
        })
        .min_by(|a, b| a.cy().partial_cmp(&b.cy()).unwrap())
}

pub fn read_licence(names: &NameReader, page: &Rgb, lines: &[TextLine], known_teams: &[String]) -> LicenceReading {
    let mut out = LicenceReading {
        raw_text: lines.iter().map(|l| l.text.clone()).collect::<Vec<_>>().join("\n"),
        anchors: LABELS.iter().filter(|(k, _)| label(lines, k).is_some()).count() as u32,
        ..Default::default()
    };

    if let Some(l) = label(lines, "garage") {
        out.garage_power = value_below(lines, l, 3, 4.5).and_then(|v| numeric_value(&v.text)).filter(|v| (100..=30_000).contains(v));
    }
    if let Some(l) = label(lines, "cup") {
        out.cup_points = value_below(lines, l, 3, 7.0).and_then(|v| numeric_value(&v.text));
    }
    if let Some(l) = label(lines, "season") {
        out.season_points = value_below(lines, l, 3, 7.0).and_then(|v| numeric_value(&v.text));
    }
    if let Some(l) = label(lines, "bestseason") {
        out.best_season = value_below(lines, l, 3, 4.0).and_then(|v| numeric_value(&v.text));
    }
    if let Some(l) = label(lines, "streak") {
        out.best_win_streak = value_below(lines, l, 1, 4.0).and_then(|v| numeric_value(&v.text));
    }
    if let Some(l) = label(lines, "achievements") {
        out.achievements = lines
            .iter()
            .filter(|x| x.cy() > l.cy() && x.cy() < l.cy() + l.h() * 4.0 && (x.cx() - l.cx()).abs() < l.w())
            .find(|x| x.text.contains('/'))
            .map(|x| x.text.trim().to_string())
            .unwrap_or_default();
    }
    if let Some(l) = label(lines, "adventurer") {
        // League name lines ("VANGUARD" / "SCOUT") then the points number, all in the label's column.
        // ...down to the next label row (best season / win streak), whichever layout spaces them how far apart.
        let floor = ["bestseason", "streak"].iter().filter_map(|k| label(lines, k)).map(|n| n.cy()).filter(|cy| *cy > l.cy() + l.h() * 3.0).fold(f32::MAX, f32::min);
        let limit = floor.min(l.cy() + l.h() * 12.0) - l.h() * 0.3;
        let mut column: Vec<&TextLine> = lines.iter().filter(|x| x.cy() > l.cy() + l.h() * 1.2 && x.cy() < limit && (x.cx() - l.cx()).abs() < l.w() * 0.7 && !x.text.trim().is_empty()).collect();
        column.sort_by(|a, b| a.cy().partial_cmp(&b.cy()).unwrap());
        let words: Vec<String> = column.iter().filter(|x| !is_numeric_token(&x.text) && norm(&x.text) != "rank").map(|x| x.text.trim().to_string()).collect();
        out.adventurer_rank = words.join(" ");
        out.rank_points = column.iter().find(|x| is_numeric_token(&x.text) && x.text.chars().filter(|c| c.is_ascii_digit()).count() >= 3).and_then(|x| numeric_value(&x.text));
    }

    // Name and team: the two text lines in the profile card above the stat labels.
    let stat_labels: Vec<&TextLine> = ["cup", "season", "adventurer"].iter().filter_map(|k| label(lines, k)).collect();
    if !stat_labels.is_empty() {
        let top = stat_labels.iter().map(|l| l.cy()).fold(f32::MAX, f32::min);
        let lh = stat_labels.iter().map(|l| l.h()).fold(0.0, f32::max).max(8.0);
        let x_min = stat_labels.iter().map(|l| l.x0).fold(f32::MAX, f32::min) - lh * 2.0;
        let x_max = stat_labels.iter().map(|l| l.x1).fold(f32::MIN, f32::max) + lh * 2.0;
        // The two lines closest above the stat labels: the team bar, then the player name above it.
        let mut head: Vec<&TextLine> = lines
            .iter()
            .filter(|l| {
                l.cy() < top - lh * 0.5
                    && l.cy() > top - lh * 7.5
                    && l.cx() > x_min
                    && l.cx() < x_max
                    && l.confidence >= 0.5
                    && l.text.chars().filter(|c| c.is_alphanumeric()).count() >= 2
                    && l.h() > lh * 0.7
                    && l.w() > lh * 1.5
            })
            .collect();
        head.sort_by(|a, b| b.cy().partial_cmp(&a.cy()).unwrap());

        let is_team = |l: &TextLine| {
            let n = norm(&l.text);
            known_teams.iter().any(|t| {
                let k = norm(t);
                !k.is_empty() && (n.starts_with(&k) || n == k)
            })
        };
        let team_line = head.iter().copied().find(|l| is_team(l)).or_else(|| head.first().copied());
        let name_line = match team_line {
            Some(t) => head.iter().copied().find(|l| l.cy() < t.cy() - 2.0),
            None => None,
        }
        .or_else(|| head.iter().copied().find(|l| team_line.is_none_or(|t| !std::ptr::eq(*l, t))));
        if let Some(l) = name_line {
            let read = names.read(page, [l.x0, l.y0, l.x1, l.y1], &l.text, 0.0, None, None);
            out.name = read.text;
            out.name_confidence = read.confidence;
        }
        if let Some(l) = team_line {
            out.team = l.text.trim().to_string();
        }
    }
    out
}

/// How many of the licence screen's fixed labels are in this text (>= 2 means "this is a licence").
pub fn anchor_count(lines: &[TextLine]) -> u32 {
    LABELS.iter().filter(|(k, _)| label(lines, k).is_some()).count() as u32
}
