//! Player-name reading: several recognisers (Latin, multilingual, then every other script when needed) vote on a
//! crop of the name, and emoji - which text recognisers cannot read - are located and matched against the
//! Noto Color Emoji glyphs.

use crate::emoji::EmojiIndex;
use crate::imaging::*;
use crate::ocr::{crop_quad, Ocr, Pt, Script};
use crate::unicode_names::clean_player_name;
use image::RgbImage;
use std::sync::Arc;

#[derive(Clone, Debug)]
pub struct ReadName {
    pub text: String,
    pub confidence: f32,
    pub flagged: bool,
    /// Drawn in gold: the reading player's own row.
    pub gold: bool,
}

pub struct NameReader {
    ocr: Arc<Ocr>,
    emoji: Option<Arc<EmojiIndex>>,
}

#[derive(Clone, Debug)]
struct Candidate {
    text: String,
    conf: f32,
    script: Script,
}

fn crop_rect(page: &Rgb, bbox: [f32; 4], pad_x: f32, pad_y: f32) -> Rect {
    let h = bbox[3] - bbox[1];
    Rect::new((bbox[0] - h * pad_x).floor() as i64, (bbox[1] - h * pad_y).floor() as i64, (bbox[2] - bbox[0] + 2.0 * h * pad_x).ceil() as i64, (h + 2.0 * h * pad_y).ceil() as i64, page.w, page.h)
}

/// How plausible a reading is as a player name for the script that produced it.
fn plausibility(text: &str, script: Script) -> f32 {
    let alphabetic: Vec<char> = text.chars().filter(|c| c.is_alphabetic()).collect();
    if alphabetic.is_empty() {
        return if text.chars().any(|c| c.is_ascii_digit()) { 0.7 } else { 0.0 };
    }
    let owned = alphabetic.iter().filter(|c| script.owns(**c) || c.is_ascii()).count();
    owned as f32 / alphabetic.len() as f32
}

fn script_purity(text: &str, script: Script) -> f32 {
    let alphabetic: Vec<char> = text.chars().filter(|c| c.is_alphabetic()).collect();
    if alphabetic.is_empty() {
        return 0.0;
    }
    alphabetic.iter().filter(|c| script.owns(**c)).count() as f32 / alphabetic.len() as f32
}

fn is_digits_only(text: &str) -> bool {
    text.chars().all(|c| c.is_ascii_digit() || c.is_whitespace() || c == '.')
}

/// A lone "." or "!" after a name is an emoji the recogniser misread.
fn strip_trailing_marks(text: &str) -> String {
    let trimmed = text.trim_end_matches(['.', '!', ':', ';', ',', '\u{b7}', ' ']);
    if trimmed.chars().filter(|c| c.is_alphanumeric()).count() >= 3 {
        trimmed.to_string()
    } else {
        text.to_string()
    }
}

/// Cyrillic letters that are drawn exactly like Latin ones.
const CYRILLIC_LOOKALIKES: &str = "АВСЕНКМОРТХУаеорсху";

/// Every letter is one a Cyrillic word could be drawn with ("CaHA", "TOM"), so the Latin reading is suspect.
fn all_lookalikes(text: &str) -> bool {
    let letters: Vec<char> = text.chars().filter(|c| c.is_alphabetic()).collect();
    letters.len() >= 3
        && letters.iter().filter(|c| "ABCEHKMOPTXYaceopxy".contains(**c)).count() * 10 >= letters.len() * 8
        && text.chars().zip(text.chars().skip(1)).any(|(a, b)| a.is_lowercase() && b.is_uppercase())
}

/// Confidence from which the page-level reading of a name is trusted (`DCA_NAME_ACCEPT`, 1.0 = always read twice).
fn fast_accept() -> f32 {
    std::env::var("DCA_NAME_ACCEPT").ok().and_then(|v| v.trim().parse().ok()).unwrap_or(0.9)
}

/// Letters and digits only, with the look-alikes the recognisers swap folded together.
fn fold_key(text: &str) -> Vec<char> {
    text.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(|c| c.to_lowercase())
        .map(|c| match c {
            'l' | 'i' | '1' | '|' => 'i',
            '0' => 'o',
            '5' => 's',
            other => other,
        })
        .collect()
}

