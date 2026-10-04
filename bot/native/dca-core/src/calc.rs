//! Per-event outputs: the Summary / Results / Attendance workbook (.xlsx or flat .fods), the spreadsheet image
//! and the summary chart. Port of `calcSpreadsheet.js`.

use crate::metrics::*;
use dca_state::models::SpreadsheetSession;
use rust_xlsxwriter::{Color, Format, FormatAlign, FormatBorder, Workbook, Worksheet};

pub const OWN_COLOR: &str = "#fff2cc";
pub const OPPONENT_COLOR: &str = "#dbeafe";
pub const HEADER_COLOR: &str = "#1f2937";
pub const ACCENT_GREEN: &str = "#d9ead3";
pub const ACCENT_RED: &str = "#fee2e2";
pub const ACCENT_BLUE: &str = "#dbeafe";
pub const KAB_COLOR: &str = "#0f7dba";
const IMAGE_HEADER_COLORS: [&str; 7] = ["#0f766e", "#2563eb", "#16a34a", "#7c3aed", "#0891b2", "#d97706", "#be123c"];

#[derive(Clone, Debug, PartialEq)]
pub enum Cell {
    N(f64),
    S(String),
    E,
}

impl Cell {
    pub fn text(&self) -> String {
        match self {
            Cell::N(n) => fmt_num(*n),
            Cell::S(s) => s.clone(),
            Cell::E => String::new(),
        }
    }
    pub fn number(&self) -> f64 {
        match self {
            Cell::N(n) => *n,
            Cell::S(s) => s.trim_end_matches('%').parse().unwrap_or(0.0),
            Cell::E => 0.0,
        }
    }
}

pub fn fmt_num(n: f64) -> String {
    if n.fract() == 0.0 && n.abs() < 1e15 {
        format!("{}", n as i64)
    } else {
        format!("{n}")
    }
}

fn num_or_empty(v: i64) -> Cell {
    if v == 0 {
        Cell::E
    } else {
        Cell::N(v as f64)
    }
}

pub fn escape_xml(value: &str) -> String {
    value.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;").replace('\'', "&apos;")
}

pub fn preview_text(value: &str, max: usize) -> String {
    let text = value.split_whitespace().collect::<Vec<_>>().join(" ");
    if text.chars().count() <= max {
        return text;
    }
    let keep = max.saturating_sub(3);
    format!("{}...", text.chars().take(keep).collect::<String>())
}

pub fn safe_sheet_name(value: &str) -> String {
    let cleaned: String = value.chars().map(|c| if matches!(c, ':' | '\\' | '/' | '?' | '*' | '[' | ']') { ' ' } else { c }).collect();
    let collapsed = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    let name: String = collapsed.chars().take(31).collect();
    if name.is_empty() {
        "Sheet".into()
    } else {
        name
    }
}

pub fn safe_file_name(value: &str, fallback: &str) -> String {
    let cleaned: String = value.chars().map(|c| if matches!(c, '/' | '\\' | '?' | '%' | '*' | ':' | '|' | '"' | '<' | '>') { '-' } else { c }).collect();
    let dashed = cleaned.split_whitespace().collect::<Vec<_>>().join("-");
    let trimmed = dashed.trim_matches('-');
    let out: String = trimmed.chars().take(90).collect();
    if out.is_empty() {
        fallback.to_string()
    } else {
        out
    }
}

// ------------------------------------------------------------------------------------------- summaries

pub struct EventSummary {
    pub event_name: String,
    pub team_name: String,
    pub opponent_teams: Vec<String>,
    pub own_players: usize,
    pub opponent_count: usize,
    pub own_points: i64,
    pub opponent_points: i64,
    pub own_score: i64,
    pub opponent_score: i64,
    pub own_team_score: i64,
    pub opponent_team_score: i64,
    pub kab_count: u32,
    pub top_driver: Option<(u32, String)>,
}

