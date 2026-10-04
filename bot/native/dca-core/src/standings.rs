//! Reader for the Hill Climb Racing 2 team-event "FINAL STANDINGS" screen.
//!
//! The reader is layout independent: it works from where the OCR found text (rank / name / score columns), not
//! from fixed pixel positions, so cropped screenshots, tablets, ultra-wide phones and photographs of devices all
//! go through the same code. Own-team rows are the yellow bands, opponent rows the blue ones.

use crate::imaging::*;
use crate::names::{NameReader, ReadName};
use crate::ocr::{crop_quad, Ocr, Pt, Script, TextLine};

#[derive(Clone, Debug)]
pub struct StandingsRow {
    pub rank: u32,
    pub rank_read: bool,
    /// Possible readings of the rank token when the detector glued the points badge to it ("3001." = 300 + 1).
    pub rank_options: Vec<u32>,
    pub name: String,
    pub name_confidence: f32,
    pub name_flagged: bool,
    pub score: Option<u64>,
    /// `BAND_YELLOW` (own team) / `BAND_BLUE` (opponent) / `BAND_NONE` when the colour could not be told.
    pub band: u8,
    pub band_confidence: f32,
    pub gold_text: bool,
    pub cy: f32,
    pub raw: String,
}

#[derive(Clone, Debug, Default)]
pub struct StandingsHeader {
    pub title: String,
    pub left_team: String,
    pub right_team: String,
    pub left_score: Option<u64>,
    pub right_score: Option<u64>,
}

#[derive(Clone, Debug)]
pub struct StandingsPage {
    pub rows: Vec<StandingsRow>,
    pub header: StandingsHeader,
    /// Every OCR line of the screenshot (for the stored `rawOcrText`).
    pub raw_text: String,
    pub has_final_standings_banner: bool,
    /// Rotation (degrees) that was applied to straighten the screenshot.
    pub deskew_degrees: f32,
}

pub const MAX_PAGE_HEIGHT: u32 = 1600;

// -------------------------------------------------------------------------------------------- helpers

pub fn norm(text: &str) -> String {
    text.chars().filter(|c| c.is_alphanumeric()).flat_map(|c| c.to_lowercase()).collect()
}

/// Digits of a numeric-looking token ("24 389", "24,389", "24389").
pub fn numeric_value(text: &str) -> Option<u64> {
    let cleaned: String = text
        .chars()
        .map(|c| match c {
            'O' | 'o' | 'D' => '0',
            'l' | 'I' | '|' | 'i' => '1',
            'S' | 's' => '5',
            'B' => '8',
            other => other,
        })
        .filter(|c| !matches!(c, ' ' | ',' | '.' | '\'' | '\u{a0}' | '\u{2009}'))
        .collect();
    if cleaned.is_empty() || !cleaned.chars().all(|c| c.is_ascii_digit()) || cleaned.len() > 12 {
        return None;
    }
    cleaned.parse().ok()
}

/// Is this token numeric (not just containing a digit)? Letters that OCR confuses with digits only count when the
/// token is short and mostly digits already.
pub fn is_numeric_token(text: &str) -> bool {
    let t = text.trim();
    if t.is_empty() {
        return false;
    }
    let digits = t.chars().filter(|c| c.is_ascii_digit()).count();
    let others = t.chars().filter(|c| !c.is_ascii_digit() && !matches!(c, ' ' | ',' | '.' | '\'')).count();
    digits >= 1 && others == 0
}

fn median(values: &mut Vec<f32>) -> f32 {
    if values.is_empty() {
        return 0.0;
    }
    values.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let mid = values.len() / 2;
    if values.len() % 2 == 1 {
        values[mid]
    } else {
        (values[mid - 1] + values[mid]) / 2.0
    }
}

/// Count yellow / blue band pixels in a region (hue based, tolerant to the darker overlay boxes and to exposure).
pub fn band_votes(img: &Rgb, x0: f32, y0: f32, x1: f32, y1: f32) -> (u32, u32) {
    let rx0 = x0.max(0.0) as u32;
    let ry0 = y0.max(0.0) as u32;
    let rx1 = (x1.min(img.w as f32) as u32).max(rx0);
    let ry1 = (y1.min(img.h as f32) as u32).max(ry0);
    let (mut yellow, mut blue) = (0u32, 0u32);
    let step_x = ((rx1 - rx0) / 80).max(1);
    let step_y = ((ry1 - ry0) / 24).max(1);
    let mut y = ry0;
    while y < ry1 {
        let mut x = rx0;
        while x < rx1 {
            let [r, g, b] = img.px(x, y);
            match loose_band(r, g, b) {
                BAND_YELLOW => yellow += 1,
                BAND_BLUE => blue += 1,
                _ => {}
            }
            x += step_x;
        }
        y += step_y;
    }
    (yellow, blue)
}

fn loose_band(r: u8, g: u8, b: u8) -> u8 {
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    if max < 60 {
        return BAND_NONE;
    }
    let delta = (max - min) as f32;
    let sat = delta / max as f32;
    if sat < 0.22 {
        return BAND_NONE;
    }
    let (rf, gf, bf) = (r as f32, g as f32, b as f32);
    let mut hue = if max == r {
        60.0 * (((gf - bf) / delta) % 6.0)
    } else if max == g {
        60.0 * ((bf - rf) / delta + 2.0)
    } else {
        60.0 * ((rf - gf) / delta + 4.0)
    };
    if hue < 0.0 {
        hue += 360.0;
    }
    if (34.0..=75.0).contains(&hue) && sat < 0.75 {
        BAND_YELLOW
    } else if (190.0..=240.0).contains(&hue) {
        BAND_BLUE
    } else {
        BAND_NONE
    }
}

