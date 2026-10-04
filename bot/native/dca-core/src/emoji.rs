//! Emoji in player names. Text recognisers cannot read emoji, so they are located as colourful glyph-sized blobs
//! inside a name and matched against every glyph of the bundled Noto Color Emoji font (shape + colour).

use crate::imaging::*;
use image::{imageops, RgbaImage};
use std::path::Path;

const GRID: u32 = 28;

struct Template {
    glyph: String,
    /// Opaque-silhouette mask on the GRID x GRID matching grid.
    mask: Vec<bool>,
    /// Colour on the same grid (only meaningful where `mask`).
    rgb: Vec<[f32; 3]>,
}

pub struct EmojiIndex {
    templates: Vec<Template>,
}

#[derive(Clone, Copy, Debug)]
pub struct Blob {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

fn tight_bounds(img: &RgbaImage) -> Option<(u32, u32, u32, u32)> {
    let (mut x0, mut y0, mut x1, mut y1) = (u32::MAX, u32::MAX, 0, 0);
    let mut any = false;
    for (x, y, p) in img.enumerate_pixels() {
        if p.0[3] > 40 {
            any = true;
            x0 = x0.min(x);
            y0 = y0.min(y);
            x1 = x1.max(x);
            y1 = y1.max(y);
        }
    }
    any.then(|| (x0, y0, x1 - x0 + 1, y1 - y0 + 1))
}

fn to_template(glyph: String, img: &RgbaImage) -> Option<Template> {
    let (x, y, w, h) = tight_bounds(img)?;
    let cropped = imageops::crop_imm(img, x, y, w, h).to_image();
    let resized = imageops::resize(&cropped, GRID, GRID, imageops::FilterType::Triangle);
    let mut mask = Vec::with_capacity((GRID * GRID) as usize);
    let mut rgb = Vec::with_capacity((GRID * GRID) as usize);
    for p in resized.pixels() {
        mask.push(p.0[3] > 110);
        rgb.push([p.0[0] as f32, p.0[1] as f32, p.0[2] as f32]);
    }
    Some(Template { glyph, mask, rgb })
}

impl EmojiIndex {
    /// Build the index from a colour emoji font (CBDT/CBLC bitmap strikes, e.g. NotoColorEmoji.ttf).
    pub fn load(font_path: &Path) -> Result<EmojiIndex, String> {
        let data = std::fs::read(font_path).map_err(|e| format!("cannot read emoji font {}: {e}", font_path.display()))?;
        let face = ttf_parser::Face::parse(&data, 0).map_err(|e| format!("invalid emoji font: {e}"))?;
        let mut templates = Vec::new();
        let mut seen = std::collections::HashSet::new();

        if let Some(cmap) = face.tables().cmap {
            for subtable in cmap.subtables {
                if !subtable.is_unicode() {
                    continue;
                }
                subtable.codepoints(|cp| {
                    let Some(ch) = char::from_u32(cp) else { return };
                    // Skip plain ASCII/letters; keep symbols, pictographs and regional/keycap-free singles.
                    if cp < 0xA9 || (ch.is_alphabetic() && cp < 0x2000) || matches!(cp, 0xFE0F | 0x200D | 0x20E3) {
                        return;
                    }
                    if !seen.insert(cp) {
                        return;
                    }
                    let Some(gid) = subtable.glyph_index(cp) else { return };
                    let Some(raster) = face.glyph_raster_image(gid, 128) else { return };
                    if raster.format != ttf_parser::RasterImageFormat::PNG {
                        return;
                    }
                    let Ok(decoded) = image::load_from_memory_with_format(raster.data, image::ImageFormat::Png) else { return };
                    if let Some(template) = to_template(ch.to_string(), &decoded.to_rgba8()) {
                        templates.push(template);
                    }
                });
            }
        }
        // Country flags are ligatures of two regional-indicator letters (GSUB), not cmap entries.
        if let (Some(gsub), Some(cmap)) = (face.tables().gsub, face.tables().cmap) {
            let indicator = |letter: u32| cmap.subtables.into_iter().filter(|t| t.is_unicode()).find_map(|t| t.glyph_index(0x1F1E6 + letter));
            let glyphs: Vec<Option<ttf_parser::GlyphId>> = (0..26).map(indicator).collect();
            for lookup in gsub.lookups {
                for subtable in lookup.subtables.into_iter::<ttf_parser::gsub::SubstitutionSubtable>() {
                    let ttf_parser::gsub::SubstitutionSubtable::Ligature(ligatures) = subtable else { continue };
                    for (first_letter, first) in glyphs.iter().enumerate() {
                        let Some(first) = first else { continue };
                        let Some(index) = ligatures.coverage.get(*first) else { continue };
                        let Some(set) = ligatures.ligature_sets.get(index) else { continue };
                        for i in 0..set.len() {
                            let Some(ligature) = set.get(i) else { continue };
                            if ligature.components.len() != 1 {
                                continue;
                            }
                            let Some(second) = ligature.components.get(0) else { continue };
                            let Some(second_letter) = glyphs.iter().position(|g| *g == Some(second)) else { continue };
                            let name: String = [0x1F1E6 + first_letter as u32, 0x1F1E6 + second_letter as u32].iter().filter_map(|c| char::from_u32(*c)).collect();
                            if !seen.insert(0x4000_0000u32 + (first_letter as u32) * 26 + second_letter as u32) {
                                continue;
                            }
                            let Some(raster) = face.glyph_raster_image(ligature.glyph, 128) else { continue };
                            if raster.format != ttf_parser::RasterImageFormat::PNG {
                                continue;
                            }
                            let Ok(decoded) = image::load_from_memory_with_format(raster.data, image::ImageFormat::Png) else { continue };
                            if let Some(template) = to_template(name, &decoded.to_rgba8()) {
                                templates.push(template);
                            }
                        }
                    }
                }
            }
        }
        if std::env::var("DCA_OCR_DEBUG").is_ok() {
            eprintln!(
                "emoji templates: {} ({} flags)",
                templates.len(),
                templates.iter().filter(|t| t.glyph.chars().count() == 2 && t.glyph.chars().all(|c| ('\u{1F1E6}'..='\u{1F1FF}').contains(&c))).count()
            );
        }
        if templates.is_empty() {
            return Err("the emoji font has no bitmap glyphs".into());
        }
        Ok(EmojiIndex { templates })
    }