pub fn event_summary(session: &SpreadsheetSession) -> EventSummary {
    let stats = &session.stats;
    let kab = session_kab_map(session);
    let own = own_players(session);
    let opponents = opponent_players(session);
    let top = own.iter().min_by_key(|p| if p.rank == 0 { 999 } else { p.rank }).map(|p| (p.rank, p.player_name.clone()));
    EventSummary {
        event_name: event_name_for_session(session, "Team Event"),
        team_name: first_non_empty(&[&session.metadata.own_team_name, &session.team_name, &session.team_id, "Team"]),
        opponent_teams: session_opponent_teams(session),
        own_players: own.len(),
        opponent_count: opponent_count(session),
        own_points: if stats.own_points != 0 { stats.own_points } else { own.iter().map(|p| points_value(p)).sum() },
        opponent_points: if stats.opponent_points != 0 { stats.opponent_points } else { opponents.iter().map(|p| points_value(p)).sum() },
        own_score: if stats.own_score != 0 { stats.own_score } else { own.iter().map(|p| score_value(p)).sum() },
        opponent_score: if stats.opponent_score != 0 { stats.opponent_score } else { opponents.iter().map(|p| score_value(p)).sum() },
        own_team_score: team_score_from_metadata(session, true),
        opponent_team_score: team_score_from_metadata(session, false),
        kab_count: kab.values().sum(),
        top_driver: top,
    }
}

fn first_non_empty(values: &[&str]) -> String {
    values.iter().find(|v| !v.is_empty()).map(|v| v.to_string()).unwrap_or_default()
}

pub struct DisplayRow {
    pub own: bool,
    /// rank, player, team, type, points, score, blues killed, blue kill %, #KAB
    pub values: Vec<Cell>,
}

pub fn player_display_rows(session: &SpreadsheetSession) -> Vec<DisplayRow> {
    let kab = session_kab_map(session);
    let possible = opponent_count(session);
    let mut players: Vec<&dca_state::models::Player> = session.players.iter().collect();
    players.sort_by_key(|p| if p.rank == 0 { 999 } else { p.rank });
    players
        .into_iter()
        .map(|p| {
            let own = p.team_type == "own";
            let killed = if own { Some(blues_killed_for_rank(session, p.rank)) } else { None };
            let killed_pct = killed.map(|k| blue_kill_percent(k, possible));
            DisplayRow {
                own,
                values: vec![
                    if p.rank == 0 { Cell::E } else { Cell::N(p.rank as f64) },
                    Cell::S(p.player_name.clone()),
                    Cell::S(if p.team_label.is_empty() {
                        if own { first_non_empty(&[&session.team_name, &session.team_id, "Own team"]) } else { "Opponent".into() }
                    } else {
                        p.team_label.clone()
                    }),
                    Cell::S(if own { "Own".into() } else { "Opponent".into() }),
                    num_or_empty(points_value(p)),
                    num_or_empty(score_value(p)),
                    killed.map(|k| Cell::N(k as f64)).unwrap_or(Cell::E),
                    killed_pct.map(|k| Cell::S(format!("{}%", fmt_num(k)))).unwrap_or(Cell::E),
                    Cell::N(*kab.get(&p.player_name).unwrap_or(&0) as f64),
                ],
            }
        })
        .collect()
}

pub struct SheetRow {
    pub values: Vec<Cell>,
    pub header: bool,
    pub fill: Option<&'static str>,
    pub kab: bool,
}

fn s(v: &str) -> Cell {
    Cell::S(v.to_string())
}

pub fn summary_workbook_rows(session: &SpreadsheetSession) -> Vec<SheetRow> {
    let sm = event_summary(session);
    let n = |v: i64| Cell::N(v as f64);
    let opt = |v: i64| if v == 0 { Cell::E } else { Cell::N(v as f64) };
    let mut rows = vec![
        SheetRow { values: vec![s("Metric"), s("Value")], header: true, fill: None, kab: false },
        SheetRow { values: vec![s("Team Event"), Cell::S(sm.event_name.clone())], header: false, fill: Some(ACCENT_GREEN), kab: false },
        SheetRow { values: vec![s("Own Team"), Cell::S(sm.team_name.clone())], header: false, fill: None, kab: false },
        SheetRow { values: vec![s("Enemy Team(s)"), Cell::S(sm.opponent_teams.join(", "))], header: false, fill: Some(ACCENT_BLUE), kab: false },
        SheetRow { values: vec![s("Submission ID"), Cell::S(session.id.clone())], header: false, fill: None, kab: false },
        SheetRow { values: vec![s("Images"), Cell::N(session.images.len() as f64)], header: false, fill: None, kab: false },
        SheetRow { values: vec![s("Own Drivers"), Cell::N(sm.own_players as f64)], header: false, fill: None, kab: false },
        SheetRow { values: vec![s("Enemy Drivers"), Cell::N(sm.opponent_count as f64)], header: false, fill: None, kab: false },
        SheetRow { values: vec![s("Own Event Points"), n(sm.own_points)], header: false, fill: None, kab: false },
        SheetRow { values: vec![s("Enemy Event Points"), n(sm.opponent_points)], header: false, fill: None, kab: false },
        SheetRow { values: vec![s("Own Score"), n(sm.own_score)], header: false, fill: None, kab: false },
        SheetRow { values: vec![s("Enemy Score"), n(sm.opponent_score)], header: false, fill: None, kab: false },
        SheetRow { values: vec![s("Own Team Score"), opt(sm.own_team_score)], header: false, fill: None, kab: false },
        SheetRow { values: vec![s("Enemy Team Score"), opt(sm.opponent_team_score)], header: false, fill: None, kab: false },
        SheetRow { values: vec![s("#KAB"), Cell::N(sm.kab_count as f64)], header: false, fill: if sm.kab_count > 0 { Some("#cfe2ff") } else { None }, kab: false },
    ];
    if let Some((rank, name)) = &sm.top_driver {
        rows.push(SheetRow { values: vec![s("Top Own Driver"), Cell::S(format!("#{rank} {name}"))], header: false, fill: Some("#e2f0d9"), kab: false });
    }
    rows
}