/// Fit the slope (dx/dy) of a column of points, used to estimate how much a photo is rotated.
fn column_slope(points: &[(f32, f32)]) -> f32 {
    let n = points.len() as f32;
    if n < 3.0 {
        return 0.0;
    }
    let (sx, sy): (f32, f32) = points.iter().fold((0.0, 0.0), |a, p| (a.0 + p.0, a.1 + p.1));
    let (mx, my) = (sx / n, sy / n);
    let (mut num, mut den) = (0.0, 0.0);
    for (x, y) in points {
        num += (y - my) * (x - mx);
        den += (y - my) * (y - my);
    }
    if den <= 0.0 {
        0.0
    } else {
        num / den
    }
}

// ------------------------------------------------------------------------------------------ the reader

struct Tokens<'a> {
    lines: &'a [TextLine],
}

/// Rows found in an OCR line list: `(score_line, row_lines)`.
struct RowGroup {
    score: usize,
    members: Vec<usize>,
    cy: f32,
    pitch: f32,
}

fn find_rows(lines: &[TextLine]) -> (Vec<RowGroup>, f32) {
    // Score candidates: numeric tokens with 3-7 digits.
    let candidates: Vec<usize> = lines
        .iter()
        .enumerate()
        .filter(|(_, l)| l.confidence >= 0.35 && is_numeric_token(&l.text) && (3..=7).contains(&l.text.chars().filter(|c| c.is_ascii_digit()).count()))
        .map(|(i, _)| i)
        .collect();
    if candidates.len() < 2 {
        return (Vec::new(), 0.0);
    }

    // The score column: the largest group of candidates sharing a right edge (allowing for a little tilt).
    let mut heights: Vec<f32> = candidates.iter().map(|&i| lines[i].h()).collect();
    let typical_h = median(&mut heights).max(8.0);
    let mut best: Vec<usize> = Vec::new();
    for &anchor in &candidates {
        let group: Vec<usize> = candidates
            .iter()
            .copied()
            .filter(|&i| (lines[i].x1 - lines[anchor].x1).abs() <= typical_h * 1.6)
            .collect();
        if group.len() > best.len() {
            best = group;
        }
    }
    if best.len() < 2 {
        return (Vec::new(), 0.0);
    }
    best.sort_by(|&a, &b| lines[a].cy().partial_cmp(&lines[b].cy()).unwrap());

    // Row pitch from consecutive score centres (ignoring jumps larger than 2.5 rows: skipped/cropped rows).
    let mut diffs: Vec<f32> = best.windows(2).map(|w| lines[w[1]].cy() - lines[w[0]].cy()).filter(|d| *d > typical_h * 0.8).collect();
    let rough = median(&mut diffs.clone());
    let pitch = if rough > 0.0 {
        let mut near: Vec<f32> = diffs.drain(..).filter(|d| *d < rough * 1.5).collect();
        median(&mut near)
    } else {
        typical_h * 1.9
    };
    if pitch <= 0.0 {
        return (Vec::new(), 0.0);
    }

    // Drop score-column members that do not sit on the row grid (e.g. a stray number in the same column).
    let mut rows: Vec<RowGroup> = Vec::new();
    for &i in &best {
        rows.push(RowGroup { score: i, members: Vec::new(), cy: lines[i].cy(), pitch });
    }
    (rows, pitch)
}