    pub fn len(&self) -> usize {
        self.templates.len()
    }

    pub fn is_empty(&self) -> bool {
        self.templates.is_empty()
    }

    /// Background colour of a region: the dominant yellow/blue band tone, else the median of the border pixels.
    fn background(page: &Rgb, rect: Rect) -> [f32; 3] {
        let mut samples: Vec<[u8; 3]> = Vec::new();
        let step_x = (rect.w / 40).max(1);
        let step_y = (rect.h / 12).max(1);
        let mut y = rect.y;
        while y < rect.bottom() {
            let mut x = rect.x;
            while x < rect.right() {
                let px = page.px(x, y);
                if band_class(px[0], px[1], px[2]) != BAND_NONE {
                    samples.push(px);
                }
                x += step_x;
            }
            y += step_y;
        }
        if samples.len() < 8 {
            samples.clear();
            for x in (rect.x..rect.right()).step_by(step_x as usize) {
                samples.push(page.px(x, rect.y));
                samples.push(page.px(x, rect.bottom() - 1));
            }
        }
        let median = |ch: usize| {
            let mut v: Vec<u8> = samples.iter().map(|p| p[ch]).collect();
            v.sort_unstable();
            v.get(v.len() / 2).copied().unwrap_or(128) as f32
        };
        [median(0), median(1), median(2)]
    }

    fn hs(r: u8, g: u8, b: u8) -> (f32, f32) {
        let max = r.max(g).max(b) as f32;
        let min = r.min(g).min(b) as f32;
        let delta = max - min;
        if max <= 0.0 || delta <= 0.0 {
            return (0.0, 0.0);
        }
        let (rf, gf, bf) = (r as f32, g as f32, b as f32);
        let mut hue = if max == rf {
            60.0 * (((gf - bf) / delta) % 6.0)
        } else if max == gf {
            60.0 * ((bf - rf) / delta + 2.0)
        } else {
            60.0 * ((rf - gf) / delta + 4.0)
        };
        if hue < 0.0 {
            hue += 360.0;
        }
        (hue, delta / max)
    }

    /// Does this pixel belong to an emoji (as opposed to the band, the text, its dark outline or the soft halo
    /// between them)? Halo pixels are band-coloured mixed with black or white, so they keep the band's hue.
    fn foreground(px: [u8; 3], bg: [f32; 3]) -> bool {
        let [r, g, b] = px;
        let max = r.max(g).max(b);
        if max < 70 {
            return false; // dark outline / shadow
        }
        if is_white_ink(r, g, b) {
            return false; // the text itself
        }
        let dist = (r as f32 - bg[0]).abs() + (g as f32 - bg[1]).abs() + (b as f32 - bg[2]).abs();
        if dist <= 70.0 {
            return false;
        }
        let (hue, sat) = Self::hs(r, g, b);
        let (bhue, bsat) = Self::hs(bg[0] as u8, bg[1] as u8, bg[2] as u8);
        let hue_diff = {
            let d = (hue - bhue).abs();
            d.min(360.0 - d)
        };
        // Same hue and no more saturated than the band (or just darker/lighter): a halo, not an emoji.
        if hue_diff < 14.0 && (sat - bsat).abs() < 0.22 {
            return false;
        }
        sat >= 0.18
    }