pub fn result_workbook_rows(session: &SpreadsheetSession) -> Vec<SheetRow> {
    let headers = ["Rank", "Player", "Team", "Type", "Event Points", "Score", "Blues Killed", "Blue Kill %", "#KAB"];
    let mut rows = vec![SheetRow { values: headers.iter().map(|h| s(h)).collect(), header: true, fill: None, kab: false }];
    for item in player_display_rows(session) {
        let kab = item.values[8].number() > 0.0;
        rows.push(SheetRow { fill: Some(if item.own { OWN_COLOR } else { OPPONENT_COLOR }), kab, values: item.values, header: false });
    }
    rows
}

pub fn attendance_workbook_rows(session: &SpreadsheetSession) -> Vec<SheetRow> {
    let mut rows = vec![SheetRow { values: ["Player", "Status", "Event Points", "Score", "Rank"].iter().map(|h| s(h)).collect(), header: true, fill: None, kab: false }];
    for p in own_players(session) {
        rows.push(SheetRow {
            values: vec![
                Cell::S(p.player_name.clone()),
                s("attended"),
                num_or_empty(points_value(p)),
                num_or_empty(score_value(p)),
                if p.rank == 0 { Cell::E } else { Cell::N(p.rank as f64) },
            ],
            header: false,
            fill: Some(OWN_COLOR),
            kab: false,
        });
    }
    for name in session.attendance.missing_players.iter().filter(|n| !n.is_empty()) {
        rows.push(SheetRow { values: vec![Cell::S(name.clone()), s("missed"), Cell::N(0.0), Cell::N(0.0), Cell::E], header: false, fill: Some(ACCENT_RED), kab: false });
    }
    rows
}

// --------------------------------------------------------------------------------------------- xlsx

pub fn color_of(hex: &str) -> Color {
    let h = hex.trim_start_matches('#');
    Color::RGB(u32::from_str_radix(h, 16).unwrap_or(0xFFFFFF))
}

fn base_format() -> Format {
    Format::new()
        .set_font_name("Aptos")
        .set_font_size(11)
        .set_font_color(Color::RGB(0x111827))
        .set_border(FormatBorder::Thin)
        .set_border_color(Color::RGB(0xD1D5DB))
        .set_align(FormatAlign::VerticalCenter)
        .set_text_wrap()
}

pub fn write_sheet(sheet: &mut Worksheet, rows: &[SheetRow]) -> Result<(), String> {
    let mut widths: Vec<usize> = Vec::new();
    for (r, row) in rows.iter().enumerate() {
        let last_col = row.values.len().saturating_sub(1);
        for (c, cell) in row.values.iter().enumerate() {
            let mut format = base_format().set_align(if c == 1 { FormatAlign::Left } else { FormatAlign::Center });
            if row.header {
                format = format.set_background_color(color_of(HEADER_COLOR)).set_font_color(Color::White).set_bold();
            } else if let Some(fill) = row.fill {
                format = format.set_background_color(color_of(fill));
            }
            if row.kab && c == last_col {
                format = format.set_background_color(color_of(KAB_COLOR)).set_font_color(Color::White).set_bold();
            }
            let (row_idx, col_idx) = (r as u32, c as u16);
            match cell {
                Cell::N(n) => sheet.write_number_with_format(row_idx, col_idx, *n, &format),
                Cell::S(t) => sheet.write_string_with_format(row_idx, col_idx, crate::unicode_names::sanitize_for_excel(t), &format),
                Cell::E => sheet.write_blank(row_idx, col_idx, &format),
            }
            .map_err(|e| e.to_string())?;
            let len = cell.text().chars().count();
            if widths.len() <= c {
                widths.resize(c + 1, if c == 1 { 22 } else { 12 });
            }
            let cap = if c == 1 { 34 } else { 22 };
            widths[c] = widths[c].max((len + 2).min(cap));
        }
    }
    for (c, w) in widths.iter().enumerate() {
        sheet.set_column_width(c as u16, *w as f64).map_err(|e| e.to_string())?;
    }
    sheet.set_freeze_panes(1, 0).map_err(|e| e.to_string())?;
    Ok(())
}

