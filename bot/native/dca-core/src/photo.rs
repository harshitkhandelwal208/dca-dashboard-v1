//! Making awkward uploads readable: letterboxed / bordered screenshots, photographs of a phone or tablet
//! (perspective, rotation) and sideways uploads. Everything here is plain pixel geometry.

use crate::imaging::*;

type P = (f32, f32);

// ------------------------------------------------------------------------------------------ borders

/// Remove uniform dark bars (letterboxing, device bezels filling the frame edges).
pub fn autocrop(img: &Rgb) -> Option<Rgb> {
    let line_is_border = |pixels: &mut dyn Iterator<Item = [u8; 3]>| -> bool {
        let (mut n, mut sum, mut sum_sq) = (0f64, 0f64, 0f64);
        for p in pixels {
            let y = 0.299 * p[0] as f64 + 0.587 * p[1] as f64 + 0.114 * p[2] as f64;
            n += 1.0;
            sum += y;
            sum_sq += y * y;
        }
        if n == 0.0 {
            return true;
        }
        let mean = sum / n;
        let var = (sum_sq / n - mean * mean).max(0.0);
        mean < 28.0 && var.sqrt() < 14.0
    };
    let step_x = (img.w / 200).max(1) as usize;
    let step_y = (img.h / 200).max(1) as usize;

    let mut top = 0;
    while top < img.h / 2 && line_is_border(&mut (0..img.w).step_by(step_x).map(|x| img.px(x, top))) {
        top += 1;
    }
    let mut bottom = img.h;
    while bottom > img.h / 2 && line_is_border(&mut (0..img.w).step_by(step_x).map(|x| img.px(x, bottom - 1))) {
        bottom -= 1;
    }
    let mut left = 0;
    while left < img.w / 2 && line_is_border(&mut (0..img.h).step_by(step_y).map(|y| img.px(left, y))) {
        left += 1;
    }
    let mut right = img.w;
    while right > img.w / 2 && line_is_border(&mut (0..img.h).step_by(step_y).map(|y| img.px(right - 1, y))) {
        right -= 1;
    }
    let (w, h) = (right.saturating_sub(left), bottom.saturating_sub(top));
    if w < img.w / 4 || h < img.h / 4 || (w >= img.w - 2 && h >= img.h - 2) {
        return None;
    }
    Some(img.crop(Rect { x: left, y: top, w, h }))
}

/// Scale a small image up so text is comfortably legible for the detector.
pub fn upscale_small(img: Rgb) -> Rgb {
    let short = img.h.min(img.w);
    if short >= 720 {
        return img;
    }
    // Narrow crops (a table cut out of a screenshot) have small text even when they are tall: scale by the short side.
    let factor = (800.0 / short as f32).clamp(1.0, 3.0);
    if factor <= 1.05 {
        return img;
    }
    img.resized((img.w as f32 * factor) as u32, (img.h as f32 * factor) as u32)
}

// -------------------------------------------------------------------------------------------- rotate

pub fn rotate_quarter(img: &Rgb, quarters: u32) -> Rgb {
    let rgb = img.to_image();
    let rotated = match quarters % 4 {
        1 => image::imageops::rotate90(&rgb),
        2 => image::imageops::rotate180(&rgb),
        3 => image::imageops::rotate270(&rgb),
        _ => rgb,
    };
    Rgb { w: rotated.width(), h: rotated.height(), data: rotated.into_raw() }
}

// ------------------------------------------------------------------------------------------ geometry

fn cross(o: P, a: P, b: P) -> f32 {
    (a.0 - o.0) * (b.1 - o.1) - (a.1 - o.1) * (b.0 - o.0)
}

pub fn convex_hull(mut pts: Vec<P>) -> Vec<P> {
    pts.sort_by(|a, b| a.partial_cmp(b).unwrap());
    pts.dedup();
    if pts.len() < 3 {
        return pts;
    }
    let mut lower: Vec<P> = Vec::new();
    for &p in &pts {
        while lower.len() >= 2 && cross(lower[lower.len() - 2], lower[lower.len() - 1], p) <= 0.0 {
            lower.pop();
        }
        lower.push(p);
    }
    let mut upper: Vec<P> = Vec::new();
    for &p in pts.iter().rev() {
        while upper.len() >= 2 && cross(upper[upper.len() - 2], upper[upper.len() - 1], p) <= 0.0 {
            upper.pop();
        }
        upper.push(p);
    }
    lower.pop();
    upper.pop();
    lower.extend(upper);
    lower
}

