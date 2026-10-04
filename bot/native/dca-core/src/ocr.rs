//! Local text reading with PaddleOCR (PP-OCRv5) models running on ONNX Runtime.
//!
//! * one DB text detector finds text in any orientation / aspect ratio,
//! * recognisers per script (Latin, multilingual CJK, Cyrillic, Arabic, Korean, Thai, Greek, Devanagari,
//!   Tamil, Telugu) read the cropped lines; the non-Latin ones are loaded the first time they are needed.

use crate::imaging::Rgb;
use image::RgbImage;
use oar_ocr::core::config::OrtSessionConfig;
use oar_ocr::predictors::{TextDetectionPredictor, TextRecognitionPredictor};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub enum Script {
    /// PP-OCRv5 "ch": Simplified + Traditional Chinese, Japanese, English, pinyin.
    Multi,
    Latin,
    Cyrillic,
    Arabic,
    Korean,
    Thai,
    Greek,
    Devanagari,
    Tamil,
    Telugu,
}

impl Script {
    pub const ALL: [Script; 10] = [
        Script::Multi,
        Script::Latin,
        Script::Cyrillic,
        Script::Arabic,
        Script::Korean,
        Script::Thai,
        Script::Greek,
        Script::Devanagari,
        Script::Tamil,
        Script::Telugu,
    ];

    /// Scripts tried (after Latin + Multi) when a name does not look Latin/CJK.
    pub const ESCALATION: [Script; 8] = [
        Script::Cyrillic,
        Script::Arabic,
        Script::Korean,
        Script::Thai,
        Script::Greek,
        Script::Devanagari,
        Script::Tamil,
        Script::Telugu,
    ];

    pub fn files(self) -> (&'static str, &'static str) {
        match self {
            Script::Multi => ("pp-ocrv5_mobile_rec.onnx", "ppocrv5_dict.txt"),
            Script::Latin => ("latin_pp-ocrv5_mobile_rec.onnx", "ppocrv5_latin_dict.txt"),
            Script::Cyrillic => ("cyrillic_pp-ocrv5_mobile_rec.onnx", "ppocrv5_cyrillic_dict.txt"),
            Script::Arabic => ("arabic_pp-ocrv5_mobile_rec.onnx", "ppocrv5_arabic_dict.txt"),
            Script::Korean => ("korean_pp-ocrv5_mobile_rec.onnx", "ppocrv5_korean_dict.txt"),
            Script::Thai => ("th_pp-ocrv5_mobile_rec.onnx", "ppocrv5_th_dict.txt"),
            Script::Greek => ("el_pp-ocrv5_mobile_rec.onnx", "ppocrv5_el_dict.txt"),
            Script::Devanagari => ("devanagari_pp-ocrv5_mobile_rec.onnx", "ppocrv5_devanagari_dict.txt"),
            Script::Tamil => ("ta_pp-ocrv5_mobile_rec.onnx", "ppocrv5_ta_dict.txt"),
            Script::Telugu => ("te_pp-ocrv5_mobile_rec.onnx", "ppocrv5_te_dict.txt"),
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Script::Multi => "multi",
            Script::Latin => "latin",
            Script::Cyrillic => "cyrillic",
            Script::Arabic => "arabic",
            Script::Korean => "korean",
            Script::Thai => "thai",
            Script::Greek => "greek",
            Script::Devanagari => "devanagari",
            Script::Tamil => "tamil",
            Script::Telugu => "telugu",
        }
    }

    /// Does this character belong to the script this model reads?
    pub fn owns(self, c: char) -> bool {
        let u = c as u32;
        match self {
            Script::Multi => {
                matches!(u, 0x4E00..=0x9FFF | 0x3400..=0x4DBF | 0x3040..=0x30FF | 0x31F0..=0x31FF | 0xFF00..=0xFFEF | 0x3000..=0x303F)
            }
            Script::Latin => c.is_alphabetic() && (u < 0x250 || (0x1E00..=0x1EFF).contains(&u)),
            Script::Cyrillic => matches!(u, 0x0400..=0x052F),
            Script::Arabic => matches!(u, 0x0600..=0x06FF | 0x0750..=0x077F | 0xFB50..=0xFDFF | 0xFE70..=0xFEFF),
            Script::Korean => matches!(u, 0xAC00..=0xD7AF | 0x1100..=0x11FF | 0x3130..=0x318F),
            Script::Thai => matches!(u, 0x0E00..=0x0E7F),
            Script::Greek => matches!(u, 0x0370..=0x03FF | 0x1F00..=0x1FFF),
            Script::Devanagari => matches!(u, 0x0900..=0x097F),
            Script::Tamil => matches!(u, 0x0B80..=0x0BFF),
            Script::Telugu => matches!(u, 0x0C00..=0x0C7F),
        }
    }
}