pub fn build_xlsx(session: &SpreadsheetSession) -> Result<Vec<u8>, String> {
    let mut workbook = Workbook::new();
    for (name, rows) in [
        ("Summary", summary_workbook_rows(session)),
        ("Results", result_workbook_rows(session)),
        ("Attendance", attendance_workbook_rows(session)),
    ] {
        let sheet = workbook.add_worksheet();
        sheet.set_name(name).map_err(|e| e.to_string())?;
        write_sheet(sheet, &rows)?;
    }
    workbook.save_to_buffer().map_err(|e| e.to_string())
}

// --------------------------------------------------------------------------------------------- fods

fn fods_cell(value: &Cell, style: &str) -> String {
    match value {
        Cell::E => format!("<table:table-cell table:style-name=\"{style}\" office:value-type=\"string\"><text:p></text:p></table:table-cell>"),
        Cell::N(n) => format!("<table:table-cell table:style-name=\"{style}\" office:value-type=\"float\" office:value=\"{}\"><text:p>{}</text:p></table:table-cell>", fmt_num(*n), fmt_num(*n)),
        Cell::S(t) => format!("<table:table-cell table:style-name=\"{style}\" office:value-type=\"string\"><text:p>{}</text:p></table:table-cell>", escape_xml(t)),
    }
}

fn fods_row(values: &[Cell], style: &str) -> String {
    format!("<table:table-row>{}</table:table-row>", values.iter().map(|v| fods_cell(v, style)).collect::<String>())
}

fn fods_table(name: &str, rows: String) -> String {
    format!(
        "<table:table table:name=\"{}\">\n<table:table-column table:number-columns-repeated=\"12\" table:style-name=\"Column\"/>\n{}\n</table:table>",
        escape_xml(&safe_sheet_name(name)),
        rows
    )
}