pub fn read_standings(ocr: &Ocr, names: &NameReader, source: &Rgb, first_lines: Option<Vec<TextLine>>) -> Result<StandingsPage, String> {
    let mut page = source.clone().normalised(MAX_PAGE_HEIGHT);
    let mut deskew = 0.0f32;
    let mut lines = match first_lines {
        Some(lines) => lines,
        None => ocr.read_page(&page),
    };
    let (mut rows, mut pitch) = find_rows(&lines);

    // Photos and rotated screenshots: straighten using the tilt of the score column, then read again.
    if rows.len() >= 3 {
        let pts: Vec<(f32, f32)> = rows.iter().map(|r| (lines[r.score].x1, lines[r.score].cy())).collect();
        let slope = column_slope(&pts);
        let degrees = slope.atan().to_degrees();
        if degrees.abs() > 0.6 && degrees.abs() < 25.0 {
            page = rotate(&page, -degrees);
            deskew = -degrees;
            lines = ocr.read_page(&page);
            let found = find_rows(&lines);
            rows = found.0;
            pitch = found.1;
        }
    }
    if rows.len() < 2 {
        return Err("no standings table found".into());
    }

    fill_missing_rows(ocr, &page, &mut lines, &mut rows, pitch);
    let tokens = Tokens { lines: &lines };
    let mut out_rows: Vec<StandingsRow> = Vec::new();
    // The flag column sits one row-pitch right of the rank column in every row.
    let flag_right: Option<f32> = {
        let mut xs: Vec<f32> = rows
            .iter()
            .filter_map(|g| {
                lines
                    .iter()
                    .filter(|l| (l.cy() - g.cy).abs() <= g.pitch * 0.4 && l.x1 < lines[g.score].x0 && numeric_value(&l.text).map_or(false, |v| (1..=500).contains(&v)) && l.text.trim().ends_with('.'))
                    .map(|l| l.x1)
                    .fold(None, |a: Option<f32>, x| Some(a.map_or(x, |m| m.max(x))))
            })
            .collect();
        if xs.len() >= 2 {
            Some(median(&mut xs) + pitch * 1.0)
        } else {
            None
        }
    };
    // The rank column: where the dotted numbers ("12.") end, row after row.
    let rank_col_x1: Option<f32> = {
        let mut xs: Vec<f32> = lines.iter().filter(|l| l.text.trim().ends_with('.') && is_numeric_token(&l.text) && l.confidence >= 0.6 && rows.iter().any(|g| (l.cy() - g.cy).abs() <= g.pitch * 0.4)).map(|l| l.x1).collect();
        if xs.len() >= 3 {
            Some(median(&mut xs))
        } else {
            None
        }
    };
    for group in &rows {
        let score_line = &tokens.lines[group.score];
        let half = group.pitch * 0.46;
        // Everything left of the score on this row's band.
        let mut members: Vec<usize> = tokens
            .lines
            .iter()
            .enumerate()
            .filter(|(i, l)| *i != group.score && (l.cy() - group.cy).abs() <= half && l.x1 <= score_line.x0 + score_line.h() * 0.3 && !l.text.trim().is_empty())
            .map(|(i, _)| i)
            .collect();
        members.sort_by(|&a, &b| tokens.lines[a].x0.partial_cmp(&tokens.lines[b].x0).unwrap());
        if members.is_empty() {
            continue;
        }

        // The name is the widest text token of the row; the rank is the number just left of it (dotted when the
        // OCR kept the dot), and the small number/icon further left is the event-points badge.
        let usable = |i: usize| {
            let l = &tokens.lines[i];
            l.confidence >= 0.3 || l.w() >= l.h() * 1.5
        };
        let main = members
            .iter()
            .copied()
            .filter(|&i| usable(i) && tokens.lines[i].text.chars().any(|c| c.is_alphabetic()))
            .max_by(|&a, &b| tokens.lines[a].w().partial_cmp(&tokens.lines[b].w()).unwrap())
            .or_else(|| {
                // Purely numeric / symbol names: the widest token that is not the rank-like first one.
                members.iter().copied().filter(|&i| usable(i)).max_by(|&a, &b| tokens.lines[a].w().partial_cmp(&tokens.lines[b].w()).unwrap())
            });
        let Some(main) = main else { continue };
        let main_line = &tokens.lines[main];

        let rank_pos = members
            .iter()
            .copied()
            .filter(|&i| i != main && tokens.lines[i].x1 <= main_line.x0 + main_line.h() * 0.6)
            .filter(|&i| {
                let t = tokens.lines[i].text.trim();
                is_numeric_token(t) && !rank_options(t).is_empty()
            })
            .max_by(|&a, &b| {
                // Prefer the token sitting in the rank column; without one, the dotted / rightmost number.
                let near = |i: usize| rank_col_x1.map_or(0, |c| ((tokens.lines[i].x1 - c).abs() <= group.pitch * 0.45) as i32);
                let da = tokens.lines[a].text.trim().ends_with('.') as i32;
                let db = tokens.lines[b].text.trim().ends_with('.') as i32;
                near(a).cmp(&near(b)).then(da.cmp(&db)).then(tokens.lines[a].x1.partial_cmp(&tokens.lines[b].x1).unwrap())
            });

        let mut rank = 0u32;
        let mut rank_read = false;
        let mut rank_options_for: Vec<u32> = Vec::new();
        let mut name_from_x = main_line.x0 - 1.0;
        if let Some(r) = rank_pos {
            rank_options_for = rank_options(&tokens.lines[r].text);
            if let Some(&v) = rank_options_for.first() {
                rank = v;
                rank_read = true;
            }
            name_from_x = tokens.lines[r].x1 - tokens.lines[r].h() * 0.2;
        } else if let Some(caps) = glued_rank_regex().captures(main_line.text.trim()) {
            // A rank (and the points badge before it) glued to the name ("57. Md.Tufayel", "1 84. Meerkat").
            if let Ok(v) = caps[1].parse::<u32>() {
                if (1..=500).contains(&v) {
                    rank = v;
                    rank_read = true;
                    rank_options_for = vec![v];
                }
            }
        }

        // The name region: the main token plus text fragments on the same row to its right (split names).
        let mut name_lines: Vec<usize> = vec![main];
        for &i in &members {
            if i == main || !usable(i) {
                continue;
            }
            let l = &tokens.lines[i];
            let in_zone = flag_right.map_or(false, |f| l.x0 >= f - pitch * 0.15 && l.x1 <= score_line.x0 - group.pitch * 1.3 + pitch * 0.2);
            if (in_zone || (l.x0 >= name_from_x && l.x0 >= main_line.x1 - main_line.h() * 0.2)) && l.text.chars().any(|c| c.is_alphanumeric()) && l.confidence >= 0.5 {
                name_lines.push(i);
            }
        }
        let name_start = 0usize;
        let _ = name_start;

        // Merge name fragments on the same row into one region.
        let first = &tokens.lines[name_lines[0]];
        let mut x0 = first.x0;
        let mut x1 = first.x1;
        let mut y0 = first.y0;
        let mut y1 = first.y1;
        for &i in &name_lines[1..] {
            let l = &tokens.lines[i];
            x0 = x0.min(l.x0);
            x1 = x1.max(l.x1);
            y0 = y0.min(l.y0);
            y1 = y1.max(l.y1);
        }
        // A detector box that swallowed the badge, rank and flag starts too far left: the name begins after the flag.
        if let Some(limit) = flag_right {
            if x0 < limit - pitch * 0.15 && x1 > limit + pitch * 0.5 {
                x0 = limit - pitch * 0.15;
            }
        }
        // Nothing of the name lies under the trophy icon that precedes the score.
        let name_cap = score_line.x0 - group.pitch * 1.3;
        if x1 > name_cap && name_cap > x0 + pitch * 0.5 {
            x1 = name_cap;
        }
        let hint = glued_rank_regex().captures(tokens.lines[name_lines[0]].text.trim()).map(|c| c[2].to_string()).unwrap_or_else(|| tokens.lines[name_lines[0]].text.clone());
        let glued = glued_rank_regex().is_match(tokens.lines[name_lines[0]].text.trim());
        let hint_conf = if glued || name_lines.len() > 1 { 0.0 } else { tokens.lines[name_lines[0]].confidence };
        let read: ReadName = names.read(&page, [x0, y0, x1, y1], &hint, hint_conf, flag_right, Some(name_cap + group.pitch * 0.15));

        // Band colour from the strip this row occupies.
        let left_edge = members.iter().map(|&i| tokens.lines[i].x0).fold(f32::MAX, f32::min);
        let (yellow, blue) = band_votes(
            &page,
            left_edge,
            group.cy - group.pitch * 0.40,
            score_line.x1 + score_line.h() * 0.5,
            group.cy + group.pitch * 0.40,
        );
        let total = (yellow + blue).max(1) as f32;
        let (band, band_conf) = if yellow == 0 && blue == 0 {
            (BAND_NONE, 0.0)
        } else if yellow >= blue {
            (BAND_YELLOW, yellow as f32 / total)
        } else {
            (BAND_BLUE, blue as f32 / total)
        };

        out_rows.push(StandingsRow {
            rank,
            rank_read,
            rank_options: rank_options_for,
            name: read.text.clone(),
            name_confidence: read.confidence,
            name_flagged: read.flagged,
            score: numeric_value(&score_line.text),
            band,
            band_confidence: band_conf,
            gold_text: read.gold,
            cy: group.cy,
            raw: format!(
                "{} | {} | {}",
                members.iter().map(|&i| tokens.lines[i].text.clone()).collect::<Vec<_>>().join(" "),
                read.text,
                score_line.text
            ),
        });
    }

    if std::env::var("DCA_OCR_DEBUG").is_ok() {
        for r in &out_rows {
            eprintln!("    row cy {:.0} rank {} read {} options {:?} score {:?} name {:?}", r.cy, r.rank, r.rank_read, r.rank_options, r.score, r.name);
        }
    }
    let out_rows = repair_ranks(out_rows, pitch);
    if out_rows.len() < 2 {
        return Err("no readable standings rows".into());
    }
    let banner_line = lines.iter().find(|l| norm(&l.text).contains("finalstandings"));
    let first_cy = out_rows.first().map(|r| r.cy).unwrap_or(0.0);
    let cut = banner_line.map(|l| l.y0).unwrap_or(first_cy - pitch * 0.9);
    let header = read_header(&lines, cut);
    let banner = banner_line.is_some();
    Ok(StandingsPage {
        rows: out_rows,
        header,
        raw_text: lines.iter().map(|l| l.text.clone()).collect::<Vec<_>>().join("\n"),
        has_final_standings_banner: banner,
        deskew_degrees: deskew,
    })
}

