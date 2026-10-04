//! Pixel helpers for the local screenshot readers: decoding, cropping, colour classification and the
//! "ink" binarisation that makes Hill Climb Racing 2's outlined UI text legible for Tesseract.

use image::{imageops, DynamicImage, GrayImage, ImageDecoder, RgbImage};

#[derive(Clone, Debug)]
pub struct Rgb {
    pub w: u32,
    pub h: u32,
    pub data: Vec<u8>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

impl Rect {
    pub fn new(x: i64, y: i64, w: i64, h: i64, bounds_w: u32, bounds_h: u32) -> Rect {
        let x0 = x.clamp(0, bounds_w as i64);
        let y0 = y.clamp(0, bounds_h as i64);
        let x1 = (x + w).clamp(0, bounds_w as i64);
        let y1 = (y + h).clamp(0, bounds_h as i64);
        Rect { x: x0 as u32, y: y0 as u32, w: (x1 - x0).max(0) as u32, h: (y1 - y0).max(0) as u32 }
    }
    pub fn right(&self) -> u32 {
        self.x + self.w
    }
    pub fn bottom(&self) -> u32 {
        self.y + self.h
    }
}

impl Rgb {
    /// Decode any supported image, honouring EXIF orientation (phone screenshots/photos are sometimes rotated).
    pub fn decode(bytes: &[u8]) -> Result<Rgb, String> {
        let reader = image::ImageReader::new(std::io::Cursor::new(bytes))
            .with_guessed_format()
            .map_err(|e| format!("unreadable image: {e}"))?;
        let mut decoder = reader.into_decoder().map_err(|e| format!("unreadable image: {e}"))?;
        let orientation = decoder.orientation().ok();
        let mut img = DynamicImage::from_decoder(decoder).map_err(|e| format!("unreadable image: {e}"))?;
        if let Some(orientation) = orientation {
            img.apply_orientation(orientation);
        }
        let rgb: RgbImage = img.to_rgb8();
        Ok(Rgb { w: rgb.width(), h: rgb.height(), data: rgb.into_raw() })
    }

    #[inline]
    pub fn px(&self, x: u32, y: u32) -> [u8; 3] {
        let i = ((y as usize) * (self.w as usize) + x as usize) * 3;
        [self.data[i], self.data[i + 1], self.data[i + 2]]
    }

    pub fn crop(&self, rect: Rect) -> Rgb {
        let mut data = Vec::with_capacity((rect.w * rect.h * 3) as usize);
        for y in rect.y..rect.bottom() {
            let start = ((y as usize) * (self.w as usize) + rect.x as usize) * 3;
            data.extend_from_slice(&self.data[start..start + (rect.w as usize) * 3]);
        }
        Rgb { w: rect.w, h: rect.h, data }
    }

    pub fn to_image(&self) -> RgbImage {
        RgbImage::from_raw(self.w, self.h, self.data.clone()).expect("rgb buffer size")
    }

    pub fn resized(&self, new_w: u32, new_h: u32) -> Rgb {
        let resized = imageops::resize(&self.to_image(), new_w.max(1), new_h.max(1), imageops::FilterType::Triangle);
        Rgb { w: resized.width(), h: resized.height(), data: resized.into_raw() }
    }