/// Every model file the bot needs (detector + all recognisers + dictionaries), as `(file name)`.
pub fn required_files() -> Vec<&'static str> {
    let mut files = vec![DET_MODEL];
    for script in Script::ALL {
        let (model, dict) = script.files();
        files.push(model);
        files.push(dict);
    }
    files
}

/// Files needed to start (detector + Latin + multilingual); the rest are loaded lazily.
pub fn core_files() -> Vec<&'static str> {
    let mut files = vec![DET_MODEL];
    for script in [Script::Latin, Script::Multi] {
        let (model, dict) = script.files();
        files.push(model);
        files.push(dict);
    }
    files
}

const REC_BATCH: usize = 6;

pub const DET_MODEL: &str = "pp-ocrv5_mobile_det.onnx";

/// Where model files are published (same files the oar-ocr project mirrors).
pub const MODEL_BASE_URL: &str = "https://github.com/GreatV/oar-ocr/releases/download/v0.3.0";

#[derive(Clone, Copy, Debug)]
pub struct Pt {
    pub x: f32,
    pub y: f32,
}

/// A detected + recognised piece of text.
#[derive(Clone, Debug)]
pub struct TextLine {
    /// Corners: top-left, top-right, bottom-right, bottom-left.
    pub quad: [Pt; 4],
    /// Axis-aligned bounds.
    pub x0: f32,
    pub y0: f32,
    pub x1: f32,
    pub y1: f32,
    pub text: String,
    pub confidence: f32,
    pub script: Script,
}

impl TextLine {
    pub fn cx(&self) -> f32 {
        (self.x0 + self.x1) / 2.0
    }
    pub fn cy(&self) -> f32 {
        (self.y0 + self.y1) / 2.0
    }
    pub fn w(&self) -> f32 {
        self.x1 - self.x0
    }
    pub fn h(&self) -> f32 {
        self.y1 - self.y0
    }
}

pub struct Ocr {
    dir: PathBuf,
    det: Mutex<TextDetectionPredictor>,
    recs: Mutex<HashMap<Script, Arc<Mutex<TextRecognitionPredictor>>>>,
    /// Lazily loaded recognisers, least recently used first (only a few stay in memory).
    recent: Mutex<Vec<Script>>,
}

fn quad_of(points: &[oar_ocr::processors::Point]) -> Option<[Pt; 4]> {
    if points.len() < 4 {
        return None;
    }
    // Order as tl, tr, br, bl by sum/diff of coordinates.
    let pts: Vec<Pt> = points.iter().map(|p| Pt { x: p.x, y: p.y }).collect();
    let tl = *pts.iter().min_by(|a, b| (a.x + a.y).partial_cmp(&(b.x + b.y)).unwrap())?;
    let br = *pts.iter().max_by(|a, b| (a.x + a.y).partial_cmp(&(b.x + b.y)).unwrap())?;
    let tr = *pts.iter().max_by(|a, b| (a.x - a.y).partial_cmp(&(b.x - b.y)).unwrap())?;
    let bl = *pts.iter().min_by(|a, b| (a.x - a.y).partial_cmp(&(b.x - b.y)).unwrap())?;
    Some([tl, tr, br, bl])
}

fn lerp(a: Pt, b: Pt, t: f32) -> Pt {
    Pt { x: a.x + (b.x - a.x) * t, y: a.y + (b.y - a.y) * t }
}

fn dist(a: Pt, b: Pt) -> f32 {
    ((a.x - b.x).powi(2) + (a.y - b.y).powi(2)).sqrt()
}