/// Rotate an image by `degrees` (counter-clockwise positive) on a black canvas of the same size.
/// Rank readings of a "rank." token. The detector often glues the event-points badge in front of the rank
/// ("3001." is badge 300 + rank 1), so every plausible suffix is an option, longest first.
fn rank_options(text: &str) -> Vec<u32> {
    let t = text.trim();
    let group: String = t.split(|c: char| !c.is_ascii_digit()).filter(|g| !g.is_empty()).last().unwrap_or("").to_string();
    let mut out: Vec<u32> = Vec::new();
    for len in (1..=group.len().min(3)).rev() {
        if let Ok(v) = group[group.len() - len..].parse::<u32>() {
            if (1..=500).contains(&v) && !out.contains(&v) {
                out.push(v);
            }
        }
    }
    out
}

/// Pick one reading per row so that ranks count up by one down the page.
fn resolve_rank_options(rows: &mut [StandingsRow]) {
    let n = rows.len();
    if rows.iter().all(|r| r.rank_options.len() <= 1) {
        return;
    }
    let cands: Vec<Vec<Option<u32>>> = rows.iter().map(|r| if r.rank_options.is_empty() { vec![None] } else { r.rank_options.iter().map(|v| Some(*v)).collect() }).collect();
    let step_cost = |a: Option<u32>, b: Option<u32>| -> f32 {
        match (a, b) {
            (Some(a), Some(b)) if b == a + 1 => 0.0,
            (Some(a), Some(b)) if b == a => 0.6,
            (Some(_), Some(_)) => 2.0,
            _ => 0.2,
        }
    };
    let mut best: Vec<Vec<(f32, usize)>> = Vec::with_capacity(n);
    for i in 0..n {
        let mut layer = Vec::new();
        for (ci, c) in cands[i].iter().enumerate() {
            let own = 0.15 * ci as f32;
            if i == 0 {
                layer.push((own, 0));
            } else {
                let (cost, from) = cands[i - 1].iter().enumerate().map(|(pi, p)| (best[i - 1][pi].0 + step_cost(*p, *c), pi)).fold((f32::MAX, 0), |a, b| if b.0 < a.0 { b } else { a });
                layer.push((cost + own, from));
            }
        }
        best.push(layer);
    }
    let mut pick = best[n - 1].iter().enumerate().fold((f32::MAX, 0), |a, (i, b)| if b.0 < a.0 { (b.0, i) } else { a }).1;
    for i in (0..n).rev() {
        if let Some(v) = cands[i][pick] {
            rows[i].rank = v;
            rows[i].rank_read = true;
        }
        pick = best[i][pick].1;
    }
}

