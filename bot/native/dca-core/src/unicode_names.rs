//! Multi-script Unicode normalization and escaping for player names and Discord.

use unicode_normalization::UnicodeNormalization;

pub const ESCALATION_LANGUAGES: &[&str] = &["rus", "chi_sim", "ara", "jpn", "kor", "tha", "hin", "ell", "heb"];

/// Strips zero-width and bidi characters.
pub fn strip_bidi_and_zero_width(s: &str) -> String {
    s.chars()
        .filter(|&c| {
            !matches!(
                c,
                '\u{200B}'..='\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}' | '\u{FEFF}'
            )
        })
        .collect()
}

/// Strip decorative badge symbols (™, ®, ©) without decomposing into letters.
pub fn strip_decorative_badges(s: &str) -> String {
    s.chars().filter(|&c| !matches!(c, '™' | '®' | '©')).collect()
}

/// Fold Latin diacritics while preserving non-Latin scripts intact.
pub fn fold_latin_diacritics(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        // If it's a Latin character with diacritics, decompose and remove combining marks
        if ('\u{00C0}'..='\u{024F}').contains(&c) || ('\u{1E00}'..='\u{1EFF}').contains(&c) {
            let nfd: String = c.to_string().nfd().collect();
            for nc in nfd.chars() {
                if !('\u{0300}'..='\u{036F}').contains(&nc) {
                    out.push(nc);
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Generate canonical playerKey for cross-page matching and indexing.
pub fn player_key(name: &str) -> String {
    let stripped_badges = strip_decorative_badges(name);
    let cleaned = strip_bidi_and_zero_width(&stripped_badges);
    let nfkc: String = cleaned.nfkc().collect();
    let folded = fold_latin_diacritics(&nfkc);
    let lower = folded.to_lowercase();

    // Keep letters, digits, and non-Latin alphabets
    let mut key = String::new();
    for c in lower.chars() {
        if c.is_alphanumeric() {
            key.push(c);
        }
    }

    if !key.is_empty() {
        return key;
    }

    // Emoji / symbol fallback
    let mut emoji_codes = Vec::new();
    for c in name.chars() {
        if !c.is_whitespace() && !matches!(c, '™' | '®' | '©') {
            emoji_codes.push(format!("{:x}", c as u32));
        }
    }

    if !emoji_codes.is_empty() {
        format!("sym_{}", emoji_codes.join("_"))
    } else {
        String::new()
    }
}

pub fn squash(name: &str) -> String {
    player_key(name)
}

/// Clean up common OCR noise, balance brackets, and normalize pipes in player names.
pub fn clean_player_name(raw: &str) -> String {
    let stripped = strip_bidi_and_zero_width(raw);
    let mut cleaned = stripped.replace('\r', "").replace(['\n', '\t'], " ");

    cleaned = cleaned.replace(" |", "|").replace("| ", "|");

    // Balance brackets
    let open_sq = cleaned.matches('[').count();
    let close_sq = cleaned.matches(']').count();
    if open_sq > close_sq {
        cleaned.push_str(&"]".repeat(open_sq - close_sq));
    } else if close_sq > open_sq {
        cleaned = format!("{}{}", "[".repeat(close_sq - open_sq), cleaned);
    }

    cleaned.trim().to_string()
}

/// Sanitize text for Excel XML (strip invalid XML 1.0 control characters).
pub fn sanitize_for_excel(text: &str) -> String {
    text.chars()
        .filter(|&c| {
            let u = c as u32;
            u == 0x9 || u == 0xA || u == 0xD || (0x20..=0xD7FF).contains(&u) || (0xE000..=0xFFFD).contains(&u)
        })
        .collect()
}

/// Escape text for Discord Markdown and mentions.
pub fn escape_for_discord(text: &str) -> String {
    text.replace('@', "@\u{200b}").replace('*', "\\*").replace('_', "\\_").replace('~', "\\~").replace('`', "\\`").replace('|', "\\|")
}

pub fn is_script_char(c: char, script: &str) -> bool {
    match script {
        "rus" => matches!(c, '\u{0400}'..='\u{04FF}' | '\u{0500}'..='\u{052F}'),
        "chi_sim" | "jpn" => matches!(c, '\u{4E00}'..='\u{9FFF}' | '\u{3400}'..='\u{4DBF}' | '\u{3040}'..='\u{30FF}'),
        "ara" => matches!(c, '\u{0600}'..='\u{06FF}' | '\u{0750}'..='\u{077F}'),
        "kor" => matches!(c, '\u{AC00}'..='\u{D7AF}' | '\u{1100}'..='\u{11FF}'),
        "tha" => matches!(c, '\u{0E00}'..='\u{0E7F}'),
        "hin" => matches!(c, '\u{0900}'..='\u{097F}'),
        "ell" => matches!(c, '\u{0370}'..='\u{03FF}'),
        "heb" => matches!(c, '\u{0590}'..='\u{05FF}'),
        _ => false,
    }
}

pub fn script_score(text: &str, script: &str) -> f64 {
    let total = text.chars().filter(|c| c.is_alphabetic()).count();
    if total == 0 {
        return 0.0;
    }
    let matched = text.chars().filter(|&c| is_script_char(c, script)).count();
    matched as f64 / total as f64
}

#[derive(Clone, Debug)]
pub struct ScriptCandidate {
    pub lang: String,
    pub text: String,
    pub confidence: f32,
}

pub fn select_best_script_candidate(candidates: &[ScriptCandidate]) -> Option<ScriptCandidate> {
    if candidates.is_empty() {
        return None;
    }

    let eng = candidates.iter().find(|c| c.lang == "eng");
    let mut best_escalation: Option<(&ScriptCandidate, f64)> = None;

    for cand in candidates {
        if cand.lang == "eng" {
            continue;
        }
        let score = script_score(&cand.text, &cand.lang);
        if score >= 0.5 && cand.confidence >= 60.0 {
            if let Some((_, best_s)) = best_escalation {
                if score > best_s || (score == best_s && cand.confidence > best_escalation.unwrap().0.confidence) {
                    best_escalation = Some((cand, score));
                }
            } else {
                best_escalation = Some((cand, score));
            }
        }
    }

    if let Some((best, _)) = best_escalation {
        Some(best.clone())
    } else {
        eng.cloned().or_else(|| candidates.first().cloned())
    }
}