pub fn polygon_area(poly: &[P]) -> f32 {
    let mut a = 0.0;
    for i in 0..poly.len() {
        let (p, q) = (poly[i], poly[(i + 1) % poly.len()]);
        a += p.0 * q.1 - q.0 * p.1;
    }
    a.abs() / 2.0
}

/// Reduce a convex polygon to four corners by repeatedly dropping the vertex that changes the area least.
pub fn simplify_to_quad(mut poly: Vec<P>) -> Option<[P; 4]> {
    if poly.len() < 4 {
        return None;
    }
    while poly.len() > 4 {
        let n = poly.len();
        let mut best = (0usize, f32::MAX);
        for i in 0..n {
            let (a, b, c) = (poly[(i + n - 1) % n], poly[i], poly[(i + 1) % n]);
            let tri = cross(a, b, c).abs() / 2.0;
            if tri < best.1 {
                best = (i, tri);
            }
        }
        poly.remove(best.0);
    }
    // Order: top-left, top-right, bottom-right, bottom-left.
    let sum = |p: &P| p.0 + p.1;
    let diff = |p: &P| p.0 - p.1;
    let tl = *poly.iter().min_by(|a, b| sum(a).partial_cmp(&sum(b)).unwrap())?;
    let br = *poly.iter().max_by(|a, b| sum(a).partial_cmp(&sum(b)).unwrap())?;
    let tr = *poly.iter().max_by(|a, b| diff(a).partial_cmp(&diff(b)).unwrap())?;
    let bl = *poly.iter().min_by(|a, b| diff(a).partial_cmp(&diff(b)).unwrap())?;
    Some([tl, tr, br, bl])
}

fn solve8(mut a: [[f64; 9]; 8]) -> Option<[f64; 8]> {
    for i in 0..8 {
        let mut p = i;
        for r in i + 1..8 {
            if a[r][i].abs() > a[p][i].abs() {
                p = r;
            }
        }
        if a[p][i].abs() < 1e-12 {
            return None;
        }
        a.swap(i, p);
        for r in i + 1..8 {
            let f = a[r][i] / a[i][i];
            for c in i..9 {
                a[r][c] -= f * a[i][c];
            }
        }
    }
    let mut x = [0f64; 8];
    for i in (0..8).rev() {
        let mut s = a[i][8];
        for c in i + 1..8 {
            s -= a[i][c] * x[c];
        }
        x[i] = s / a[i][i];
    }
    Some(x)
}

/// Warp the quad `[tl, tr, br, bl]` of `src` into an `out_w` x `out_h` rectangle (bilinear sampling).
pub fn warp_quad(src: &Rgb, quad: [P; 4], out_w: u32, out_h: u32) -> Option<Rgb> {
    // Homography mapping output (x, y) -> source (u, v).
    let dst = [(0.0, 0.0), (out_w as f64, 0.0), (out_w as f64, out_h as f64), (0.0, out_h as f64)];
    let mut m = [[0f64; 9]; 8];
    for i in 0..4 {
        let (x, y) = dst[i];
        let (u, v) = (quad[i].0 as f64, quad[i].1 as f64);
        m[2 * i] = [x, y, 1.0, 0.0, 0.0, 0.0, -u * x, -u * y, u];
        m[2 * i + 1] = [0.0, 0.0, 0.0, x, y, 1.0, -v * x, -v * y, v];
    }
    let h = solve8(m)?;
    let mut out = Rgb { w: out_w, h: out_h, data: vec![0u8; (out_w * out_h * 3) as usize] };
    for y in 0..out_h {
        for x in 0..out_w {
            let (xf, yf) = (x as f64 + 0.5, y as f64 + 0.5);
            let d = h[6] * xf + h[7] * yf + 1.0;
            let u = ((h[0] * xf + h[1] * yf + h[2]) / d) as f32 - 0.5;
            let v = ((h[3] * xf + h[4] * yf + h[5]) / d) as f32 - 0.5;
            if u < 0.0 || v < 0.0 || u >= (src.w - 1) as f32 || v >= (src.h - 1) as f32 {
                continue;
            }
            let (x0, y0) = (u as u32, v as u32);
            let (fx, fy) = (u - x0 as f32, v - y0 as f32);
            let (a, b, c, d2) = (src.px(x0, y0), src.px(x0 + 1, y0), src.px(x0, y0 + 1), src.px(x0 + 1, y0 + 1));
            let i = ((y * out_w + x) * 3) as usize;
            for ch in 0..3 {
                let v = a[ch] as f32 * (1.0 - fx) * (1.0 - fy) + b[ch] as f32 * fx * (1.0 - fy) + c[ch] as f32 * (1.0 - fx) * fy + d2[ch] as f32 * fx * fy;
                out.data[i + ch] = v as u8;
            }
        }
    }
    Some(out)
}