/// The detector sometimes drops or splits a score ("51627" read as "51" + "62"), which would lose the whole row.
/// Rows sit on a regular grid, so wherever the grid has a hole, read the score cell directly.
fn fill_missing_rows(ocr: &Ocr, page: &Rgb, lines: &mut Vec<TextLine>, rows: &mut Vec<RowGroup>, pitch: f32) {
    if rows.len() < 3 || pitch <= 0.0 {
        return;
    }
    rows.sort_by(|a, b| a.cy.partial_cmp(&b.cy).unwrap());
    let mut x0s: Vec<f32> = rows.iter().map(|r| lines[r.score].x0).collect();
    let mut x1s: Vec<f32> = rows.iter().map(|r| lines[r.score].x1).collect();
    let (x0, x1) = (median(&mut x0s), median(&mut x1s));
    let height = {
        let mut hs: Vec<f32> = rows.iter().map(|r| lines[r.score].h()).collect();
        median(&mut hs)
    };
    let mut holes: Vec<f32> = Vec::new();
    for w in rows.windows(2) {
        let gap = w[1].cy - w[0].cy;
        let steps = (gap / pitch).round() as usize;
        if (2..=4).contains(&steps) && (gap - steps as f32 * pitch).abs() < pitch * 0.25 * steps as f32 {
            for m in 1..steps {
                holes.push(w[0].cy + gap * m as f32 / steps as f32);
            }
        }
    }
    if holes.is_empty() {
        return;
    }
    let image = page.to_image();
    for cy in holes {
        let pad = height * 0.35;
        let (top, bottom) = ((cy - height * 0.62).max(0.0), (cy + height * 0.62).min(page.h as f32 - 1.0));
        let (left, right) = ((x0 - pad).max(0.0), (x1 + pad).min(page.w as f32 - 1.0));
        if right - left < 8.0 || bottom - top < 6.0 {
            continue;
        }
        let quad = [Pt { x: left, y: top }, Pt { x: right, y: top }, Pt { x: right, y: bottom }, Pt { x: left, y: bottom }];
        let crop = crop_quad(&image, &quad);
        let (text, confidence) = ocr.recognise(Script::Latin, vec![crop]).into_iter().next().unwrap_or_default();
        let digits = text.chars().filter(|c| c.is_ascii_digit()).count();
        if confidence >= 0.5 && is_numeric_token(&text) && (3..=7).contains(&digits) {
            lines.push(TextLine { quad, x0: left, y0: top, x1: right, y1: bottom, text, confidence, script: Script::Latin });
            rows.push(RowGroup { score: lines.len() - 1, members: Vec::new(), cy, pitch });
        }
    }
    rows.sort_by(|a, b| a.cy.partial_cmp(&b.cy).unwrap());
}

fn glued_rank_regex() -> &'static regex::Regex {
    static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    RE.get_or_init(|| regex::Regex::new(r"^[\d\s)(,]*?(\d{1,3})\.\s*(.*)$").unwrap())
}

pub fn rotate(src: &Rgb, degrees: f32) -> Rgb {
    let rad = degrees.to_radians();
    let (sin, cos) = (rad.sin(), rad.cos());
    let (w, h) = (src.w as f32, src.h as f32);
    let (cx, cy) = (w / 2.0, h / 2.0);
    let mut out = Rgb { w: src.w, h: src.h, data: vec![0u8; src.data.len()] };
    for y in 0..src.h {
        for x in 0..src.w {
            let dx = x as f32 - cx;
            let dy = y as f32 - cy;
            let sx = cos * dx - sin * dy + cx;
            let sy = sin * dx + cos * dy + cy;
            if sx >= 0.0 && sy >= 0.0 && sx < w - 1.0 && sy < h - 1.0 {
                let (ix, iy) = (sx as u32, sy as u32);
                let i = ((y * src.w + x) * 3) as usize;
                out.data[i..i + 3].copy_from_slice(&src.px(ix, iy));
            }
        }
    }
    out
}

