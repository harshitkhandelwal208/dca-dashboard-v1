//! One entry point for reading screenshots: owns the OCR models and the emoji index, and copes with whatever
//! people upload (cropped, letterboxed, rotated, photographed off a device screen).

use crate::emoji::EmojiIndex;
use crate::imaging::Rgb;
use crate::licence::{anchor_count, read_licence, LicenceReading};
use crate::names::NameReader;
use crate::ocr::{crop_quad, Ocr, Script, TextLine};
use crate::photo;
use crate::standings::{find_rows_count, looks_like_podium, looks_like_standings, read_podium, read_standings, StandingsPage, MAX_PAGE_HEIGHT};
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScreenKind {
    DriverLicence,
    Standings,
    /// The team-event result screen (title, team scores, top players of each side).
    Podium,
    Unknown,
}

pub struct Reader {
    pub ocr: Arc<Ocr>,
    pub names: NameReader,
}

pub struct Analysis {
    pub kind: ScreenKind,
    /// The (possibly straightened / cropped) page the lines refer to.
    pub page: Rgb,
    pub lines: Vec<TextLine>,
    pub licence: LicenceReading,
    /// Which preparation made the screenshot readable ("direct", "cropped", "table-flattened", ...).
    pub method: String,
    /// The parsed table when the screenshot is a standings screen.
    pub standings: Option<StandingsPage>,
    evidence: u32,
}

const TIME_BUDGET: Duration = Duration::from_secs(40);

impl Reader {
    pub fn new(models_dir: &Path, fonts_dir: &Path) -> Result<Reader, String> {
        let ocr = Arc::new(Ocr::load(models_dir)?);
        let emoji_font = fonts_dir.join("NotoColorEmoji.ttf");
        let emoji = match EmojiIndex::load(&emoji_font) {
            Ok(index) => Some(Arc::new(index)),
            Err(error) => {
                tracing::warn!("emoji matching disabled: {error}");
                None
            }
        };
        Ok(Reader { names: NameReader::new(ocr.clone(), emoji), ocr })
    }

    pub fn decode(bytes: &[u8]) -> Result<Rgb, String> {
        Ok(Rgb::decode(bytes)?.normalised(MAX_PAGE_HEIGHT))
    }

    fn attempt(&self, page: Rgb, method: &str, known_teams: &[String]) -> Analysis {
        let page = photo::upscale_small(page).normalised(MAX_PAGE_HEIGHT);
        let lines = self.ocr.read_page(&page);
        let licence = read_licence(&self.names, &page, &lines, known_teams);
        let rows = find_rows_count(&lines) as u32;
        let mut standings = None;
        let (kind, evidence) = if licence.anchors >= 2 && licence.anchors * 2 >= rows {
            let filled = licence.garage_power.is_some() as u32 + licence.cup_points.is_some() as u32 + licence.season_points.is_some() as u32 + !licence.name.is_empty() as u32;
            (ScreenKind::DriverLicence, licence.anchors * 4 + filled * 3)
        } else if looks_like_standings(&lines) {
            match read_standings(&self.ocr, &self.names, &page, Some(lines.clone())) {
                Ok(parsed) => {
                    let n = parsed.rows.len() as f32;
                    let ranked = parsed.rows.iter().filter(|r| r.rank_read).count() as f32;
                    let named = parsed.rows.iter().filter(|r| !r.name.is_empty()).count() as f32;
                    let scored = parsed.rows.iter().filter(|r| r.score.is_some()).count() as f32;
                    let evidence = (ranked * 3.0 + named * 1.0 + scored * 2.0 + n) as u32;
                    standings = Some(parsed);
                    (ScreenKind::Standings, evidence)
                }
                Err(_) => (ScreenKind::Unknown, licence.anchors + rows),
            }
        } else if let Some(podium) = read_podium(&self.names, &page, &lines) {
            let evidence = podium.rows.len() as u32 * 4 + !podium.header.title.is_empty() as u32 * 3 + podium.header.left_score.is_some() as u32 * 3;
            standings = Some(podium);
            (ScreenKind::Podium, evidence)
        } else {
            (ScreenKind::Unknown, licence.anchors + rows)
        };
        let analysis = Analysis { kind, page, lines, licence, method: method.to_string(), standings, evidence };
        if std::env::var("DCA_OCR_DEBUG").is_ok() {
            let detail = analysis.standings.as_ref().map(|p| {
                let n = p.rows.len().max(1) as f32;
                format!(
                    "rows {} ranked {:.2} named {:.2} scored {:.2}",
                    p.rows.len(),
                    p.rows.iter().filter(|r| r.rank_read).count() as f32 / n,
                    p.rows.iter().filter(|r| !r.name.is_empty()).count() as f32 / n,
                    p.rows.iter().filter(|r| r.score.is_some()).count() as f32 / n
                )
            });
            eprintln!("  attempt {method}: {:?} evidence {} good {} {}", analysis.kind, analysis.evidence, Self::good(&analysis), detail.unwrap_or_default());
        }
        analysis
    }