fn char_distance(a: &[char], b: &[char]) -> usize {
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

impl NameReader {
    pub fn new(ocr: Arc<Ocr>, emoji: Option<Arc<EmojiIndex>>) -> NameReader {
        NameReader { ocr, emoji }
    }

    pub fn ocr(&self) -> &Ocr {
        &self.ocr
    }

    /// Prepare the crop variants a recogniser sees: the raw pixels, and the white/gold "ink" only (black on white).
    fn variants(&self, page: &Rgb, rect: Rect) -> Vec<RgbImage> {
        let raw = page.crop(rect).to_image();
        let mut out = vec![raw];
        if rect.w > 4 && rect.h > 4 {
            let ink = ink_gray(page, rect, Ink::WhiteOrGold);
            if ink_pixel_count(&ink) > 30 {
                out.push(image::DynamicImage::ImageLuma8(ink).to_rgb8());
            }
        }
        out
    }

    fn collect(&self, scripts: &[Script], crops: &[RgbImage]) -> Vec<Candidate> {
        let mut out = Vec::new();
        for &script in scripts {
            if !self.ocr.has_script(script) {
                continue;
            }
            for (text, conf) in self.ocr.recognise(script, crops.to_vec()) {
                let text = text.trim().to_string();
                if !text.is_empty() {
                    out.push(Candidate { text, conf, script });
                }
            }
        }
        out
    }

    /// The reading the recognisers agree on most: each candidate is scored by its own confidence plus the support
    /// of the others that read nearly the same text.
    fn best(&self, candidates: &[Candidate]) -> Option<Candidate> {
        if std::env::var("DCA_NAMES_LEGACY").is_ok() {
            return candidates.iter().max_by(|a, b| (a.conf * plausibility(&a.text, a.script)).partial_cmp(&(b.conf * plausibility(&b.text, b.script))).unwrap()).cloned();
        }
        let keys: Vec<Vec<char>> = candidates.iter().map(|c| fold_key(&c.text)).collect();
        let own: Vec<f32> = candidates.iter().map(|c| c.conf * plausibility(&c.text, c.script)).collect();
        let n = candidates.len();
        let mut best: Option<(f32, usize)> = None;
        for i in 0..n {
            let mut support = 0.0;
            for j in 0..n {
                if i == j {
                    continue;
                }
                let longest = keys[i].len().max(keys[j].len()).max(1) as f32;
                let similarity = 1.0 - char_distance(&keys[i], &keys[j]) as f32 / longest;
                if similarity >= 0.7 {
                    support += similarity * own[j];
                }
            }
            let score = own[i] + 0.5 * support / (n.max(2) - 1) as f32;
            if best.is_none_or(|b| score > b.0) {
                best = Some((score, i));
            }
        }
        best.map(|(_, i)| candidates[i].clone())
    }

    /// Read one text segment (no emoji) from `rect` of the page.
    fn read_segment(&self, page: &Rgb, rect: Rect, hint: &str) -> (String, f32) {
        if rect.w < 3 || rect.h < 4 {
            return (String::new(), 0.0);
        }
        let crops = self.variants(page, rect);
        let mut candidates = self.collect(&[Script::Latin, Script::Multi], &crops);
        // The reading the page-level pass already made (full-line context) is one more vote.
        let hint_text = hint.trim();
        if std::env::var("DCA_NAMES_LEGACY").is_err() && hint_text.chars().filter(|c| c.is_alphanumeric()).count() >= 2 && !is_digits_only(hint_text) {
            candidates.push(Candidate { text: hint_text.to_string(), conf: 0.8, script: Script::Latin });
        }
        if std::env::var("DCA_OCR_DEBUG").is_ok() {
            for c in &candidates {
                eprintln!("    cand {:?} {:.2} {}", c.text, c.conf, c.script.name());
            }
        }
        let mut best = self.best(&candidates);

        // Not Latin / not convincing: let every other script have a go and keep the one that clearly owns the text.
        let needs_escalation = best.as_ref().is_none_or(|b| b.conf < 0.85 || b.text.chars().filter(|c| c.is_alphanumeric()).count() < 3 || all_lookalikes(&b.text));
        if needs_escalation {
            // Most likely scripts first, stopping at the first convincing reading (each model costs memory and time).
            let mut extra: Vec<Candidate> = Vec::new();
            let unreadable = best.as_ref().is_none_or(|b| b.conf < 0.6 || b.text.chars().filter(|c| c.is_alphanumeric()).count() < 2);
            let order: &[Script] = if unreadable { &Script::ESCALATION } else { &Script::ESCALATION[..1] };
            for script in order {
                let found = self.collect(&[*script], &crops[..1]);
                let convincing = found.iter().any(|c| c.conf >= 0.85 && script_purity(&c.text, c.script) >= 0.8);
                extra.extend(found);
                if convincing {
                    break;
                }
            }
            if std::env::var("DCA_OCR_DEBUG").is_ok() {
                for c in &extra {
                    eprintln!("    esc  {:?} {:.2} {}", c.text, c.conf, c.script.name());
                }
            }
            candidates.extend(extra);
            let mut winner: Option<Candidate> = best.clone();
            for cand in &candidates {
                if cand.script == Script::Latin || cand.script == Script::Multi {
                    continue;
                }
                let purity = script_purity(&cand.text, cand.script);
                if purity >= 0.6 && cand.conf >= 0.6 && winner.as_ref().is_none_or(|w| cand.conf > w.conf + 0.08 || w.conf < 0.5) {
                    winner = Some(cand.clone());
                }
            }
            best = winner;
            // Cyrillic names built from letters that look Latin ("Саня" reads as "CaHA"): the Cyrillic recogniser knows
            // better when it finds letters that only exist in Cyrillic.
            if let Some(cyr) = candidates.iter().filter(|c| c.script == Script::Cyrillic).max_by(|a, b| a.conf.partial_cmp(&b.conf).unwrap()) {
                let letters: Vec<char> = cyr.text.chars().filter(|c| c.is_alphabetic()).collect();
                let cyrillic = letters.iter().filter(|c| Script::Cyrillic.owns(**c)).count();
                let exclusive = letters.iter().filter(|c| Script::Cyrillic.owns(**c) && !CYRILLIC_LOOKALIKES.contains(**c)).count();
                let current_is_lookalike = best.as_ref().is_none_or(|b| all_lookalikes(&b.text) || b.conf < cyr.conf);
                if cyr.conf >= 0.6 && cyrillic * 10 >= letters.len() * 8 && exclusive >= 1 && current_is_lookalike {
                    best = Some(cyr.clone());
                }
            }
        }
        let _ = hint;
        match best {
            Some(b) => (strip_trailing_marks(&clean_player_name(&b.text)), b.conf),
            None => (clean_player_name(hint), 0.0),
        }
    }

    /// Read a name without looking for emoji (large lettering over a scene, e.g. the podium screen).
    pub fn read_text_only(&self, page: &Rgb, bbox: [f32; 4], hint: &str) -> ReadName {
        let h = bbox[3] - bbox[1];
        let rect = crop_rect(page, [bbox[0] + h * 0.1, bbox[1], bbox[2], bbox[3]], 0.05, 0.12);
        if rect.w < 3 || rect.h < 4 {
            return ReadName { text: clean_player_name(hint), confidence: 0.0, flagged: true, gold: false };
        }
        let (white, gold) = ink_counts(page, rect);
        let (text, conf) = self.read_segment(page, rect, hint);
        ReadName { flagged: conf < 0.6 || text.is_empty(), text, confidence: conf, gold: gold > white }
    }

    /// Read a player name located at `bbox` = `[x0, y0, x1, y1]` on `page`.
    /// `hint_conf` is the confidence of the page-level reading `hint`: a confident, emoji-free name is accepted as is
    /// (no second recognition); pass 0 to always read the crop again.
    pub fn read(&self, page: &Rgb, bbox: [f32; 4], hint: &str, hint_conf: f32, flag_right: Option<f32>, right_limit: Option<f32>) -> ReadName {
        let rect = crop_rect(page, bbox, 0.18, 0.10);
        if rect.w < 3 || rect.h < 4 {
            return ReadName { text: clean_player_name(hint), confidence: 0.0, flagged: true, gold: false };
        }
        let (white, gold) = ink_counts(page, rect);
        let gold_text = gold > white;
        let h = bbox[3] - bbox[1];

        // Emoji sit next to (or inside) the text box but are not text, so look a little beyond it.
        let blobs = match &self.emoji {
            Some(index) => {
                let wide = Rect::new((bbox[0] - h * 2.6) as i64, (bbox[1] - h * 0.25) as i64, ((bbox[2] - bbox[0]) + h * 4.8) as i64, (h * 1.5) as i64, page.w, page.h);
                let mut found = index.locate(page, wide);
                if std::env::var("DCA_OCR_DEBUG").is_ok() {
                    eprintln!("    name bbox {:?} wide {:?} blobs {:?}", bbox, wide, found);
                }
                // The country flag drawn before every name is a wide, colourful blob: not an emoji.
                match flag_right {
                    // The flag column is known from the rank column: nothing centred in it is an emoji.
                    Some(limit) => found.retain(|b| (b.x + b.w / 2) as f32 > limit),
                    None => {
                        let left_of_text: Vec<usize> = found.iter().enumerate().filter(|(_, b)| (b.x as f32) < bbox[0] - h * 0.2).map(|(i, _)| i).collect();
                        if let Some(&first) = left_of_text.first() {
                            if found[first].w as f32 / found[first].h.max(1) as f32 > 1.12 {
                                found.remove(first);
                            }
                        }
                    }
                }
                found.retain(|b| {
                    let (x0, x1) = (b.x as f32, (b.x + b.w) as f32);
                    // The trophy icon in front of the score is not an emoji of the name.
                    x1 > bbox[0] - h * 1.5 && x0 < bbox[2] + h * 1.7 && right_limit.is_none_or(|l| ((x0 + x1) / 2.0) < l)
                });
                found
            }
            None => Vec::new(),
        };
        if blobs.is_empty() {
            if hint_conf >= fast_accept() && hint.chars().filter(|c| c.is_alphanumeric()).count() >= 2 {
                return ReadName { text: strip_trailing_marks(&clean_player_name(hint)), confidence: hint_conf, flagged: false, gold: gold_text };
            }
            let (text, conf) = self.read_segment(page, rect, hint);
            return ReadName { flagged: conf < 0.6 || text.is_empty(), text, confidence: conf, gold: gold_text };
        }

        let index = self.emoji.as_ref().unwrap();
        let mut out = String::new();
        let mut confs: Vec<f32> = Vec::new();
        let mut flagged = false;
        let text_x0 = rect.x;
        let text_x1 = rect.right();
        let mut cursor = text_x0;
        let read_between = |from: u32, to: u32, out: &mut String, confs: &mut Vec<f32>| {
            let from = from.max(text_x0);
            let to = to.min(text_x1);
            if to > from + 3 {
                let seg = Rect { x: from, y: rect.y, w: to - from, h: rect.h };
                let (t, c) = self.read_segment(page, seg, "");
                // A couple of stray symbols next to an emoji are the recogniser misreading its edge.
                let junk = t.chars().filter(|ch| ch.is_alphanumeric()).count() == 0 && t.chars().count() <= 2;
                if !t.is_empty() && !junk {
                    out.push_str(&t);
                    confs.push(c);
                }
            }
        };
        for blob in &blobs {
            read_between(cursor, blob.x, &mut out, &mut confs);
            match index.identify(page, blob) {
                Some((glyph, score)) => {
                    out.push_str(&glyph);
                    confs.push(score.max(0.35));
                    if score < 0.5 {
                        flagged = true;
                    }
                }
                None => {
                    out.push('\u{fffd}');
                    flagged = true;
                }
            }
            cursor = (blob.x + blob.w).max(cursor);
        }
        read_between(cursor, text_x1, &mut out, &mut confs);

        let confidence = if confs.is_empty() { 0.0 } else { confs.iter().sum::<f32>() / confs.len() as f32 };
        ReadName { text: out.trim().to_string(), confidence, flagged: flagged || confidence < 0.6, gold: gold_text }
    }

    /// Read the text of an arbitrary quad (used for non-name labels).
    pub fn read_quad(&self, page: &RgbImage, quad: &[Pt; 4], script: Script) -> (String, f32) {
        let crop = crop_quad(page, quad);
        self.ocr.recognise(script, vec![crop]).into_iter().next().unwrap_or_default()
    }
}