fn sample(img: &RgbImage, x: f32, y: f32) -> [f32; 3] {
    let (w, h) = (img.width() as i64, img.height() as i64);
    let xf = x.floor();
    let yf = y.floor();
    let fx = x - xf;
    let fy = y - yf;
    let get = |ix: i64, iy: i64| -> [f32; 3] {
        let p = img.get_pixel(ix.clamp(0, w - 1) as u32, iy.clamp(0, h - 1) as u32).0;
        [p[0] as f32, p[1] as f32, p[2] as f32]
    };
    let (x0, y0) = (xf as i64, yf as i64);
    let (a, b, c, d) = (get(x0, y0), get(x0 + 1, y0), get(x0, y0 + 1), get(x0 + 1, y0 + 1));
    let mut out = [0f32; 3];
    for i in 0..3 {
        out[i] = a[i] * (1.0 - fx) * (1.0 - fy) + b[i] * fx * (1.0 - fy) + c[i] * (1.0 - fx) * fy + d[i] * fx * fy;
    }
    out
}

/// Cut a (possibly rotated / skewed) text quad out of the page and straighten it.
pub fn crop_quad(img: &RgbImage, quad: &[Pt; 4]) -> RgbImage {
    let [tl, tr, br, bl] = *quad;
    let w = dist(tl, tr).max(dist(bl, br)).round().max(2.0) as u32;
    let h = dist(tl, bl).max(dist(tr, br)).round().max(2.0) as u32;
    let mut out = RgbImage::new(w, h);
    for v in 0..h {
        let tv = (v as f32 + 0.5) / h as f32;
        let left = lerp(tl, bl, tv);
        let right = lerp(tr, br, tv);
        for u in 0..w {
            let p = lerp(left, right, (u as f32 + 0.5) / w as f32);
            let c = sample(img, p.x - 0.5, p.y - 0.5);
            out.put_pixel(u, v, image::Rgb([c[0] as u8, c[1] as u8, c[2] as u8]));
        }
    }
    // Tall narrow boxes are vertical text: lay them down.
    if h as f32 > w as f32 * 1.8 && w >= 8 {
        return image::imageops::rotate270(&out);
    }
    out
}

/// ONNX Runtime threads per model: `DCA_OCR_THREADS`, else what the container is allowed to use (a 0.1 CPU host gets 1).
fn session_config() -> OrtSessionConfig {
    let threads = std::env::var("DCA_OCR_THREADS").ok().and_then(|v| v.trim().parse::<usize>().ok()).filter(|n| *n > 0).unwrap_or_else(|| std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1).min(4));
    OrtSessionConfig::new().with_intra_threads(threads).with_inter_threads(1).with_memory_pattern(false)
}

fn build_detector(path: &Path) -> Result<TextDetectionPredictor, String> {
    TextDetectionPredictor::builder()
        .score_threshold(0.25)
        .box_threshold(0.5)
        .unclip_ratio(1.6)
        .with_ort_config(session_config())
        .build(path)
        .map_err(|e| format!("could not load the text detector: {e}"))
}

/// Resident memory of this process in MB.
pub fn rss_mb() -> u64 {
    std::fs::read_to_string("/proc/self/statm").ok().and_then(|s| s.split_whitespace().nth(1).and_then(|v| v.parse::<u64>().ok())).map(|pages| pages * 4096 / 1048576).unwrap_or(0)
}

/// The memory this process may use, in MB: the tightest of its own cgroup limit (a container, or a systemd
/// `MemoryMax=`) and the machine's RAM.
pub fn memory_limit_mb() -> Option<u64> {
    let mut limits: Vec<u64> = Vec::new();
    // This process's own cgroup (v2), and the legacy / container-root locations.
    if let Ok(text) = std::fs::read_to_string("/proc/self/cgroup") {
        if let Some(path) = text.lines().find_map(|l| l.strip_prefix("0::")) {
            let mut dir = std::path::PathBuf::from("/sys/fs/cgroup").join(path.trim_start_matches('/'));
            for _ in 0..4 {
                if let Ok(v) = std::fs::read_to_string(dir.join("memory.max")) {
                    if let Ok(bytes) = v.trim().parse::<u64>() {
                        limits.push(bytes / 1048576);
                    }
                }
                if !dir.pop() || dir == std::path::Path::new("/sys/fs") {
                    break;
                }
            }
        }
    }
    for path in ["/sys/fs/cgroup/memory.max", "/sys/fs/cgroup/memory/memory.limit_in_bytes"] {
        if let Ok(text) = std::fs::read_to_string(path) {
            if let Ok(bytes) = text.trim().parse::<u64>() {
                if bytes < (1u64 << 50) {
                    limits.push(bytes / 1048576);
                }
            }
        }
    }
    if let Ok(text) = std::fs::read_to_string("/proc/meminfo") {
        if let Some(kb) = text.lines().find_map(|l| l.strip_prefix("MemTotal:")).and_then(|v| v.split_whitespace().next()).and_then(|v| v.parse::<u64>().ok()) {
            limits.push(kb / 1024);
        }
    }
    limits.into_iter().filter(|m| *m > 0 && *m < (1 << 30)).min()
}