    /// Good enough to stop trying other preparations?
    fn good(a: &Analysis) -> bool {
        match a.kind {
            ScreenKind::DriverLicence => a.licence.anchors >= 3 && a.licence.garage_power.is_some() && !a.licence.name.is_empty(),
            ScreenKind::Standings => a.standings.as_ref().is_some_and(|p| {
                let n = p.rows.len().max(1) as f32;
                let ranked = p.rows.iter().filter(|r| r.rank_read).count() as f32 / n;
                let named = p.rows.iter().filter(|r| !r.name.is_empty()).count() as f32 / n;
                let scored = p.rows.iter().filter(|r| r.score.is_some()).count() as f32 / n;
                p.rows.len() >= 3 && ranked >= 0.5 && named >= 0.9 && scored >= 0.9
            }),
            ScreenKind::Podium => a.standings.as_ref().is_some_and(|p| p.rows.len() >= 3),
            ScreenKind::Unknown => false,
        }
    }

    /// Quarter turns (0-3) that make the text upright, judged by how well the first few lines read.
    fn upright_rotation(&self, page: &Rgb) -> u32 {
        let mut best = (0u32, 0f32);
        let mut zero = 0f32;
        for q in 0..4u32 {
            let rotated = photo::rotate_quarter(page, q);
            let image = rotated.to_image();
            let mut quads = self.ocr.detect(&image);
            quads.sort_by(|a, b| {
                let wa = (a[1].x - a[0].x).abs();
                let wb = (b[1].x - b[0].x).abs();
                wb.partial_cmp(&wa).unwrap()
            });
            quads.truncate(16);
            let crops = quads.iter().map(|q| crop_quad(&image, q)).collect();
            let score: f32 = self
                .ocr
                .recognise(Script::Latin, crops)
                .iter()
                .filter(|(t, c)| *c > 0.55 && t.chars().filter(|c| c.is_alphanumeric()).count() >= 2)
                .map(|(t, c)| c * (t.chars().count().min(10) as f32))
                .sum();
            if q == 0 {
                zero = score;
            }
            if score > best.1 {
                best = (q, score);
            }
        }
        if best.1 > zero * 1.25 + 4.0 {
            best.0
        } else {
            0
        }
    }

    /// Read the whole screenshot (trying the cheap fixes first) and say what it is.
    pub fn analyse(&self, bytes: &[u8], known_teams: &[String]) -> Result<Analysis, String> {
        let started = Instant::now();
        self.ocr.recycle_if_large();
        let base = Self::decode(bytes)?;
        let mut best = self.attempt(base.clone(), "direct", known_teams);
        if Self::good(&best) {
            return Ok(best);
        }

        // A candidate replaces the best reading when it is good and the best is not, or when it clearly has more evidence.
        let consider = |candidate: Analysis, best: &mut Analysis| -> bool {
            let ok = Self::good(&candidate);
            let best_ok = Self::good(best);
            if (ok && !best_ok) || (ok == best_ok && candidate.evidence > best.evidence + (best.evidence / 8).max(2)) {
                *best = candidate;
            }
            ok
        };

        // Sideways / upside-down uploads first: every other fix assumes upright text.
        let mut working = base.clone();
        let quarters = self.upright_rotation(&base);
        if quarters != 0 {
            working = photo::rotate_quarter(&base, quarters);
            if consider(self.attempt(working.clone(), "rotated", known_teams), &mut best) {
                return Ok(best);
            }
        }

        let cropped = photo::autocrop(&working);
        if let Some(c) = &cropped {
            if consider(self.attempt(c.clone(), "borders-removed", known_teams), &mut best) {
                return Ok(best);
            }
        }
        let working = cropped.unwrap_or(working);

        if started.elapsed() < TIME_BUDGET {
            if let Some(flat) = photo::rectify_standings(&working) {
                if consider(self.attempt(flat, "table-flattened", known_teams), &mut best) {
                    return Ok(best);
                }
            }
        }
        if started.elapsed() < TIME_BUDGET {
            if let Some(flat) = photo::rectify_screen(&working) {
                if consider(self.attempt(flat.clone(), "screen-flattened", known_teams), &mut best) {
                    return Ok(best);
                }
                if let Some(table) = photo::rectify_standings(&flat) {
                    if consider(self.attempt(table, "screen+table-flattened", known_teams), &mut best) {
                        return Ok(best);
                    }
                }
            }
        }
        Ok(best)
    }