/// Ranks count up by one down a page. Overlapping screenshots repeat rows (same score and name): the repeats are
/// dropped first. Then the rank that most rows agree on (rank minus position) wins, which fills in unreadable
/// ranks and corrects isolated misreads; rows that never fit are dropped.
fn repair_ranks(mut rows: Vec<StandingsRow>, pitch: f32) -> Vec<StandingsRow> {
    rows.sort_by(|a, b| a.cy.partial_cmp(&b.cy).unwrap());
    if rows.is_empty() {
        return rows;
    }
    // Repeated rows from overlapping screenshots.
    let mut kept: Vec<StandingsRow> = Vec::with_capacity(rows.len());
    let mut repeats: Vec<f32> = Vec::new();
    for row in rows.drain(..) {
        let repeat = row.score.map_or(false, |s| s >= 1000)
            && kept.iter().any(|k| k.score == row.score && (norm(&k.name) == norm(&row.name) || edit_distance(&norm(&k.name), &norm(&row.name)) <= 2));
        if repeat {
            repeats.push(row.cy);
        } else {
            kept.push(row);
        }
    }
    let mut rows = kept;
    resolve_rank_options(&mut rows);
    // Position on the row grid (a row the detector lost leaves a hole, not a shifted list).
    let first_cy = rows[0].cy;
    // ...minus the slots taken by repeated rows, which are not part of the ranking.
    let grid: Vec<i64> = rows
        .iter()
        .map(|r| if pitch > 0.0 { ((r.cy - first_cy) / pitch).round() as i64 - repeats.iter().filter(|c| **c < r.cy && **c > first_cy).count() as i64 } else { 0 })
        .collect();
    use std::collections::HashMap;
    let mut offsets: HashMap<i64, u32> = HashMap::new();
    for (i, row) in rows.iter().enumerate() {
        if row.rank_read && row.rank > 0 {
            *offsets.entry(row.rank as i64 - grid[i]).or_default() += 1;
        }
    }
    let Some((&offset, _)) = offsets.iter().max_by_key(|(off, count)| (**count, -off.abs())) else {
        // No rank was readable (a cropped screenshot): leave them unnumbered; the session merge handles that.
        return rows;
    };
    for (i, row) in rows.iter_mut().enumerate() {
        let expected = offset + grid[i];
        if expected >= 1 && (!row.rank_read || row.rank as i64 != expected) {
            row.rank = expected as u32;
            row.rank_read = false;
        }
    }
    rows.retain(|r| r.rank >= 1);
    rows
}

fn edit_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for i in 1..=a.len() {
        let mut cur = vec![i; b.len() + 1];
        for j in 1..=b.len() {
            cur[j] = (prev[j] + 1).min(cur[j - 1] + 1).min(prev[j - 1] + usize::from(a[i - 1] != b[j - 1]));
        }
        prev = cur;
    }
    prev[b.len()]
}

// --------------------------------------------------------------------------------------------- header

/// Insert the spaces the recogniser drops in stylised titles ("Sky-RockSamba" -> "Sky-Rock Samba").
pub fn respace_title(title: &str) -> String {
    let t = title.trim();
    if t.contains(' ') || t.chars().count() < 7 {
        return t.to_string();
    }
    let mut out = String::new();
    let mut prev: Option<char> = None;
    for c in t.chars() {
        if let Some(p) = prev {
            if p.is_lowercase() && c.is_uppercase() {
                out.push(' ');
            }
        }
        out.push(c);
        prev = Some(c);
    }
    out
}

/// Fix the trademark artefacts OCR produces for team names ("Discord 3TM", "Discord 3\u{2122}M") and snap to a
/// known team name when it is clearly the same team.
pub fn clean_team_label(raw: &str, known: &[String]) -> String {
    let text = raw.trim();
    let key = norm(text);
    let mut best: Option<(&String, usize)> = None;
    for team in known {
        let k = norm(team);
        if k.is_empty() {
            continue;
        }
        let k_trim = k.trim_end_matches("tm").to_string();
        if key == k || key == k_trim || (key.starts_with(&k_trim) && key.len() <= k.len() + 2 && !k_trim.is_empty()) {
            if best.map_or(true, |(_, len)| k.len() > len) {
                best = Some((team, k.len()));
            }
        }
    }
    if let Some((team, _)) = best {
        return team.clone();
    }
    let mut cleaned = text.replace("\u{2122}M", "\u{2122}").replace("\u{2122}m", "\u{2122}");
    for suffix in ["TM", "Tm", "tm"] {
        if let Some(stripped) = cleaned.strip_suffix(suffix) {
            if stripped.chars().last().map_or(false, |c| c.is_ascii_digit()) {
                cleaned = format!("{stripped}\u{2122}");
            }
        }
    }
    cleaned
}