/// Resident size above which the OCR sessions are rebuilt before the next job (`DCA_OCR_RECYCLE_MB`; by default
/// 40% of the container's memory limit, so a 512 MB host recycles at ~205 MB).
fn recycle_threshold_mb() -> u64 {
    if let Some(v) = std::env::var("DCA_OCR_RECYCLE_MB").ok().and_then(|v| v.trim().parse::<u64>().ok()) {
        return v;
    }
    // A machine with plenty of memory (4 GB+) never needs to recycle.
    memory_limit_mb().filter(|limit| *limit <= 4096).map(|limit| (limit * 2 / 5).max(160)).unwrap_or(u64::MAX)
}

impl Ocr {
    /// ONNX Runtime keeps every buffer a session ever needed. When the process has grown past the threshold, throw the
    /// sessions away (the memory goes back to the system) and let them be rebuilt on demand.
    pub fn recycle_if_large(&self) {
        let threshold = recycle_threshold_mb();
        if threshold == u64::MAX || rss_mb() <= threshold {
            return;
        }
        let before = rss_mb();
        {
            let mut det = self.det.lock().unwrap();
            if let Ok(fresh) = build_detector(&self.dir.join(DET_MODEL)) {
                *det = fresh;
            }
        }
        self.recs.lock().unwrap().clear();
        self.recent.lock().unwrap().clear();
        // Hand the freed heap back to the system (glibc keeps it otherwise).
        #[cfg(all(target_os = "linux", target_env = "gnu"))]
        unsafe {
            libc::malloc_trim(0);
        }
        let _ = self.recogniser(Script::Latin);
        let _ = self.recogniser(Script::Multi);
        tracing::info!("[ocr] recycled the OCR sessions ({before} MB -> {} MB resident)", rss_mb());
        if std::env::var("DCA_OCR_DEBUG").is_ok() {
            eprintln!("  [ocr] recycled: {before} MB -> {} MB", rss_mb());
        }
    }

    /// Load the detector and the two always-on recognisers from `dir`.
    pub fn load(dir: &Path) -> Result<Ocr, String> {
        let det_path = dir.join(DET_MODEL);
        if !det_path.exists() {
            return Err(format!("OCR model {} not found in {}", DET_MODEL, dir.display()));
        }
        let det = build_detector(&det_path)?;

        let ocr = Ocr { dir: dir.to_path_buf(), det: Mutex::new(det), recs: Mutex::new(HashMap::new()), recent: Mutex::new(Vec::new()) };
        ocr.recogniser(Script::Latin)?;
        ocr.recogniser(Script::Multi)?;
        Ok(ocr)
    }

    pub fn has_script(&self, script: Script) -> bool {
        let (model, dict) = script.files();
        self.dir.join(model).exists() && self.dir.join(dict).exists()
    }

    fn recogniser(&self, script: Script) -> Result<Arc<Mutex<TextRecognitionPredictor>>, String> {
        let permanent = matches!(script, Script::Latin | Script::Multi);
        if let Some(existing) = self.recs.lock().unwrap().get(&script) {
            if !permanent {
                let mut recent = self.recent.lock().unwrap();
                recent.retain(|s| *s != script);
                recent.push(script);
            }
            return Ok(existing.clone());
        }
        let (model, dict) = script.files();
        let model_path = self.dir.join(model);
        let dict_path = self.dir.join(dict);
        if !model_path.exists() || !dict_path.exists() {
            return Err(format!("OCR model for {} is not installed", script.name()));
        }
        let predictor = TextRecognitionPredictor::builder()
            .dict_path(dict_path.as_path())
            .with_ort_config(session_config())
            .build(model_path.as_path())
            .map_err(|e| format!("could not load the {} recogniser: {e}", script.name()))?;
        let predictor = Arc::new(Mutex::new(predictor));
        self.recs.lock().unwrap().insert(script, predictor.clone());
        if !permanent {
            // Each loaded recogniser costs ~25 MB: keep the most recent few, drop the rest.
            let keep = std::env::var("DCA_OCR_KEEP_SCRIPTS").ok().and_then(|v| v.trim().parse::<usize>().ok()).unwrap_or(2).max(1);
            let mut recent = self.recent.lock().unwrap();
            recent.retain(|s| *s != script);
            recent.push(script);
            while recent.len() > keep {
                let evicted = recent.remove(0);
                self.recs.lock().unwrap().remove(&evicted);
            }
        }
        Ok(predictor)
    }