    /// A fast look at what a screenshot is, for deciding whether to let an applicant through without waiting for the
    /// full reading: upright screenshots of the profile or the team-event screens are recognised from their text
    /// labels alone. `Unknown` means "not recognised this way" (photos, sideways files, other images), not "not HCR2".
    pub fn classify_quick(&self, bytes: &[u8], cancel: &std::sync::atomic::AtomicBool) -> Result<ScreenKind, String> {
        self.ocr.recycle_if_large();
        let mut base = Self::decode(bytes)?;
        let mut turns = 0;
        loop {
            // Big enough that small text is legible, small enough to be quick.
            let long = base.w.max(base.h) as f32;
            let short = base.w.min(base.h) as f32;
            let side: f32 = std::env::var("DCA_QUICK_SIDE").ok().and_then(|v| v.parse().ok()).unwrap_or(960.0);
            let scale = (side / long).min(1.0).max(side * 0.5 / short).min(2.0);
            let page = if (scale - 1.0).abs() > 0.05 { base.resized((base.w as f32 * scale) as u32, (base.h as f32 * scale) as u32) } else { base.clone() };
            let (w, h) = (page.w as f32, page.h as f32);
            let decided = |lines: &[TextLine]| anchor_count(lines) >= 2 || looks_like_standings(lines) || looks_like_podium(lines, w, h);
            let boxes = std::env::var("DCA_QUICK_BOXES").ok().and_then(|v| v.parse().ok()).unwrap_or(48);
            let Some((lines, sideways)) = self.ocr.read_page_fast(&page, boxes, cancel, decided) else { return Err("cancelled".into()) };
            if sideways && turns < 2 {
                // Lying on its side: turn it (one way, then the other) and look again.
                base = photo::rotate_quarter(&base, if turns == 0 { 1 } else { 2 });
                turns += 1;
                continue;
            }
            if anchor_count(&lines) >= 2 {
                return Ok(ScreenKind::DriverLicence);
            }
            if looks_like_standings(&lines) {
                return Ok(ScreenKind::Standings);
            }
            if looks_like_podium(&lines, w, h) {
                return Ok(ScreenKind::Podium);
            }
            return Ok(ScreenKind::Unknown);
        }
    }

    /// Read one team-event screenshot with a single text pass (the dedicated spreadsheet channels only get these):
    /// the standings table or the result screen. Names the line pass was sure of are not read again. Only a
    /// screenshot that did not give a table (a photo, a sideways file) falls back to the full ladder.
    pub fn read_event_screenshot(&self, bytes: &[u8], known_teams: &[String]) -> Result<(StandingsPage, String), String> {
        self.ocr.recycle_if_large();
        let base = Self::decode(bytes)?;
        let page = photo::upscale_small(base.clone()).normalised(MAX_PAGE_HEIGHT);
        let lines = self.ocr.read_page(&page);
        let (w, h) = (page.w as f32, page.h as f32);
        if looks_like_standings(&lines) {
            if let Ok(parsed) = read_standings(&self.ocr, &self.names, &page, Some(lines.clone())) {
                if parsed.rows.len() >= 3 {
                    return Ok((parsed, "direct".into()));
                }
            }
        } else if looks_like_podium(&lines, w, h) {
            if let Some(podium) = read_podium(&self.names, &page, &lines) {
                return Ok((podium, "direct".into()));
            }
        }
        // Not a clean table: photographed, sideways, cropped oddly.
        let analysis = self.analyse(bytes, known_teams)?;
        match analysis.kind {
            ScreenKind::DriverLicence => Err("This is a driver's licence screenshot, not team-event standings.".into()),
            _ => {
                let method = analysis.method.clone();
                self.standings(&analysis).map(|p| (p, method))
            }
        }
    }

    /// Read the driver's licence once (one text pass; the name crop is read for the in-game name).
    pub fn read_licence_once(&self, bytes: &[u8], known_teams: &[String]) -> Result<LicenceReading, String> {
        self.ocr.recycle_if_large();
        let page = photo::upscale_small(Self::decode(bytes)?).normalised(MAX_PAGE_HEIGHT);
        let lines = self.ocr.read_page(&page);
        let reading = read_licence(&self.names, &page, &lines, known_teams);
        if reading.anchors < 2 {
            return Err("This does not look like a driver's licence screenshot.".into());
        }
        Ok(reading)
    }

    pub fn standings(&self, analysis: &Analysis) -> Result<StandingsPage, String> {
        match &analysis.standings {
            Some(page) => Ok(page.clone()),
            None => read_standings(&self.ocr, &self.names, &analysis.page, Some(analysis.lines.clone())),
        }
    }
}