pub fn build_fods(session: &SpreadsheetSession) -> String {
    let sm = event_summary(session);
    let n = |v: i64| Cell::N(v as f64);
    let opt = |v: i64| if v == 0 { Cell::E } else { Cell::N(v as f64) };
    let mut summary = vec![
        fods_row(&[s("Metric"), s("Value")], "HeaderCell"),
        fods_row(&[s("Team Event"), Cell::S(sm.event_name.clone())], "Cell"),
        fods_row(&[s("Own Team"), Cell::S(sm.team_name.clone())], "Cell"),
        fods_row(&[s("Enemy Team(s)"), Cell::S(sm.opponent_teams.join(", "))], "Cell"),
        fods_row(&[s("Submission ID"), Cell::S(session.id.clone())], "Cell"),
        fods_row(&[s("Images"), Cell::N(session.images.len() as f64)], "Cell"),
        fods_row(&[s("Own Drivers"), Cell::N(sm.own_players as f64)], "Cell"),
        fods_row(&[s("Enemy Drivers"), Cell::N(sm.opponent_count as f64)], "Cell"),
        fods_row(&[s("Own Event Points"), n(sm.own_points)], "Cell"),
        fods_row(&[s("Enemy Event Points"), n(sm.opponent_points)], "Cell"),
        fods_row(&[s("Own Score"), n(sm.own_score)], "Cell"),
        fods_row(&[s("Enemy Score"), n(sm.opponent_score)], "Cell"),
        fods_row(&[s("Own Team Score"), opt(sm.own_team_score)], "Cell"),
        fods_row(&[s("Enemy Team Score"), opt(sm.opponent_team_score)], "Cell"),
        fods_row(&[s("#KAB"), Cell::N(sm.kab_count as f64)], "Cell"),
    ];
    if let Some((rank, name)) = &sm.top_driver {
        summary.push(fods_row(&[s("Top Own Driver"), Cell::S(format!("#{rank} {name}"))], "Cell"));
    }

    let mut results = vec![fods_row(&["Rank", "Player", "Team", "Type", "Event Points", "Score", "Blues Killed", "Blue Kill %", "#KAB"].iter().map(|h| s(h)).collect::<Vec<_>>(), "HeaderCell")];
    for item in player_display_rows(session) {
        results.push(fods_row(&item.values, if item.own { "OwnCell" } else { "OpponentCell" }));
    }
    let attendance: String = attendance_workbook_rows(session)
        .iter()
        .map(|r| fods_row(&r.values, if r.header { "HeaderCell" } else { "Cell" }))
        .collect::<Vec<_>>()
        .join("\n");

    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<office:document xmlns:office=\"urn:oasis:names:tc:opendocument:xmlns:office:1.0\" xmlns:style=\"urn:oasis:names:tc:opendocument:xmlns:style:1.0\" xmlns:text=\"urn:oasis:names:tc:opendocument:xmlns:text:1.0\" xmlns:table=\"urn:oasis:names:tc:opendocument:xmlns:table:1.0\" xmlns:fo=\"urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0\" office:version=\"1.2\">\n<office:automatic-styles>\n<style:style style:name=\"Column\" style:family=\"table-column\"><style:table-column-properties style:column-width=\"1.35in\"/></style:style>\n<style:style style:name=\"Cell\" style:family=\"table-cell\"><style:table-cell-properties fo:border=\"0.5pt solid #d1d5db\" fo:padding=\"0.03in\"/><style:text-properties fo:font-size=\"10pt\"/></style:style>\n<style:style style:name=\"HeaderCell\" style:family=\"table-cell\"><style:table-cell-properties fo:background-color=\"{HEADER_COLOR}\" fo:border=\"0.5pt solid #111827\" fo:padding=\"0.04in\"/><style:text-properties fo:color=\"#ffffff\" fo:font-weight=\"bold\" fo:font-size=\"10pt\"/></style:style>\n<style:style style:name=\"OwnCell\" style:family=\"table-cell\"><style:table-cell-properties fo:background-color=\"{OWN_COLOR}\" fo:border=\"0.5pt solid #d6b656\" fo:padding=\"0.03in\"/><style:text-properties fo:font-size=\"10pt\"/></style:style>\n<style:style style:name=\"OpponentCell\" style:family=\"table-cell\"><style:table-cell-properties fo:background-color=\"{OPPONENT_COLOR}\" fo:border=\"0.5pt solid #6ea8fe\" fo:padding=\"0.03in\"/><style:text-properties fo:font-size=\"10pt\"/></style:style>\n</office:automatic-styles>\n<office:body>\n<office:spreadsheet>\n{}\n{}\n{}\n</office:spreadsheet>\n</office:body>\n</office:document>",
        fods_table("Summary", summary.join("\n")),
        fods_table("Results", results.join("\n")),
        fods_table("Attendance", attendance),
    )
}

// ---------------------------------------------------------------------------------------------- svg

struct Pill<'a> {
    label: &'a str,
    value: String,
    width: u32,
    max_length: usize,
}

fn header_stat_pill(x: u32, y: u32, item: &Pill) -> String {
    let width = item.width;
    let label = preview_text(item.label, 12).to_uppercase();
    let value = preview_text(&item.value, item.max_length);
    let label_width = (label.chars().count() as u32 * 7 + 14).clamp(42, 76);
    format!(
        "<g>\n<rect x=\"{x}\" y=\"{y}\" width=\"{width}\" height=\"24\" fill=\"rgba(255,255,255,0.14)\" stroke=\"rgba(255,255,255,0.24)\" rx=\"12\"/>\n<text x=\"{}\" y=\"{}\" font-family=\"Arial\" font-size=\"10\" font-weight=\"700\" fill=\"#d1fae5\">{}</text>\n<text x=\"{}\" y=\"{}\" font-family=\"Arial\" font-size=\"13\" font-weight=\"700\" fill=\"#ffffff\">{}</text>\n</g>",
        x + 12,
        y + 16,
        escape_xml(&label),
        x + label_width,
        y + 17,
        escape_xml(&value)
    )
}

fn header_stat_pills(items: &[Pill], start_x: u32, y: u32, gap: u32) -> String {
    let mut x = start_x;
    let mut out = Vec::new();
    for item in items {
        out.push(header_stat_pill(x, y, item));
        x += item.width + gap;
    }
    out.join("\n")
}