    /// Run the text detector.
    pub fn detect(&self, img: &RgbImage) -> Vec<[Pt; 4]> {
        // The detector's memory grows with the square of its input: look at a reduced copy (the text is cut out of
        // the full-size picture afterwards, so reading quality is not affected).
        let side = std::env::var("DCA_OCR_DET_SIDE").ok().and_then(|v| v.trim().parse::<u32>().ok()).filter(|v| *v >= 320).unwrap_or(1600);
        let longest = img.width().max(img.height());
        if longest > side {
            let scale = side as f32 / longest as f32;
            let small = image::imageops::resize(img, ((img.width() as f32 * scale).round() as u32).max(32), ((img.height() as f32 * scale).round() as u32).max(32), image::imageops::FilterType::Triangle);
            let (sx, sy) = (img.width() as f32 / small.width() as f32, img.height() as f32 / small.height() as f32);
            return self.detect_raw(&small).into_iter().map(|q| q.map(|p| Pt { x: p.x * sx, y: p.y * sy })).collect();
        }
        self.detect_raw(img)
    }

    fn detect_raw(&self, img: &RgbImage) -> Vec<[Pt; 4]> {
        let result = {
            let det = self.det.lock().unwrap();
            det.predict(vec![img.clone()])
        };
        match result {
            Ok(result) => result
                .detections
                .into_iter()
                .next()
                .unwrap_or_default()
                .into_iter()
                .filter_map(|d| quad_of(&d.bbox.points))
                .collect(),
            Err(error) => {
                tracing::warn!("text detection failed: {error}");
                Vec::new()
            }
        }
    }

    /// Read already cropped text lines with one script's recogniser. Lines are sorted by aspect ratio and run in
    /// small batches: a batch is padded to its widest line, so mixing short and long lines wastes most of the work.
    pub fn recognise(&self, script: Script, crops: Vec<RgbImage>) -> Vec<(String, f32)> {
        if crops.is_empty() {
            return Vec::new();
        }
        let count = crops.len();
        let Ok(rec) = self.recogniser(script) else {
            return vec![(String::new(), 0.0); count];
        };
        let mut order: Vec<usize> = (0..count).collect();
        order.sort_by(|&a, &b| {
            let ra = crops[a].width() as f32 / crops[a].height().max(1) as f32;
            let rb = crops[b].width() as f32 / crops[b].height().max(1) as f32;
            ra.partial_cmp(&rb).unwrap()
        });
        let mut out = vec![(String::new(), 0.0f32); count];
        let predictor = rec.lock().unwrap();
        for chunk in order.chunks(REC_BATCH) {
            let batch: Vec<RgbImage> = chunk.iter().map(|&i| crops[i].clone()).collect();
            match predictor.predict(batch) {
                Ok(result) => {
                    for (slot, (text, score)) in chunk.iter().zip(result.texts.into_iter().zip(result.scores)) {
                        out[*slot] = (text, score);
                    }
                }
                Err(error) => tracing::warn!("text recognition failed: {error}"),
            }
        }
        out
    }