    /// Bring very large uploads down to a size the readers are tuned for (tall side <= `max_h`).
    pub fn normalised(self, max_h: u32) -> Rgb {
        if self.h <= max_h {
            return self;
        }
        let scale = max_h as f64 / self.h as f64;
        self.resized(((self.w as f64) * scale).round() as u32, max_h)
    }
}

/// Colour classes of the standings bands.
pub const BAND_NONE: u8 = 0;
pub const BAND_YELLOW: u8 = 1;
pub const BAND_BLUE: u8 = 2;

fn hsv(r: u8, g: u8, b: u8) -> (f32, f32, f32) {
    let max = r.max(g).max(b) as f32;
    let min = r.min(g).min(b) as f32;
    let delta = max - min;
    if max <= 0.0 {
        return (0.0, 0.0, 0.0);
    }
    let sat = delta / max;
    if delta <= 0.0 {
        return (0.0, sat, max);
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
    (hue, sat, max)
}

/// Is this pixel part of a yellow (own team) or blue (opponent) standings band?
#[inline]
pub fn band_class(r: u8, g: u8, b: u8) -> u8 {
    let (hue, sat, val) = hsv(r, g, b);
    if val < 140.0 {
        return BAND_NONE;
    }
    if (38.0..=70.0).contains(&hue) && (0.25..=0.62).contains(&sat) {
        BAND_YELLOW
    } else if (195.0..=232.0).contains(&hue) && (0.38..=0.8).contains(&sat) {
        BAND_BLUE
    } else {
        BAND_NONE
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ink {
    White,
    Gold,
    WhiteOrGold,
}

#[inline]
pub fn is_white_ink(r: u8, g: u8, b: u8) -> bool {
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    max > 150 && ((max - min) as f32) / (max as f32) <= 0.22
}

#[inline]
pub fn is_gold_ink(r: u8, g: u8, b: u8) -> bool {
    let (hue, sat, val) = hsv(r, g, b);
    val > 170.0 && sat > 0.62 && (28.0..=56.0).contains(&hue)
}

/// Binarise a region: ink pixels become black (0) on a white (255) background, ready for Tesseract.
pub fn ink_gray(img: &Rgb, rect: Rect, ink: Ink) -> GrayImage {
    let mut out = GrayImage::from_pixel(rect.w.max(1), rect.h.max(1), image::Luma([255]));
    for y in 0..rect.h {
        for x in 0..rect.w {
            let [r, g, b] = img.px(rect.x + x, rect.y + y);
            let hit = match ink {
                Ink::White => is_white_ink(r, g, b),
                Ink::Gold => is_gold_ink(r, g, b),
                Ink::WhiteOrGold => is_white_ink(r, g, b) || is_gold_ink(r, g, b),
            };
            if hit {
                out.put_pixel(x, y, image::Luma([0]));
            }
        }
    }
    out
}

/// Count ink pixels of each kind in a region (used to decide white vs gold text).
pub fn ink_counts(img: &Rgb, rect: Rect) -> (u32, u32) {
    let (mut white, mut gold) = (0, 0);
    for y in rect.y..rect.bottom() {
        for x in rect.x..rect.right() {
            let [r, g, b] = img.px(x, y);
            if is_white_ink(r, g, b) {
                white += 1;
            } else if is_gold_ink(r, g, b) {
                gold += 1;
            }
        }
    }
    (white, gold)
}

/// Grow the dark (ink) pixels by `radius` to reconnect strokes broken by anti-aliasing/outline removal.
pub fn thicken(gray: &GrayImage, radius: i32) -> GrayImage {
    if radius <= 0 {
        return gray.clone();
    }
    let (w, h) = (gray.width() as i32, gray.height() as i32);
    let mut out = GrayImage::from_pixel(gray.width(), gray.height(), image::Luma([255]));
    for y in 0..h {
        for x in 0..w {
            if gray.get_pixel(x as u32, y as u32).0[0] != 0 {
                continue;
            }
            for dy in -radius..=radius {
                for dx in -radius..=radius {
                    let (nx, ny) = (x + dx, y + dy);
                    if nx >= 0 && ny >= 0 && nx < w && ny < h {
                        out.put_pixel(nx as u32, ny as u32, image::Luma([0]));
                    }
                }
            }
        }
    }
    out
}

/// Add a white margin (Tesseract reads text touching the border badly).
pub fn pad_gray(gray: &GrayImage, pad: u32) -> GrayImage {
    let mut out = GrayImage::from_pixel(gray.width() + pad * 2, gray.height() + pad * 2, image::Luma([255]));
    imageops::replace(&mut out, gray, pad as i64, pad as i64);
    out
}

/// Scale a binarised line so its height is about `target_h` px (OCR is most accurate around 40-70px glyph lines).
pub fn scale_to_height(gray: &GrayImage, target_h: u32) -> GrayImage {
    if gray.height() == 0 {
        return gray.clone();
    }
    let factor = target_h as f64 / gray.height() as f64;
    let new_w = ((gray.width() as f64) * factor).round().max(1.0) as u32;
    imageops::resize(gray, new_w, target_h.max(1), imageops::FilterType::Triangle)
}

/// Tight bounding box of the dark pixels of a binarised image, if any.
pub fn ink_bounds(gray: &GrayImage) -> Option<Rect> {
    let (mut x0, mut y0, mut x1, mut y1) = (u32::MAX, u32::MAX, 0u32, 0u32);
    let mut any = false;
    for (x, y, px) in gray.enumerate_pixels() {
        if px.0[0] == 0 {
            any = true;
            x0 = x0.min(x);
            y0 = y0.min(y);
            x1 = x1.max(x);
            y1 = y1.max(y);
        }
    }
    any.then(|| Rect { x: x0, y: y0, w: x1 - x0 + 1, h: y1 - y0 + 1 })
}

pub fn ink_pixel_count(gray: &GrayImage) -> u32 {
    gray.pixels().filter(|p| p.0[0] == 0).count() as u32
}