    /// Emoji-sized, colourful blobs inside a name crop, left to right.
    pub fn locate(&self, page: &Rgb, rect: Rect) -> Vec<Blob> {
        if rect.w < 12 || rect.h < 12 {
            return Vec::new();
        }
        let bg = Self::background(page, rect);
        let text_h = rect.h as f32 / 1.2;

        // Column occupancy of foreground pixels.
        let mut columns = vec![0u32; rect.w as usize];
        for x in 0..rect.w {
            for y in 0..rect.h {
                if Self::foreground(page.px(rect.x + x, rect.y + y), bg) {
                    columns[x as usize] += 1;
                }
            }
        }
        let min_count = (rect.h as f32 * 0.22) as u32;
        let mut blobs = Vec::new();
        let mut start: Option<usize> = None;
        for x in 0..=columns.len() {
            let on = x < columns.len() && columns[x] >= min_count.max(2);
            match (on, start) {
                (true, None) => start = Some(x),
                (false, Some(s)) => {
                    let width = (x - s) as f32;
                    if width >= text_h * 0.55 && width <= text_h * 1.9 {
                        // Vertical extent and fill ratio.
                        let (mut y0, mut y1, mut count, mut gold) = (u32::MAX, 0u32, 0u32, 0u32);
                        for cx in s..x {
                            for y in 0..rect.h {
                                let px = page.px(rect.x + cx as u32, rect.y + y);
                                if Self::foreground(px, bg) {
                                    y0 = y0.min(y);
                                    y1 = y1.max(y);
                                    count += 1;
                                    if is_gold_ink(px[0], px[1], px[2]) {
                                        gold += 1;
                                    }
                                }
                            }
                        }
                        if y1 > y0 {
                            let height = (y1 - y0 + 1) as f32;
                            let fill = count as f32 / (width * height);
                            // Gold lettering (the reader's own row) looks like a yellow emoji; letters are far less solid.
                            let needed = if gold as f32 > count as f32 * 0.5 { 0.55 } else { 0.32 };
                            if height >= text_h * 0.55 && fill >= needed {
                                blobs.push(Blob { x: rect.x + s as u32, y: rect.y + y0, w: width as u32, h: height as u32 });
                            }
                        }
                    }
                    start = None;
                }
                _ => {}
            }
        }
        blobs
    }

    /// Best matching emoji for a blob and a 0..1 similarity.
    pub fn identify(&self, page: &Rgb, blob: &Blob) -> Option<(String, f32)> {
        let rect = Rect::new(blob.x as i64 - 2, blob.y as i64 - 2, blob.w as i64 + 4, blob.h as i64 + 4, page.w, page.h);
        let bg = Self::background(page, rect);
        let crop = page.crop(rect);

        // Tight foreground box, then resample both mask and colours to the template grid.
        let (mut x0, mut y0, mut x1, mut y1) = (u32::MAX, u32::MAX, 0u32, 0u32);
        for y in 0..crop.h {
            for x in 0..crop.w {
                if Self::foreground(crop.px(x, y), bg) {
                    x0 = x0.min(x);
                    y0 = y0.min(y);
                    x1 = x1.max(x);
                    y1 = y1.max(y);
                }
            }
        }
        if x1 <= x0 || y1 <= y0 {
            return None;
        }
        let (bw, bh) = (x1 - x0 + 1, y1 - y0 + 1);
        let mut rgba = RgbaImage::new(bw, bh);
        for y in 0..bh {
            for x in 0..bw {
                let px = crop.px(x0 + x, y0 + y);
                let alpha = if Self::foreground(px, bg) { 255 } else { 0 };
                rgba.put_pixel(x, y, image::Rgba([px[0], px[1], px[2], alpha]));
            }
        }
        let probe = to_template(String::new(), &rgba)?;

        let mut best: Option<(&Template, f32)> = None;
        for template in &self.templates {
            let (mut inter, mut union) = (0u32, 0u32);
            let (mut colour_err, mut colour_n) = (0f32, 0u32);
            for i in 0..(GRID * GRID) as usize {
                let (a, b) = (template.mask[i], probe.mask[i]);
                if a && b {
                    inter += 1;
                    let d = (template.rgb[i][0] - probe.rgb[i][0]).abs() + (template.rgb[i][1] - probe.rgb[i][1]).abs() + (template.rgb[i][2] - probe.rgb[i][2]).abs();
                    colour_err += d / 765.0;
                    colour_n += 1;
                }
                if a || b {
                    union += 1;
                }
            }
            if union == 0 || colour_n == 0 {
                continue;
            }
            let iou = inter as f32 / union as f32;
            let colour = 1.0 - colour_err / colour_n as f32;
            let score = 0.55 * iou + 0.45 * colour;
            if best.is_none_or(|(_, s)| score > s) {
                best = Some((template, score));
            }
        }
        best.map(|(t, s)| (t.glyph.clone(), ((s - 0.45) / 0.45).clamp(0.0, 1.0)))
    }
}