pub fn build_chart_svg(session: &SpreadsheetSession) -> String {
    let sm = event_summary(session);
    let buckets = &session.stats.buckets;
    let matchup = format!("{} vs {}", sm.team_name, sm.opponent_teams.join(", "));
    let pills = header_stat_pills(
        &[
            Pill { label: "Drivers", value: sm.own_players.to_string(), width: 92, max_length: 18 },
            Pill { label: "KAB", value: sm.kab_count.to_string(), width: 70, max_length: 18 },
        ],
        970,
        25,
        8,
    );
    struct Group {
        title: &'static str,
        y: i64,
        max: i64,
        rows: [(&'static str, i64, &'static str); 2],
    }
    let groups = [
        Group { title: "Score", y: 126, max: 1.max(sm.own_score).max(sm.opponent_score), rows: [("Own score", sm.own_score, "#0f766e"), ("Enemy score", sm.opponent_score, "#2563eb")] },
        Group { title: "Event points", y: 252, max: 1.max(sm.own_points).max(sm.opponent_points), rows: [("Own event pts", sm.own_points, "#d97706"), ("Enemy event pts", sm.opponent_points, "#7c3aed")] },
    ];
    let (width, height, chart_x, chart_width, row_height) = (1180i64, 670i64, 230i64, 720i64, 42i64);
    let mut bar_rows = Vec::new();
    for group in &groups {
        let mut rows = Vec::new();
        for (index, (label, value, color)) in group.rows.iter().enumerate() {
            let y = group.y + index as i64 * row_height;
            let raw_width = ((*value as f64 / group.max as f64) * chart_width as f64).round() as i64;
            let bar_width = if *value > 0 { 6.max(raw_width) } else { 0 };
            let value_x = (chart_x + bar_width + 12).min(width - 116);
            rows.push(format!(
                "<text x=\"52\" y=\"{}\" font-family=\"Arial\" font-size=\"18\" fill=\"#111827\">{}</text><rect x=\"{chart_x}\" y=\"{y}\" width=\"{chart_width}\" height=\"30\" fill=\"#eef2ff\" rx=\"7\"/><rect x=\"{chart_x}\" y=\"{y}\" width=\"{bar_width}\" height=\"30\" fill=\"{color}\" rx=\"7\"/><text x=\"{value_x}\" y=\"{}\" font-family=\"Arial\" font-size=\"17\" font-weight=\"700\" fill=\"#111827\">{value}</text>",
                y + 23,
                escape_xml(label),
                y + 22
            ));
        }
        bar_rows.push(format!(
            "<text x=\"36\" y=\"{}\" font-family=\"Arial\" font-size=\"20\" font-weight=\"700\" fill=\"#111827\">{}</text>{}",
            group.y - 18,
            escape_xml(group.title),
            rows.join("\n")
        ));
    }
    let mut bucket_rows = Vec::new();
    for (index, bucket) in buckets.iter().enumerate() {
        let x = 150 + index as i64 * 260;
        let bucket_max = 1.max(bucket.own as i64).max(bucket.opponents as i64);
        let own_height = ((bucket.own as f64 / bucket_max as f64) * 132.0).round() as i64;
        let opp_height = ((bucket.opponents as f64 / bucket_max as f64) * 132.0).round() as i64;
        let baseline = 594;
        bucket_rows.push(format!(
            "<rect x=\"{}\" y=\"444\" width=\"140\" height=\"174\" fill=\"#f8fafc\" stroke=\"#e2e8f0\" rx=\"8\"/><text x=\"{x}\" y=\"636\" font-family=\"Arial\" font-size=\"17\" text-anchor=\"middle\" fill=\"#111827\">{}</text><rect x=\"{}\" y=\"{}\" width=\"40\" height=\"{own_height}\" fill=\"#f59e0b\" rx=\"5\"/><rect x=\"{}\" y=\"{}\" width=\"40\" height=\"{opp_height}\" fill=\"#2563eb\" rx=\"5\"/><text x=\"{}\" y=\"{}\" font-family=\"Arial\" font-size=\"14\" font-weight=\"700\" text-anchor=\"middle\" fill=\"#111827\">{}</text><text x=\"{}\" y=\"{}\" font-family=\"Arial\" font-size=\"14\" font-weight=\"700\" text-anchor=\"middle\" fill=\"#111827\">{}</text>",
            x - 70,
            escape_xml(&bucket.label),
            x - 45,
            baseline - own_height,
            x + 6,
            baseline - opp_height,
            x - 25,
            baseline - own_height - 8,
            bucket.own,
            x + 26,
            baseline - opp_height - 8,
            bucket.opponents
        ));
    }
    format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{width}\" height=\"{height}\" viewBox=\"0 0 {width} {height}\">\n<defs>\n<linearGradient id=\"chartTitle\" x1=\"0\" y1=\"0\" x2=\"1\" y2=\"0\"><stop offset=\"0%\" stop-color=\"#0f766e\"/><stop offset=\"50%\" stop-color=\"#2563eb\"/><stop offset=\"100%\" stop-color=\"#7c3aed\"/></linearGradient>\n</defs>\n<rect width=\"100%\" height=\"100%\" fill=\"#f8fafc\"/>\n<rect x=\"0\" y=\"0\" width=\"{width}\" height=\"76\" fill=\"url(#chartTitle)\"/>\n<text x=\"34\" y=\"31\" font-family=\"Arial\" font-size=\"24\" font-weight=\"700\" fill=\"#ffffff\">{}</text>\n<text x=\"34\" y=\"58\" font-family=\"Arial\" font-size=\"16\" font-weight=\"600\" fill=\"#d1fae5\">{}</text>\n{pills}\n{}\n<text x=\"36\" y=\"396\" font-family=\"Arial\" font-size=\"22\" font-weight=\"700\" fill=\"#111827\">Placement buckets</text>\n<line x1=\"70\" y1=\"594\" x2=\"1070\" y2=\"594\" stroke=\"#cbd5e1\"/>\n{}\n</svg>",
        escape_xml(&preview_text(&sm.event_name, 50)),
        escape_xml(&preview_text(&matchup, 72)),
        bar_rows.join("\n"),
        bucket_rows.join("\n")
    )
}