fn dist(a: P, b: P) -> f32 {
    ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt()
}

fn lerp(a: P, b: P, t: f32) -> P {
    (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t)
}

/// Grow a quad outwards in its own (perspective) plane.
fn extend_quad(q: [P; 4], left: f32, right: f32, top: f32, bottom: f32) -> [P; 4] {
    let [tl, tr, br, bl] = q;
    let top_edge = |t: f32| (lerp(tl, bl, -top * 0.0 + t), lerp(tr, br, t));
    let _ = top_edge;
    // Move along the left/right edges for top/bottom, then along the top/bottom edges for left/right.
    let ntl = (tl.0 + (tl.0 - bl.0) * top, tl.1 + (tl.1 - bl.1) * top);
    let ntr = (tr.0 + (tr.0 - br.0) * top, tr.1 + (tr.1 - br.1) * top);
    let nbl = (bl.0 + (bl.0 - tl.0) * bottom, bl.1 + (bl.1 - tl.1) * bottom);
    let nbr = (br.0 + (br.0 - tr.0) * bottom, br.1 + (br.1 - tr.1) * bottom);
    let wl = |a: P, b: P| (a.0 + (a.0 - b.0) * left, a.1 + (a.1 - b.1) * left);
    let wr = |a: P, b: P| (a.0 + (a.0 - b.0) * right, a.1 + (a.1 - b.1) * right);
    [wl(ntl, ntr), wr(ntr, ntl), wr(nbr, nbl), wl(nbl, nbr)]
}

// ---------------------------------------------------------------------------------------- components

struct Grid {
    w: usize,
    h: usize,
    cells: Vec<bool>,
}

impl Grid {
    fn largest_component(&self) -> Vec<(usize, usize)> {
        let mut seen = vec![false; self.cells.len()];
        let mut best: Vec<(usize, usize)> = Vec::new();
        for start in 0..self.cells.len() {
            if !self.cells[start] || seen[start] {
                continue;
            }
            let mut comp: Vec<(usize, usize)> = Vec::new();
            let mut stack = vec![start];
            seen[start] = true;
            while let Some(i) = stack.pop() {
                let (x, y) = (i % self.w, i / self.w);
                comp.push((x, y));
                for (dx, dy) in [(-1i32, 0i32), (1, 0), (0, -1), (0, 1)] {
                    let (nx, ny) = (x as i32 + dx, y as i32 + dy);
                    if nx < 0 || ny < 0 || nx >= self.w as i32 || ny >= self.h as i32 {
                        continue;
                    }
                    let ni = ny as usize * self.w + nx as usize;
                    if self.cells[ni] && !seen[ni] {
                        seen[ni] = true;
                        stack.push(ni);
                    }
                }
            }
            if comp.len() > best.len() {
                best = comp;
            }
        }
        best
    }

    fn dilate(&self, r: i32) -> Grid {
        let mut out = vec![false; self.cells.len()];
        for y in 0..self.h as i32 {
            for x in 0..self.w as i32 {
                if !self.cells[y as usize * self.w + x as usize] {
                    continue;
                }
                for dy in -r..=r {
                    for dx in -r..=r {
                        let (nx, ny) = (x + dx, y + dy);
                        if nx >= 0 && ny >= 0 && nx < self.w as i32 && ny < self.h as i32 {
                            out[ny as usize * self.w + nx as usize] = true;
                        }
                    }
                }
            }
        }
        Grid { w: self.w, h: self.h, cells: out }
    }
}

fn small_copy(img: &Rgb, max_side: u32) -> (Rgb, f32) {
    let scale = (max_side as f32 / img.w.max(img.h) as f32).min(1.0);
    if scale >= 1.0 {
        return (img.clone(), 1.0);
    }
    (img.resized((img.w as f32 * scale) as u32, (img.h as f32 * scale) as u32), scale)
}

// ---------------------------------------------------------------------------- the standings table