fn read_header(lines: &[TextLine], cut_y: f32) -> StandingsHeader {
    let mut header = StandingsHeader::default();
    let above: Vec<&TextLine> = lines
        .iter()
        .filter(|l| l.cy() < cut_y && !l.text.trim().is_empty() && l.confidence >= 0.3)
        .collect();
    if above.is_empty() {
        return header;
    }

    // Candidate title / team-name lines: real words, not the currency counters or UI glyphs.
    let mut words: Vec<&TextLine> = above
        .iter()
        .copied()
        .filter(|l| l.confidence >= 0.5 && l.text.chars().filter(|c| c.is_alphabetic()).count() >= 3 && norm(&l.text) != "vs")
        .collect();
    words.sort_by(|a, b| a.cy().partial_cmp(&b.cy()).unwrap());
    if words.is_empty() {
        return header;
    }

    // Group by height of the line; the topmost group is the title when it is a single centred line.
    let mut groups: Vec<Vec<&TextLine>> = Vec::new();
    for l in words {
        match groups.last_mut() {
            Some(g) if (l.cy() - g[0].cy()).abs() <= g[0].h().max(l.h()) * 0.7 => g.push(l),
            _ => groups.push(vec![l]),
        }
    }

    let vs = above.iter().find(|l| norm(&l.text) == "vs");
    let (title_group, team_group) = if groups.len() >= 2 && groups[0].len() == 1 {
        (Some(&groups[0]), Some(&groups[1]))
    } else if groups.len() >= 2 && groups[1].len() >= 2 {
        (Some(&groups[0]), Some(&groups[1]))
    } else {
        (None, groups.first())
    };

    if let Some(group) = title_group {
        // Only a single line counts as the title.
        if group.len() == 1 {
            header.title = respace_title(&group[0].text);
        }
    }
    let mut team_lines: Vec<&TextLine> = team_group.map(|g| g.clone()).unwrap_or_default();
    team_lines.sort_by(|a, b| a.cx().partial_cmp(&b.cx()).unwrap());
    let (left, right) = match (vs, team_lines.len()) {
        (Some(vs), n) if n >= 2 => (
            team_lines.iter().copied().filter(|l| l.cx() < vs.cx()).max_by(|a, b| a.w().partial_cmp(&b.w()).unwrap()),
            team_lines.iter().copied().filter(|l| l.cx() > vs.cx()).max_by(|a, b| a.w().partial_cmp(&b.w()).unwrap()),
        ),
        (_, n) if n >= 2 => (team_lines.first().copied(), team_lines.last().copied()),
        _ => (None, None),
    };
    if let Some(l) = left {
        header.left_team = l.text.trim().to_string();
    }
    if let Some(r) = right {
        header.right_team = r.text.trim().to_string();
    }

    let score_for = |team: Option<&TextLine>| -> Option<u64> {
        let team = team?;
        above
            .iter()
            .filter(|l| is_numeric_token(&l.text) && l.cy() > team.cy() && l.cy() < team.cy() + team.h() * 3.5)
            .filter(|l| (l.cx() - team.cx()).abs() < team.w() * 0.75)
            .min_by(|a, b| (a.cy() - team.cy()).abs().partial_cmp(&(b.cy() - team.cy()).abs()).unwrap())
            .and_then(|l| numeric_value(&l.text))
            .filter(|v| *v < 1_000_000)
    };
    header.left_score = score_for(left);
    header.right_score = score_for(right);
    header
}

/// Quick structural test: a score column with at least two rows, or the "FINAL STANDINGS" banner.
pub fn looks_like_standings(lines: &[TextLine]) -> bool {
    if lines.iter().any(|l| norm(&l.text).contains("finalstandings")) {
        return true;
    }
    find_rows(lines).0.len() >= 3
}

pub fn find_rows_count(lines: &[TextLine]) -> usize {
    find_rows(lines).0.len()
}


// ------------------------------------------------------------------------------------------------ podium