    /// Cheap text pass for deciding what a screenshot is: Latin recogniser only, the `max_boxes` biggest boxes only,
    /// stoppable. `None` when `cancel` was raised.
    pub fn read_page_fast(&self, page: &Rgb, max_boxes: usize, cancel: &std::sync::atomic::AtomicBool, enough: impl Fn(&[TextLine]) -> bool) -> Option<(Vec<TextLine>, bool)> {
        use std::sync::atomic::Ordering;
        let image = page.to_image();
        let t_det = std::time::Instant::now();
        let mut quads = self.detect(&image);
        if std::env::var("DCA_OCR_DEBUG").is_ok() {
            eprintln!("  [ocr-fast] page {}x{} det {:?} -> {} boxes", page.w, page.h, t_det.elapsed(), quads.len());
        }
        if cancel.load(Ordering::Relaxed) {
            return None;
        }
        // Mostly tall boxes: the picture is lying on its side.
        let tall = quads.iter().filter(|q| dist(q[0], q[3]) > dist(q[0], q[1]) * 1.6).count();
        let sideways = quads.len() >= 8 && tall * 2 > quads.len();
        quads.sort_by(|a, b| {
            let area = |q: &[Pt; 4]| (dist(q[0], q[1]) * dist(q[0], q[3])).abs();
            area(b).partial_cmp(&area(a)).unwrap()
        });
        quads.truncate(max_boxes);
        if quads.is_empty() {
            return Some((Vec::new(), false));
        }
        if sideways {
            // Not worth reading: the caller turns the picture first.
            return Some((Vec::new(), true));
        }
        let crops: Vec<RgbImage> = quads.iter().map(|q| crop_quad(&image, q)).collect();
        let mut lines = Vec::with_capacity(quads.len());
        // Recognise in slices so a cancellation lands within a fraction of a second.
        let mut offset = 0;
        for chunk in crops.chunks(REC_BATCH) {
            if cancel.load(Ordering::Relaxed) {
                return None;
            }
            // Stop as soon as what has been read is enough to tell what the screenshot is.
            if offset >= REC_BATCH * 2 && enough(&lines) {
                break;
            }
            for (i, (text, confidence)) in self.recognise(Script::Latin, chunk.to_vec()).into_iter().enumerate() {
                let quad = quads[offset + i];
                let xs = quad.iter().map(|p| p.x);
                let ys = quad.iter().map(|p| p.y);
                lines.push(TextLine { quad, x0: xs.clone().fold(f32::MAX, f32::min), x1: xs.fold(f32::MIN, f32::max), y0: ys.clone().fold(f32::MAX, f32::min), y1: ys.fold(f32::MIN, f32::max), text: text.trim().to_string(), confidence, script: Script::Latin });
            }
            offset += chunk.len();
        }
        Some((lines, false))
    }

    /// Detect and read all text of a page (Latin + multilingual models, best reading kept per line).
    pub fn read_page(&self, page: &Rgb) -> Vec<TextLine> {
        let image = page.to_image();
        let t0 = std::time::Instant::now();
        let quads = self.detect(&image);
        let t_det = t0.elapsed();
        if quads.is_empty() {
            return Vec::new();
        }
        let crops: Vec<RgbImage> = quads.iter().map(|q| crop_quad(&image, q)).collect();
        let t1 = std::time::Instant::now();
        let latin = self.recognise(Script::Latin, crops.clone());
        let t_latin = t1.elapsed();
        // The multilingual model only gets the lines the Latin one is unsure of (half the work on a clean screenshot).
        let doubtful: Vec<usize> = (0..crops.len()).filter(|&i| latin.get(i).map_or(true, |(t, c)| *c < 0.85 || t.trim().chars().filter(|c| c.is_alphanumeric()).count() < 2)).collect();
        let mut multi = vec![(String::new(), 0.0f32); crops.len()];
        for (slot, result) in doubtful.iter().zip(self.recognise(Script::Multi, doubtful.iter().map(|&i| crops[i].clone()).collect())) {
            multi[*slot] = result;
        }
        if std::env::var("DCA_OCR_DEBUG").is_ok() {
            eprintln!("  [ocr] det {:?} ({} boxes), latin {:?}, multi {:?}", t_det, quads.len(), t_latin, t1.elapsed() - t_latin);
        }

        let mut lines = Vec::with_capacity(quads.len());
        for (i, quad) in quads.iter().enumerate() {
            let (lt, lc) = latin.get(i).cloned().unwrap_or_default();
            let (mt, mc) = multi.get(i).cloned().unwrap_or_default();
            let multi_has_cjk = mt.chars().any(|c| Script::Multi.owns(c));
            let (text, confidence, script) = if multi_has_cjk && mc >= 0.55 {
                (mt, mc, Script::Multi)
            } else if lc + 0.03 >= mc {
                (lt, lc, Script::Latin)
            } else {
                (mt, mc, Script::Multi)
            };
            let xs = quad.iter().map(|p| p.x);
            let ys = quad.iter().map(|p| p.y);
            lines.push(TextLine {
                quad: *quad,
                x0: xs.clone().fold(f32::MAX, f32::min),
                x1: xs.fold(f32::MIN, f32::max),
                y0: ys.clone().fold(f32::MAX, f32::min),
                y1: ys.fold(f32::MIN, f32::max),
                text: text.trim().to_string(),
                confidence,
                script,
            });
        }
        lines
    }
}