struct Column {
    label: &'static str,
    width: u32,
    center: bool,
    index: usize,
    max_length: usize,
    kind: ColKind,
}

#[derive(PartialEq)]
enum ColKind {
    Rank,
    Plain,
    Fill(&'static str),
    Kab,
}

pub fn build_spreadsheet_image_svg(session: &SpreadsheetSession) -> String {
    let sm = event_summary(session);
    let team_points = if sm.own_team_score != 0 || sm.opponent_team_score != 0 { format!("{}-{}", sm.own_team_score, sm.opponent_team_score) } else { "n/a".into() };
    let score_text = format!("{}-{}", sm.own_score, sm.opponent_score);
    let versus = format!("{} vs {}", sm.team_name, sm.opponent_teams.join(", "));
    let pills = header_stat_pills(
        &[
            Pill { label: "Drivers", value: sm.own_players.to_string(), width: 94, max_length: 18 },
            Pill { label: "KAB", value: sm.kab_count.to_string(), width: 72, max_length: 18 },
            Pill { label: "Team pts", value: team_points, width: 154, max_length: 18 },
            Pill { label: "Score", value: score_text, width: 188, max_length: 18 },
        ],
        28,
        84,
        8,
    );
    let columns = [
        Column { label: "Rank", width: 70, center: true, index: 0, max_length: 16, kind: ColKind::Rank },
        Column { label: "Player", width: 280, center: false, index: 1, max_length: 34, kind: ColKind::Plain },
        Column { label: "Pts", width: 95, center: true, index: 4, max_length: 16, kind: ColKind::Fill("#dcfce7") },
        Column { label: "Score", width: 135, center: true, index: 5, max_length: 16, kind: ColKind::Fill("#ede9fe") },
        Column { label: "Blues", width: 95, center: true, index: 6, max_length: 16, kind: ColKind::Fill("#dbeafe") },
        Column { label: "Blue %", width: 95, center: true, index: 7, max_length: 16, kind: ColKind::Fill("#ccfbf1") },
        Column { label: "#KAB", width: 85, center: true, index: 8, max_length: 16, kind: ColKind::Kab },
    ];
    let rows: Vec<DisplayRow> = player_display_rows(session).into_iter().filter(|r| r.own).collect();
    let (row_height, margin, title_height) = (34u32, 28u32, 156u32);
    let table_width: u32 = columns.iter().map(|c| c.width).sum();
    let width = table_width + margin * 2;
    let height = title_height + row_height * (1.max(rows.len() as u32) + 1) + 62;

    let mut x = margin;
    let mut header_cells = Vec::new();
    for (index, column) in columns.iter().enumerate() {
        let fill = IMAGE_HEADER_COLORS[index % IMAGE_HEADER_COLORS.len()];
        let (anchor, text_x) = if column.center { ("middle", x + column.width / 2) } else { ("start", x + 10) };
        header_cells.push(format!(
            "<rect x=\"{x}\" y=\"{title_height}\" width=\"{}\" height=\"{row_height}\" fill=\"{fill}\" stroke=\"#111827\"/>\n<text x=\"{text_x}\" y=\"{}\" font-family=\"Arial\" font-size=\"14\" font-weight=\"700\" text-anchor=\"{anchor}\" fill=\"#ffffff\">{}</text>",
            column.width,
            title_height + 23,
            escape_xml(column.label)
        ));
        x += column.width;
    }

    let body = if rows.is_empty() {
        format!(
            "<rect x=\"{margin}\" y=\"{}\" width=\"{table_width}\" height=\"{row_height}\" fill=\"#f8fafc\" stroke=\"#d1d5db\"/>\n<text x=\"{}\" y=\"{}\" font-family=\"Arial\" font-size=\"13\" fill=\"#64748b\">No own drivers found</text>",
            title_height + row_height,
            margin + 12,
            title_height + row_height + 22
        )
    } else {
        let mut parts = Vec::new();
        for (row_index, item) in rows.iter().enumerate() {
            let y = title_height + row_height * (row_index as u32 + 1);
            let kab_value = item.values[8].number();
            let base_fill = if kab_value > 0.0 { "#fef3c7" } else if row_index % 2 == 0 { "#fffbeb" } else { "#f8fafc" };
            let mut cell_x = margin;
            for column in &columns {
                let text = preview_text(&item.values[column.index].text(), if column.max_length > 0 { column.max_length } else if column.width > 180 { 30 } else { 16 });
                let fill = match &column.kind {
                    ColKind::Rank => {
                        let rank = item.values[0].number() as u32;
                        if rank == 1 { "#fde68a" } else if rank <= 3 && rank > 0 { "#e0f2fe" } else if rank <= 10 && rank > 0 { "#dcfce7" } else { base_fill }
                    }
                    ColKind::Plain => base_fill,
                    ColKind::Fill(c) => c,
                    ColKind::Kab => if kab_value > 0.0 { KAB_COLOR } else { "#fce7f3" },
                };
                let text_color = if column.kind == ColKind::Kab && kab_value > 0.0 { "#ffffff" } else { "#111827" };
                let (anchor, text_x) = if column.center { ("middle", cell_x + column.width / 2) } else { ("start", cell_x + 10) };
                parts.push(format!(
                    "<rect x=\"{cell_x}\" y=\"{y}\" width=\"{}\" height=\"{row_height}\" fill=\"{fill}\" stroke=\"#d1d5db\"/>\n<text x=\"{text_x}\" y=\"{}\" font-family=\"Arial\" font-size=\"13\" font-weight=\"{}\" text-anchor=\"{anchor}\" fill=\"{text_color}\">{}</text>",
                    column.width,
                    y + 22,
                    if column.center { "700" } else { "400" },
                    escape_xml(&text)
                ));
                cell_x += column.width;
            }
        }
        parts.join("\n")
    };

    format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{width}\" height=\"{height}\" viewBox=\"0 0 {width} {height}\">\n<defs>\n<linearGradient id=\"tableTitle\" x1=\"0\" y1=\"0\" x2=\"1\" y2=\"0\"><stop offset=\"0%\" stop-color=\"#0f766e\"/><stop offset=\"55%\" stop-color=\"#2563eb\"/><stop offset=\"100%\" stop-color=\"#d97706\"/></linearGradient>\n</defs>\n<rect width=\"100%\" height=\"100%\" fill=\"#f8fafc\"/>\n<rect x=\"0\" y=\"0\" width=\"{width}\" height=\"118\" fill=\"url(#tableTitle)\"/>\n<text x=\"{margin}\" y=\"35\" font-family=\"Arial\" font-size=\"25\" font-weight=\"700\" fill=\"#ffffff\">{}</text>\n<text x=\"{margin}\" y=\"66\" font-family=\"Arial\" font-size=\"16\" font-weight=\"600\" fill=\"#d1fae5\">{}</text>\n{pills}\n{}\n{body}\n</svg>",
        escape_xml(&preview_text(&sm.event_name, 54)),
        escape_xml(&preview_text(&versus, 62)),
        header_cells.join("\n")
    )
}