fn is_band_pixel(r: u8, g: u8, b: u8) -> u8 {
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    if max < 70 {
        return 0;
    }
    let delta = (max - min) as f32;
    let sat = delta / max as f32;
    if sat < 0.25 {
        return 0;
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
    if (38.0..=72.0).contains(&hue) && sat < 0.7 {
        1
    } else if (196.0..=232.0).contains(&hue) && sat > 0.4 {
        2
    } else {
        0
    }
}

/// Rectify a photographed standings screen from the geometry of its yellow/blue row bands.
pub fn rectify_standings(img: &Rgb) -> Option<Rgb> {
    let (small, scale) = small_copy(img, 700);
    let step = 2usize;
    let (gw, gh) = (small.w as usize / step, small.h as usize / step);
    let mut cells = vec![false; gw * gh];
    for gy in 0..gh {
        for gx in 0..gw {
            let [r, g, b] = small.px((gx * step) as u32, (gy * step) as u32);
            cells[gy * gw + gx] = is_band_pixel(r, g, b) != 0;
        }
    }
    let grid = Grid { w: gw, h: gh, cells }.dilate(1);
    let comp = grid.largest_component();
    if comp.len() < (gw * gh) / 60 {
        return None;
    }
    let pts: Vec<P> = comp.iter().map(|&(x, y)| ((x * step) as f32 / scale, (y * step) as f32 / scale)).collect();
    let quad = simplify_to_quad(convex_hull(pts))?;
    let area = polygon_area(&quad);
    if area < (img.w * img.h) as f32 * 0.04 {
        return None;
    }
    let width = dist(quad[0], quad[1]).max(dist(quad[3], quad[2]));
    let height = dist(quad[0], quad[3]).max(dist(quad[1], quad[2]));
    if width < 100.0 || height < 60.0 {
        return None;
    }
    // The bands are the table: pull in the header above it and the score/rewards area around it.
    let extended = extend_quad(quad, 0.10, 0.55, 0.62, 0.12);
    let out_w = (width * 1.65).clamp(600.0, 2400.0) as u32;
    let out_h = (height * 1.74 * (out_w as f32 / (width * 1.65))).clamp(400.0, 2000.0) as u32;
    warp_quad(img, extended, out_w, out_h)
}

// --------------------------------------------------------------------------------------- the screen

/// Find the bright screen of a photographed device and flatten it.
pub fn rectify_screen(img: &Rgb) -> Option<Rgb> {
    let (small, scale) = small_copy(img, 480);
    let (gw, gh) = (small.w as usize, small.h as usize);
    let luma: Vec<f32> = (0..gw * gh)
        .map(|i| {
            let p = small.px((i % gw) as u32, (i / gw) as u32);
            0.299 * p[0] as f32 + 0.587 * p[1] as f32 + 0.114 * p[2] as f32
        })
        .collect();

    // Otsu threshold.
    let mut hist = [0u32; 256];
    for &l in &luma {
        hist[l.clamp(0.0, 255.0) as usize] += 1;
    }
    let total = luma.len() as f64;
    let sum_all: f64 = hist.iter().enumerate().map(|(i, &c)| i as f64 * c as f64).sum();
    let (mut sum_b, mut w_b, mut best_t, mut best_var) = (0.0, 0.0, 0usize, 0.0);
    for t in 0..256 {
        w_b += hist[t] as f64;
        if w_b == 0.0 {
            continue;
        }
        let w_f = total - w_b;
        if w_f == 0.0 {
            break;
        }
        sum_b += t as f64 * hist[t] as f64;
        let (m_b, m_f) = (sum_b / w_b, (sum_all - sum_b) / w_f);
        let var = w_b * w_f * (m_b - m_f).powi(2);
        if var > best_var {
            best_var = var;
            best_t = t;
        }
    }
    let threshold = (best_t as f32 * 0.85).max(35.0);
    let cells: Vec<bool> = luma.iter().map(|&l| l > threshold).collect();
    let grid = Grid { w: gw, h: gh, cells }.dilate(2);
    let comp = grid.largest_component();
    if comp.len() < gw * gh / 8 {
        return None;
    }
    let pts: Vec<P> = comp.iter().map(|&(x, y)| (x as f32 / scale, y as f32 / scale)).collect();
    let quad = simplify_to_quad(convex_hull(pts))?;
    if polygon_area(&quad) < (img.w * img.h) as f32 * 0.15 {
        return None;
    }
    let width = dist(quad[0], quad[1]).max(dist(quad[3], quad[2]));
    let height = dist(quad[0], quad[3]).max(dist(quad[1], quad[2]));
    let aspect = width / height.max(1.0);
    if !(0.4..=3.2).contains(&aspect) {
        return None;
    }
    let out_w = width.clamp(500.0, 2200.0) as u32;
    let out_h = (out_w as f32 / aspect).clamp(300.0, 2200.0) as u32;
    // Pull the corners in a touch: the dilation added a margin.
    let inset = extend_quad(quad, -0.006, -0.006, -0.006, -0.006);
    warp_quad(img, inset, out_w, out_h)
}