/// The team-event result ("podium") screen: the event title, both team names and scores and the top players of
/// each side, drawn over a scene instead of a table. Returned as a page of rows without scores so the usual merge
/// can use it for the header and to confirm names.
pub fn read_podium(names: &NameReader, page: &Rgb, lines: &[TextLine]) -> Option<StandingsPage> {
    let (w, h) = (page.w as f32, page.h as f32);
    let rank_of = |l: &TextLine| -> Option<u32> {
        let t = l.text.trim().trim_end_matches('.').trim();
        if t.is_empty() || !t.chars().all(|c| c.is_ascii_digit()) || t.len() > 3 || l.confidence < 0.3 || l.cy() > h * 0.6 {
            return None;
        }
        t.parse().ok().filter(|v| (1..=100).contains(v))
    };
    let mut pairs: Vec<(u32, &TextLine, &TextLine)> = Vec::new();
    for rank_line in lines {
        let Some(rank) = rank_of(rank_line) else { continue };
        let name_line = lines
            .iter()
            .filter(|n| {
                n.x0 >= rank_line.x1 - rank_line.h() * 0.3
                    && n.x0 - rank_line.x1 <= rank_line.h() * 4.5
                    && (n.cy() - rank_line.cy()).abs() <= rank_line.h() * 0.6
                    && n.confidence >= 0.4
                    && n.text.chars().any(|c| c.is_alphanumeric())
                    && !is_numeric_token(&n.text)
            })
            .min_by(|a, b| a.x0.partial_cmp(&b.x0).unwrap());
        if let Some(n) = name_line {
            if !pairs.iter().any(|(r, _, _)| *r == rank) {
                pairs.push((rank, rank_line, n));
            }
        }
    }
    if pairs.len() < 3 {
        return None;
    }
    let first_top = pairs.iter().map(|(_, r, _)| r.y0).fold(f32::MAX, f32::min);
    let above = |l: &&TextLine| l.cy() < first_top && l.confidence >= 0.4;
    let numeric_header: Vec<&TextLine> = lines.iter().filter(above).filter(|l| is_numeric_token(&l.text) && (3..=6).contains(&l.text.chars().filter(|c| c.is_ascii_digit()).count())).collect();
    let marker = lines.iter().any(|l| {
        let k = norm(&l.text);
        k.contains("touchtocontinue") || k == "winner" || k == "loser" || k == "draw"
    });
    if numeric_header.len() < 2 && !marker {
        return None;
    }

    let mut header = StandingsHeader::default();
    let words: Vec<&TextLine> = lines
        .iter()
        .filter(above)
        .filter(|l| l.confidence >= 0.5 && l.text.chars().filter(|c| c.is_alphabetic()).count() >= 3)
        .filter(|l| {
            let k = norm(&l.text);
            !(k.contains("winner") || k.contains("touch") || k.contains("continue") || k.contains("bonus") || k.contains("reward"))
        })
        .collect();
    let title = words.iter().copied().filter(|l| l.cy() < h * 0.3).max_by(|a, b| a.h().partial_cmp(&b.h()).unwrap().then(b.cy().partial_cmp(&a.cy()).unwrap()));
    if let Some(t) = title {
        header.title = respace_title(&t.text);
    }
    let left_score_line = numeric_header.iter().copied().filter(|l| l.cx() < w * 0.5).max_by(|a, b| a.cx().partial_cmp(&b.cx()).unwrap());
    let right_score_line = numeric_header.iter().copied().filter(|l| l.cx() >= w * 0.5).min_by(|a, b| a.cx().partial_cmp(&b.cx()).unwrap());
    header.left_score = left_score_line.and_then(|l| numeric_value(&l.text));
    header.right_score = right_score_line.and_then(|l| numeric_value(&l.text));
    let row_cy = match (left_score_line, right_score_line) {
        (Some(a), Some(b)) => Some((a.cy() + b.cy()) / 2.0),
        (Some(a), None) | (None, Some(a)) => Some(a.cy()),
        _ => None,
    };
    if let Some(cy) = row_cy {
        let team_lines: Vec<&TextLine> = words.iter().copied().filter(|l| Some(*l as *const TextLine) != title.map(|t| t as *const TextLine) && (l.cy() - cy).abs() <= l.h().max(20.0) * 1.2).collect();
        let team_label = |l: &TextLine| names.read_text_only(page, [l.x0, l.y0, l.x1, l.y1], &l.text).text;
        if let Some(l) = team_lines.iter().copied().filter(|l| l.cx() < w * 0.5).max_by(|a, b| a.x1.partial_cmp(&b.x1).unwrap()) {
            header.left_team = team_label(l);
        }
        if let Some(l) = team_lines.iter().copied().filter(|l| l.cx() >= w * 0.5).min_by(|a, b| a.x0.partial_cmp(&b.x0).unwrap()) {
            header.right_team = team_label(l);
        }
    }

    pairs.sort_by(|a, b| a.1.cy().partial_cmp(&b.1.cy()).unwrap().then(a.1.cx().partial_cmp(&b.1.cx()).unwrap()));
    let mut rows = Vec::new();
    for (rank, rank_line, name_line) in pairs {
        // The flag drawn before every name is not an emoji of the name.
        let read = names.read_text_only(page, [name_line.x0, name_line.y0, name_line.x1, name_line.y1], &name_line.text);
        let (yellow, blue) = band_votes(page, rank_line.x0, name_line.cy() - name_line.h() * 0.6, name_line.x1 + name_line.h(), name_line.cy() + name_line.h() * 0.6);
        let left_side = name_line.cx() < w * 0.5;
        let total = (yellow + blue).max(1) as f32;
        let (band, conf) = if yellow + blue >= 20 && (yellow as f32 / total > 0.7 || blue as f32 / total > 0.7) {
            if yellow > blue { (BAND_YELLOW, yellow as f32 / total) } else { (BAND_BLUE, blue as f32 / total) }
        } else if left_side {
            (BAND_YELLOW, 0.6)
        } else {
            (BAND_BLUE, 0.6)
        };
        rows.push(StandingsRow {
            rank,
            rank_read: true,
            rank_options: vec![rank],
            name: read.text.clone(),
            name_confidence: read.confidence,
            name_flagged: read.flagged,
            score: None,
            band,
            band_confidence: conf,
            gold_text: false,
            cy: name_line.cy(),
            raw: format!("podium {} {}", rank, read.text),
        });
    }
    Some(StandingsPage {
        rows,
        header,
        raw_text: lines.iter().map(|l| l.text.clone()).collect::<Vec<_>>().join("\n"),
        has_final_standings_banner: false,
        deskew_degrees: 0.0,
    })
}


/// Cheap structural test for the result screen: rank-and-name pairs near the top, team scores, or its markers.
pub fn looks_like_podium(lines: &[TextLine], page_w: f32, page_h: f32) -> bool {
    let _ = page_w;
    let rank_lines: Vec<&TextLine> = lines
        .iter()
        .filter(|l| {
            let t = l.text.trim().trim_end_matches('.').trim();
            !t.is_empty() && t.len() <= 3 && t.chars().all(|c| c.is_ascii_digit()) && l.confidence >= 0.3 && l.cy() < page_h * 0.6 && t.parse::<u32>().map_or(false, |v| (1..=100).contains(&v))
        })
        .collect();
    let pairs = rank_lines
        .iter()
        .filter(|r| {
            lines.iter().any(|n| {
                n.x0 >= r.x1 - r.h() * 0.3 && n.x0 - r.x1 <= r.h() * 4.5 && (n.cy() - r.cy()).abs() <= r.h() * 0.6 && n.confidence >= 0.4 && n.text.chars().any(|c| c.is_alphabetic()) && !is_numeric_token(&n.text)
            })
        })
        .count();
    let marker = lines.iter().any(|l| {
        let k = norm(&l.text);
        k.contains("touchtocontinue") || k == "winner" || k == "loser"
    });
    let numeric_header = lines.iter().filter(|l| l.cy() < page_h * 0.3 && is_numeric_token(&l.text) && (3..=6).contains(&l.text.chars().filter(|c| c.is_ascii_digit()).count())).count();
    pairs >= 3 && (marker || numeric_header >= 2)
}
