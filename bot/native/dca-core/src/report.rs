//! Weekly / monthly team reports. Port of `spreadsheetReports.js`.

use crate::calc::{color_of, escape_xml, fmt_num, preview_text, safe_file_name, Cell};
use crate::metrics::*;
use crate::session::same_event_name;
use chrono::{DateTime, Datelike, Duration, NaiveDate, TimeZone, Utc};
use dca_state::models::SpreadsheetSession;
use rust_xlsxwriter::{Color, Format, FormatAlign, FormatBorder, Workbook, Worksheet};
use std::collections::HashMap;

const HEADER_COLOR: &str = "#1f2937";
const TITLE_FILL: &str = "#0f766e";
const OWN_FILL: &str = "#fff2cc";
const ALT_FILL: &str = "#eaf2ff";
const KAB_FILL: &str = "#0f7dba";
const MISSED_FILL: &str = "#fee2e2";
const GOOD_FILL: &str = "#d9ead3";
const WARN_FILL: &str = "#fff2cc";

#[derive(Clone, Debug)]
pub struct Bounds {
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
    pub key: String,
    pub label: String,
}

fn start_of_utc_day(d: DateTime<Utc>) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(d.year(), d.month(), d.day(), 0, 0, 0).unwrap()
}

pub fn iso_week_number(date: DateTime<Utc>) -> String {
    format!("{:02}", date.iso_week().week())
}

pub fn period_bounds(period: &str, anchor: DateTime<Utc>) -> Bounds {
    let day = start_of_utc_day(anchor);
    if period == "weekly" {
        let offset = day.weekday().num_days_from_monday() as i64;
        let start = day - Duration::days(offset);
        let end = start + Duration::days(7);
        return Bounds { start, end, key: format!("{}-W{}", start.year(), iso_week_number(start)), label: format!("Week of {}", start.format("%Y-%m-%d")) };
    }
    let start = Utc.with_ymd_and_hms(day.year(), day.month(), 1, 0, 0, 0).unwrap();
    let (ny, nm) = if day.month() == 12 { (day.year() + 1, 1) } else { (day.year(), day.month() + 1) };
    let end = Utc.with_ymd_and_hms(ny, nm, 1, 0, 0, 0).unwrap();
    Bounds { start, end, key: format!("{}-{:02}", start.year(), start.month()), label: start.format("%B %Y").to_string() }
}

fn same_period(session: &SpreadsheetSession, bounds: &Bounds) -> bool {
    let d = session_date(session);
    d >= bounds.start && d < bounds.end
}

fn find_anchor_session<'a>(sessions: &'a [SpreadsheetSession], bounds: &Bounds, anchor: DateTime<Utc>) -> Option<&'a SpreadsheetSession> {
    let mut processed: Vec<&SpreadsheetSession> = sessions.iter().filter(|s| s.status == "processed" && same_period(s, bounds)).collect();
    processed.sort_by_key(|s| session_date(s));
    processed.iter().rev().find(|s| session_date(s) <= anchor).copied().or_else(|| processed.last().copied())
}

pub fn filter_sessions_for_report<'a>(
    all: &'a [SpreadsheetSession],
    period: &str,
    bounds: &Bounds,
    anchor: DateTime<Utc>,
    event_name: Option<&str>,
    anchor_session: Option<&SpreadsheetSession>,
) -> Vec<&'a SpreadsheetSession> {
    let mut processed: Vec<&SpreadsheetSession> = all.iter().filter(|s| s.status == "processed" && same_period(s, bounds)).collect();
    processed.sort_by_key(|s| session_date(s));
    if period != "weekly" {
        return processed;
    }
    let anchor_name = match (event_name, anchor_session) {
        (Some(n), _) => n.to_string(),
        (None, Some(s)) => event_name_for_session(s, "Team Event"),
        (None, None) => find_anchor_session(all, bounds, anchor).map(|s| event_name_for_session(s, "Team Event")).unwrap_or_default(),
    };
    if normalize_key(&anchor_name).is_empty() {
        return processed;
    }
    processed.into_iter().filter(|s| same_event_name(&event_name_for_session(s, "Team Event"), &anchor_name)).collect()
}

#[derive(Clone, Debug)]
pub struct ReportEvent {
    pub id: String,
    pub label: String,
    pub event_name: String,
    pub date: String,
    pub opponent_teams: Vec<String>,
    pub opponent_count: usize,
    pub top_opponent_rank: Option<u32>,
    pub max_score: i64,
    pub max_points: i64,
    pub own_team_score: i64,
    pub opponent_team_score: i64,
}

#[derive(Clone, Debug)]
pub struct EventCell {
    pub event_id: String,
    pub opponent_teams: Vec<String>,
    pub rank: Option<u32>,
    pub score: i64,
    pub points: i64,
    pub blues_killed: usize,
    pub possible_blues: usize,
    pub blue_kill_percent: f64,
    pub kab: u32,
}

#[derive(Clone, Debug)]
pub struct ReportRow {
    pub key: String,
    pub name: String,
    pub events: Vec<EventCell>,
    pub attended: u32,
    pub missed: u32,
    pub kab: u32,
    pub total_score: i64,
    pub total_points: i64,
    pub blues_killed: usize,
    pub possible_blues: usize,
    pub blue_kill_percent: f64,
    pub best_rank: Option<u32>,
    pub rank: u32,
}

#[derive(Clone, Debug)]
pub struct Totals {
    pub events: usize,
    pub players: usize,
    pub total_score: i64,
    pub total_points: i64,
    pub total_kab: u32,
    pub total_missed: u32,
    pub blues_killed: usize,
    pub possible_blues: usize,
}

#[derive(Clone, Debug)]
pub struct ReportModel {
    pub team_id: String,
    pub team_name: String,
    pub period: String,
    pub period_label: String,
    pub period_key: String,
    pub event_name: String,
    pub enemy_teams: Vec<String>,
    pub events: Vec<ReportEvent>,
    pub rows: Vec<ReportRow>,
    pub totals: Totals,
}

fn event_label(session: &SpreadsheetSession, used: &mut std::collections::HashSet<String>) -> String {
    let opponent = session_opponent_teams(session).join(", ");
    let date = session_date(session).format("%Y-%m-%d");
    let mut label = format!("{date} vs {opponent}");
    if label.chars().count() > 48 {
        label = format!("{}...", label.chars().take(45).collect::<String>().trim_end());
    }
    let base = label.clone();
    let mut suffix = 2;
    while used.contains(&label) {
        label = format!("{base} {suffix}");
        suffix += 1;
    }
    used.insert(label.clone());
    label
}

fn blank_row(key: String, name: String, events: &[ReportEvent]) -> ReportRow {
    ReportRow {
        key,
        name,
        events: events
            .iter()
            .map(|e| EventCell {
                event_id: e.id.clone(),
                opponent_teams: e.opponent_teams.clone(),
                rank: None,
                score: 0,
                points: 0,
                blues_killed: 0,
                possible_blues: e.opponent_count,
                blue_kill_percent: 0.0,
                kab: 0,
            })
            .collect(),
        attended: 0,
        missed: events.len() as u32,
        kab: 0,
        total_score: 0,
        total_points: 0,
        blues_killed: 0,
        possible_blues: events.iter().map(|e| e.opponent_count).sum(),
        blue_kill_percent: 0.0,
        best_rank: None,
        rank: 0,
    }
}

pub struct TeamInfo<'a> {
    pub id: &'a str,
    pub name: &'a str,
    pub own_player_aliases: &'a [String],
}

pub fn build_report_model(team: &TeamInfo, sessions: &[&SpreadsheetSession], period: &str, bounds: &Bounds, all_sessions: &[SpreadsheetSession]) -> ReportModel {
    let mut used = std::collections::HashSet::new();
    let event_name = if period == "weekly" { sessions.first().map(|s| event_name_for_session(s, "Team Event")).unwrap_or_else(|| "Team Event".into()) } else { "Multiple team events".to_string() };
    let events: Vec<ReportEvent> = sessions
        .iter()
        .enumerate()
        .map(|(index, s)| ReportEvent {
            id: s.id.clone(),
            label: event_label(s, &mut used),
            event_name: event_name_for_session(s, &format!("Event {}", index + 1)),
            date: session_date(s).format("%Y-%m-%d").to_string(),
            opponent_teams: session_opponent_teams(s),
            opponent_count: opponent_count(s),
            top_opponent_rank: top_opponent_rank(s),
            max_score: event_max_score(s),
            max_points: event_max_points(s),
            own_team_score: team_score_from_metadata(s, true),
            opponent_team_score: team_score_from_metadata(s, false),
        })
        .collect();

    let mut enemy_teams: Vec<String> = Vec::new();
    for e in &events {
        for name in &e.opponent_teams {
            let clean = clean_team_name(name, "");
            let key = normalize_key(&clean);
            if key.is_empty() || enemy_teams.iter().any(|n| normalize_key(n) == key) {
                continue;
            }
            enemy_teams.push(clean);
        }
    }
    if enemy_teams.is_empty() {
        enemy_teams.push("Opponent".into());
    }

    let mut order: Vec<String> = Vec::new();
    let mut by_key: HashMap<String, ReportRow> = HashMap::new();
    for alias in team.own_player_aliases {
        let key = player_key(alias);
        if !key.is_empty() && !by_key.contains_key(&key) {
            by_key.insert(key.clone(), blank_row(key.clone(), alias.clone(), &events));
            order.push(key);
        }
    }
    for s in all_sessions.iter().filter(|s| s.status == "processed") {
        for p in own_players(s) {
            let key = player_key(&p.player_name);
            if !key.is_empty() && !by_key.contains_key(&key) {
                by_key.insert(key.clone(), blank_row(key.clone(), p.player_name.clone(), &events));
                order.push(key);
            }
        }
    }

    for (event_index, event) in events.iter().enumerate() {
        let session = sessions[event_index];
        let kab_map = session_kab_map(session);
        for p in own_players(session) {
            let key = player_key(&p.player_name);
            if key.is_empty() {
                continue;
            }
            if !by_key.contains_key(&key) {
                by_key.insert(key.clone(), blank_row(key.clone(), p.player_name.clone(), &events));
                order.push(key.clone());
            }
            let row = by_key.get_mut(&key).unwrap();
            let rank = if p.rank > 0 { Some(p.rank) } else { None };
            let killed = rank.map(|r| blues_killed_for_rank(session, r)).unwrap_or(0);
            row.events[event_index] = EventCell {
                event_id: event.id.clone(),
                opponent_teams: event.opponent_teams.clone(),
                rank,
                score: score_value(p),
                points: points_value(p),
                blues_killed: killed,
                possible_blues: event.opponent_count,
                blue_kill_percent: blue_kill_percent(killed, event.opponent_count),
                kab: *kab_map.get(&p.player_name).unwrap_or(&0),
            };
        }
    }

    let mut rows: Vec<ReportRow> = order.into_iter().filter_map(|k| by_key.remove(&k)).collect();
    for row in rows.iter_mut() {
        row.attended = row.events.iter().filter(|e| e.rank.is_some()).count() as u32;
        row.missed = (events.len() as u32).saturating_sub(row.attended);
        row.kab = row.events.iter().map(|e| e.kab).sum();
        row.total_score = row.events.iter().map(|e| e.score).sum();
        row.total_points = row.events.iter().map(|e| e.points).sum();
        row.blues_killed = row.events.iter().map(|e| e.blues_killed).sum();
        row.possible_blues = row.events.iter().map(|e| e.possible_blues).sum();
        row.blue_kill_percent = blue_kill_percent(row.blues_killed, row.possible_blues);
        row.best_rank = row.events.iter().filter_map(|e| e.rank).min();
    }

    let weekly = period == "weekly";
    rows.sort_by(|a, b| {
        if weekly {
            b.blues_killed.cmp(&a.blues_killed).then(b.kab.cmp(&a.kab)).then(b.total_score.cmp(&a.total_score)).then(b.total_points.cmp(&a.total_points)).then_with(|| a.name.cmp(&b.name))
        } else {
            b.total_score.cmp(&a.total_score).then(b.total_points.cmp(&a.total_points)).then(b.blues_killed.cmp(&a.blues_killed)).then(b.kab.cmp(&a.kab)).then_with(|| a.name.cmp(&b.name))
        }
    });
    let (mut rank, mut prev_primary, mut prev_secondary) = (0u32, None::<i64>, None::<i64>);
    for (index, row) in rows.iter_mut().enumerate() {
        let primary = if weekly { row.blues_killed as i64 } else { row.total_score };
        let secondary = if weekly { None } else { Some(row.total_points) };
        if Some(primary) != prev_primary || secondary != prev_secondary {
            rank = index as u32 + 1;
        }
        row.rank = rank;
        prev_primary = Some(primary);
        prev_secondary = secondary;
    }

    let period_key = if weekly { format!("{}-{}", bounds.key, safe_file_name(&event_name, "team-event").to_lowercase()) } else { bounds.key.clone() };
    let totals = Totals {
        events: events.len(),
        players: rows.len(),
        total_score: rows.iter().map(|r| r.total_score).sum(),
        total_points: rows.iter().map(|r| r.total_points).sum(),
        total_kab: rows.iter().map(|r| r.kab).sum(),
        total_missed: rows.iter().map(|r| r.missed).sum(),
        blues_killed: rows.iter().map(|r| r.blues_killed).sum(),
        possible_blues: rows.iter().map(|r| r.possible_blues).sum(),
    };
    ReportModel {
        team_id: team.id.to_string(),
        team_name: if team.name.is_empty() { team.id.to_string() } else { team.name.to_string() },
        period: period.to_string(),
        period_label: bounds.label.clone(),
        period_key,
        event_name,
        enemy_teams,
        events,
        rows,
        totals,
    }
}

// -------------------------------------------------------------------------------------------- xlsx

struct RowStyle {
    fill: Option<&'static str>,
    bold: bool,
    color: Option<&'static str>,
    size: f64,
}

fn style_cell(row: &RowStyle, horizontal: FormatAlign) -> Format {
    let mut f = Format::new()
        .set_font_name("Aptos")
        .set_font_size(row.size)
        .set_border(FormatBorder::Thin)
        .set_border_color(Color::RGB(0xCBD5E1))
        .set_align(FormatAlign::VerticalCenter)
        .set_align(horizontal)
        .set_text_wrap()
        .set_font_color(row.color.map(color_of).unwrap_or(Color::RGB(0x111827)));
    if row.bold {
        f = f.set_bold();
    }
    if let Some(fill) = row.fill {
        f = f.set_background_color(color_of(fill));
    }
    f
}

fn write_cells(sheet: &mut Worksheet, r: u32, values: &[Cell], style: &RowStyle, overrides: &HashMap<usize, Format>) -> Result<(), String> {
    for (c, v) in values.iter().enumerate() {
        let format = overrides.get(&c).cloned().unwrap_or_else(|| style_cell(style, FormatAlign::Center));
        match v {
            Cell::N(n) => sheet.write_number_with_format(r, c as u16, *n, &format),
            Cell::S(t) => sheet.write_string_with_format(r, c as u16, crate::unicode_names::sanitize_for_excel(t), &format),
            Cell::E => sheet.write_blank(r, c as u16, &format),
        }
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn n(v: i64) -> Cell {
    Cell::N(v as f64)
}
fn t(v: &str) -> Cell {
    Cell::S(v.to_string())
}

pub fn build_report_xlsx(model: &ReportModel) -> Result<Vec<u8>, String> {
    let mut wb = Workbook::new();
    let header = RowStyle { fill: Some(HEADER_COLOR), bold: true, color: Some("#ffffff"), size: 11.0 };
    let plain = RowStyle { fill: None, bold: false, color: None, size: 11.0 };

    // ---- Report sheet
    {
        let sheet = wb.add_worksheet();
        sheet.set_name("Report").map_err(|e| e.to_string())?;
        let headers = ["Driver Rank", "Driver", "Team Event", "Enemy Team(s)", "Best Rank", "Score", "Event Points", "Blues Killed", "Blue Kill %", "#KAB", "Missed", "Attended"];
        let title = format!("{} {} Report", model.team_name, if model.period == "weekly" { "Weekly" } else { "Monthly" });
        let title_style = RowStyle { fill: Some(TITLE_FILL), bold: true, color: Some("#ffffff"), size: 18.0 };
        sheet.merge_range(0, 0, 0, (headers.len() - 1) as u16, &title, &style_cell(&title_style, FormatAlign::Center)).map_err(|e| e.to_string())?;
        let info_style = RowStyle { fill: Some(GOOD_FILL), bold: true, color: None, size: 11.0 };
        write_cells(
            sheet,
            1,
            &[
                t("Team Event"),
                Cell::S(model.event_name.clone()),
                t("Enemy Team(s)"),
                Cell::S(model.enemy_teams.join(", ")),
                t("Period"),
                Cell::S(model.period_label.clone()),
                t("Sessions"),
                Cell::N(model.events.len() as f64),
                t("Drivers"),
                Cell::N(model.rows.len() as f64),
                Cell::E,
                Cell::E,
            ],
            &info_style,
            &HashMap::new(),
        )?;
        write_cells(sheet, 2, &headers.iter().map(|h| t(h)).collect::<Vec<_>>(), &header, &HashMap::new())?;
        for (index, entry) in model.rows.iter().enumerate() {
            let fill = if index % 2 == 0 { ALT_FILL } else { "#ffffff" };
            let style = RowStyle { fill: Some(fill), bold: false, color: None, size: 11.0 };
            let mut over: HashMap<usize, Format> = HashMap::new();
            over.insert(1, style_cell(&style, FormatAlign::Left));
            if entry.rank <= 3 {
                over.insert(0, style_cell(&RowStyle { fill: Some(WARN_FILL), bold: true, color: None, size: 11.0 }, FormatAlign::Center));
            }
            if entry.blue_kill_percent >= 75.0 {
                over.insert(8, style_cell(&RowStyle { fill: Some(GOOD_FILL), bold: false, color: None, size: 11.0 }, FormatAlign::Center));
            }
            if entry.kab > 0 {
                over.insert(9, style_cell(&RowStyle { fill: Some(KAB_FILL), bold: true, color: Some("#ffffff"), size: 11.0 }, FormatAlign::Center));
            }
            if entry.missed > 0 {
                over.insert(10, style_cell(&RowStyle { fill: Some(MISSED_FILL), bold: false, color: None, size: 11.0 }, FormatAlign::Center));
            }
            write_cells(
                sheet,
                3 + index as u32,
                &[
                    n(entry.rank as i64),
                    Cell::S(entry.name.clone()),
                    Cell::S(model.event_name.clone()),
                    Cell::S(model.enemy_teams.join(", ")),
                    entry.best_rank.map(|r| n(r as i64)).unwrap_or(Cell::E),
                    n(entry.total_score),
                    n(entry.total_points),
                    n(entry.blues_killed as i64),
                    Cell::S(format!("{}%", fmt_num(entry.blue_kill_percent))),
                    n(entry.kab as i64),
                    n(entry.missed as i64),
                    n(entry.attended as i64),
                ],
                &style,
                &over,
            )?;
        }
        for (i, w) in [12.0, 24.0, 28.0, 28.0, 11.0, 14.0, 14.0, 14.0, 13.0, 10.0, 10.0, 11.0].iter().enumerate() {
            sheet.set_column_width(i as u16, *w).map_err(|e| e.to_string())?;
        }
        sheet.set_freeze_panes(3, 0).map_err(|e| e.to_string())?;
    }

    // ---- Details sheet
    {
        let sheet = wb.add_worksheet();
        sheet.set_name("Details").map_err(|e| e.to_string())?;
        let mut r = 0u32;
        write_cells(sheet, r, &[t("Metric"), t("Value")], &header, &HashMap::new())?;
        r += 1;
        let totals = &model.totals;
        let items: Vec<(&str, Cell)> = vec![
            ("Team", Cell::S(model.team_name.clone())),
            ("Period", Cell::S(model.period_label.clone())),
            ("Team Event", Cell::S(model.event_name.clone())),
            ("Enemy Team(s)", Cell::S(model.enemy_teams.join(", "))),
            ("Sessions included", Cell::N(totals.events as f64)),
            ("Drivers included", Cell::N(totals.players as f64)),
            ("Total score", n(totals.total_score)),
            ("Total event points", n(totals.total_points)),
            ("Blues killed", Cell::S(format!("{}/{}", totals.blues_killed, totals.possible_blues))),
            ("Blue kill %", Cell::S(format!("{}%", fmt_num(blue_kill_percent(totals.blues_killed, totals.possible_blues))))),
        ];
        for (k, v) in items {
            write_cells(sheet, r, &[t(k), v], &plain, &HashMap::new())?;
            r += 1;
        }
        r += 1;
        write_cells(
            sheet,
            r,
            &["Date", "Team Event", "Enemy Team(s)", "Own Team Score", "Enemy Team Score", "Enemy Drivers", "Max Score", "Max Event Points", "Session ID"].iter().map(|h| t(h)).collect::<Vec<_>>(),
            &header,
            &HashMap::new(),
        )?;
        r += 1;
        for e in &model.events {
            write_cells(
                sheet,
                r,
                &[
                    Cell::S(e.date.clone()),
                    Cell::S(e.event_name.clone()),
                    Cell::S(e.opponent_teams.join(", ")),
                    if e.own_team_score != 0 { n(e.own_team_score) } else { Cell::E },
                    if e.opponent_team_score != 0 { n(e.opponent_team_score) } else { Cell::E },
                    n(e.opponent_count as i64),
                    n(e.max_score),
                    n(e.max_points),
                    Cell::S(e.id.clone()),
                ],
                &plain,
                &HashMap::new(),
            )?;
            r += 1;
        }
        r += 1;
        write_cells(
            sheet,
            r,
            &["Driver", "Date", "Enemy Team(s)", "Rank", "Score", "Event Points", "Blues Killed", "Blue Kill %", "#KAB"].iter().map(|h| t(h)).collect::<Vec<_>>(),
            &header,
            &HashMap::new(),
        )?;
        r += 1;
        for row in &model.rows {
            for cell in &row.events {
                let fill = if cell.rank.is_none() {
                    Some(MISSED_FILL)
                } else if cell.kab > 0 {
                    Some(OWN_FILL)
                } else {
                    None
                };
                let style = RowStyle { fill, bold: false, color: None, size: 11.0 };
                write_cells(
                    sheet,
                    r,
                    &[
                        Cell::S(row.name.clone()),
                        Cell::S(model.events.iter().find(|e| e.id == cell.event_id).map(|e| e.date.clone()).unwrap_or_default()),
                        Cell::S(cell.opponent_teams.join(", ")),
                        cell.rank.map(|v| n(v as i64)).unwrap_or(Cell::E),
                        n(cell.score),
                        n(cell.points),
                        n(cell.blues_killed as i64),
                        Cell::S(format!("{}%", fmt_num(cell.blue_kill_percent))),
                        n(cell.kab as i64),
                    ],
                    &style,
                    &HashMap::new(),
                )?;
                r += 1;
            }
        }
        for (i, w) in [22.0, 14.0, 28.0, 10.0, 14.0, 14.0, 14.0, 12.0, 10.0].iter().enumerate() {
            sheet.set_column_width(i as u16, *w).map_err(|e| e.to_string())?;
        }
        sheet.set_freeze_panes(1, 0).map_err(|e| e.to_string())?;
    }
    wb.save_to_buffer().map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------------------------------- svg

fn report_pill(x: u32, y: u32, label: &str, value: &str, width: u32) -> String {
    format!(
        "<g>\n<rect x=\"{x}\" y=\"{y}\" width=\"{width}\" height=\"24\" fill=\"rgba(255,255,255,0.14)\" stroke=\"rgba(255,255,255,0.24)\" rx=\"12\"/>\n<text x=\"{}\" y=\"{}\" font-family=\"Arial\" font-size=\"10\" font-weight=\"700\" fill=\"#d1fae5\">{}</text>\n<text x=\"{}\" y=\"{}\" font-family=\"Arial\" font-size=\"13\" font-weight=\"700\" text-anchor=\"end\" fill=\"#ffffff\">{}</text>\n</g>",
        x + 12,
        y + 16,
        escape_xml(&label.to_uppercase()),
        x + width - 12,
        y + 17,
        escape_xml(value)
    )
}

fn event_team_name(event: &ReportEvent) -> String {
    event.opponent_teams.iter().map(|n| clean_team_name(n, "")).find(|n| !n.is_empty()).unwrap_or_else(|| "Opponent".into())
}

struct WeeklyRow<'a> {
    row: &'a ReportRow,
    cells: Vec<(bool, Option<usize>, usize)>, // attended, value, possible
    total: usize,
    max: usize,
    percent: f64,
    rank: u32,
}

fn weekly_rows(model: &ReportModel) -> Vec<WeeklyRow<'_>> {
    let mut rows: Vec<WeeklyRow> = model
        .rows
        .iter()
        .map(|row| {
            let cells: Vec<(bool, Option<usize>, usize)> = row
                .events
                .iter()
                .map(|e| {
                    let attended = e.rank.is_some();
                    (attended, if attended { Some(e.blues_killed) } else { None }, if attended { e.possible_blues } else { 0 })
                })
                .collect();
            let total: usize = cells.iter().map(|c| c.1.unwrap_or(0)).sum();
            let max: usize = cells.iter().map(|c| c.2).sum();
            WeeklyRow { row, cells, total, max, percent: blue_kill_percent(total, max), rank: 0 }
        })
        .collect();
    rows.sort_by(|a, b| b.total.cmp(&a.total).then(b.row.kab.cmp(&a.row.kab)).then(b.row.total_score.cmp(&a.row.total_score)).then_with(|| a.row.name.cmp(&b.row.name)));
    let (mut rank, mut prev) = (0u32, None::<usize>);
    for (i, r) in rows.iter_mut().enumerate() {
        if Some(r.total) != prev {
            rank = i as u32 + 1;
        }
        r.rank = rank;
        prev = Some(r.total);
    }
    rows
}

fn weekly_event_cell_fill(attended: bool, value: usize, possible: usize, base: &'static str) -> &'static str {
    if !attended {
        return "#f8fafc";
    }
    if possible > 0 && value >= possible {
        return "#fef08a";
    }
    let ratio = if possible > 0 { value as f64 / possible as f64 } else { 0.0 };
    if ratio >= 0.9 {
        "#bbf7d0"
    } else if ratio >= 0.75 {
        "#dbeafe"
    } else if ratio >= 0.5 {
        "#ede9fe"
    } else if value > 0 {
        "#ffedd5"
    } else {
        base
    }
}

pub fn build_weekly_table_svg(model: &ReportModel) -> String {
    let rows = weekly_rows(model);
    let events = &model.events;
    let event_width: u32 = if events.len() > 5 { 68 } else { 78 };
    let (margin, title_height, group_height, header_height, row_height) = (24u32, 128u32, 30u32, 32u32, 30u32);
    let stat_cols: [(&str, u32); 4] = [("%Kill", 84), ("Total", 78), ("Max", 76), ("#KAB", 76)];
    let table_width = 72 + 250 + events.len() as u32 * event_width + stat_cols.iter().map(|c| c.1).sum::<u32>();
    let width = table_width + margin * 2;
    let body_start_y = title_height + group_height + header_height;
    let height = body_start_y + rows.len() as u32 * row_height + 34;
    let event_start_x = margin + 72 + 250;
    let stats_start_x = event_start_x + events.len() as u32 * event_width;
    let stats_width: u32 = stat_cols.iter().map(|c| c.1).sum();
    let pill_x = (margin + 420).max(width.saturating_sub(306));
    let pills = [report_pill(pill_x, 26, "Sessions", &events.len().to_string(), 128), report_pill(pill_x + 138, 26, "Drivers", &rows.len().to_string(), 128)].join("\n");

    let mut static_headers = Vec::new();
    let mut x = margin;
    for (label, w) in [("Rank", 72u32), ("Driver", 250)] {
        static_headers.push(format!(
            "<rect x=\"{x}\" y=\"{title_height}\" width=\"{w}\" height=\"{}\" fill=\"#1f2937\" stroke=\"#111827\"/>\n<text x=\"{}\" y=\"{}\" font-family=\"Arial\" font-size=\"17\" font-weight=\"700\" text-anchor=\"middle\" fill=\"#ffffff\">{}</text>",
            group_height + header_height,
            x + w / 2,
            title_height + 39,
            escape_xml(label)
        ));
        x += w;
    }
    let event_headers: Vec<String> = events
        .iter()
        .enumerate()
        .map(|(index, event)| {
            let cx = event_start_x + index as u32 * event_width;
            let label = preview_text(&event_team_name(event), if event_width > 72 { 9 } else { 7 });
            format!(
                "<rect x=\"{cx}\" y=\"{title_height}\" width=\"{event_width}\" height=\"{group_height}\" fill=\"#bfdbfe\" stroke=\"#1d4ed8\"/>\n<text x=\"{}\" y=\"{}\" font-family=\"Arial\" font-size=\"15\" font-weight=\"700\" text-anchor=\"middle\" fill=\"#111827\">{}</text>\n<rect x=\"{cx}\" y=\"{}\" width=\"{event_width}\" height=\"{header_height}\" fill=\"#eff6ff\" stroke=\"#1d4ed8\"/>\n<text x=\"{}\" y=\"{}\" font-family=\"Arial\" font-size=\"17\" font-weight=\"700\" text-anchor=\"middle\" fill=\"#111827\">{}</text>",
                cx + event_width / 2,
                title_height + 21,
                escape_xml(&label),
                title_height + group_height,
                cx + event_width / 2,
                title_height + group_height + 22,
                event.opponent_count
            )
        })
        .collect();
    let mut stat_headers = vec![format!(
        "<rect x=\"{stats_start_x}\" y=\"{title_height}\" width=\"{stats_width}\" height=\"{group_height}\" fill=\"#fef3c7\" stroke=\"#111827\"/>\n<text x=\"{}\" y=\"{}\" font-family=\"Arial\" font-size=\"17\" font-weight=\"700\" text-anchor=\"middle\" fill=\"#111827\">Kill Stats</text>",
        stats_start_x + stats_width / 2,
        title_height + 21
    )];
    let mut stat_x = stats_start_x;
    for (label, w) in stat_cols {
        let fill = if label == "#KAB" { "#0f7dba" } else { "#1f2937" };
        stat_headers.push(format!(
            "<rect x=\"{stat_x}\" y=\"{}\" width=\"{w}\" height=\"{header_height}\" fill=\"{fill}\" stroke=\"#111827\"/>\n<text x=\"{}\" y=\"{}\" font-family=\"Arial\" font-size=\"15\" font-weight=\"700\" text-anchor=\"middle\" fill=\"#ffffff\">{}</text>",
            title_height + group_height,
            stat_x + w / 2,
            title_height + group_height + 22,
            escape_xml(label)
        ));
        stat_x += w;
    }

    let mut body = Vec::new();
    for (row_index, wr) in rows.iter().enumerate() {
        let y = body_start_y + row_index as u32 * row_height;
        let base_fill = if row_index % 2 == 0 { "#ffffff" } else { "#dbeafe" };
        let mut cell_x = margin;
        let mut push = |w: u32, text: String, fill: &str, color: &str, weight: &str, center: bool| {
            let (anchor, tx) = if center { ("middle", cell_x + w / 2) } else { ("start", cell_x + 9) };
            body.push(format!(
                "<rect x=\"{cell_x}\" y=\"{y}\" width=\"{w}\" height=\"{row_height}\" fill=\"{fill}\" stroke=\"#2563eb\" stroke-width=\"0.75\"/>\n<text x=\"{tx}\" y=\"{}\" font-family=\"Arial\" font-size=\"14\" font-weight=\"{weight}\" text-anchor=\"{anchor}\" fill=\"{color}\">{}</text>",
                y + 21,
                escape_xml(&text)
            ));
            cell_x += w;
        };
        push(72, wr.rank.to_string(), base_fill, "#111827", "700", true);
        push(250, preview_text(&wr.row.name, 28), base_fill, "#111827", "400", false);
        for (i, (attended, value, possible)) in wr.cells.iter().enumerate() {
            let fill = weekly_event_cell_fill(*attended, value.unwrap_or(0), *possible, "");
            let fill = if fill.is_empty() { base_fill } else { fill };
            push(event_width, value.map(|v| v.to_string()).unwrap_or_default(), fill, "#111827", "700", true);
            let _ = i;
        }
        push(84, if wr.max > 0 { format!("{}%", fmt_num(wr.percent)) } else { String::new() }, base_fill, "#111827", "700", true);
        push(78, if wr.total > 0 { wr.total.to_string() } else { String::new() }, base_fill, "#111827", "700", true);
        push(76, if wr.max > 0 { wr.max.to_string() } else { String::new() }, base_fill, "#111827", "700", true);
        let kab_fill = if wr.row.kab > 0 { "#0f7dba" } else { base_fill };
        push(76, wr.row.kab.to_string(), kab_fill, if wr.row.kab > 0 { "#ffffff" } else { "#111827" }, "700", true);
    }

    format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{width}\" height=\"{height}\" viewBox=\"0 0 {width} {height}\">\n<defs>\n<linearGradient id=\"weeklyTitle\" x1=\"0\" y1=\"0\" x2=\"1\" y2=\"0\"><stop offset=\"0%\" stop-color=\"#d9ead3\"/><stop offset=\"100%\" stop-color=\"#bfdbfe\"/></linearGradient>\n<linearGradient id=\"weeklyBand\" x1=\"0\" y1=\"0\" x2=\"1\" y2=\"0\"><stop offset=\"0%\" stop-color=\"#0f766e\"/><stop offset=\"52%\" stop-color=\"#2563eb\"/><stop offset=\"100%\" stop-color=\"#7c3aed\"/></linearGradient>\n</defs>\n<rect width=\"100%\" height=\"100%\" fill=\"#f8fafc\"/>\n<rect x=\"0\" y=\"0\" width=\"{width}\" height=\"86\" fill=\"url(#weeklyBand)\"/>\n<rect x=\"0\" y=\"86\" width=\"{width}\" height=\"42\" fill=\"url(#weeklyTitle)\"/>\n<text x=\"{margin}\" y=\"35\" font-family=\"Arial\" font-size=\"25\" font-weight=\"700\" fill=\"#ffffff\">{}</text>\n<text x=\"{margin}\" y=\"63\" font-family=\"Arial\" font-size=\"15\" font-weight=\"600\" fill=\"#d1fae5\">{}</text>\n{pills}\n<text x=\"{}\" y=\"116\" font-family=\"Georgia, 'Times New Roman', serif\" font-size=\"31\" font-weight=\"700\" text-anchor=\"middle\" fill=\"#0f7dba\">Team Event - {}</text>\n{}\n{}\n{}\n{}\n</svg>",
        escape_xml(&format!("{} {} Report", model.team_name, if model.period == "weekly" { "Weekly" } else { "Monthly" })),
        escape_xml(&preview_text(&model.period_label, 62)),
        width / 2,
        escape_xml(&preview_text(&model.event_name, 42)),
        static_headers.join("\n"),
        event_headers.join("\n"),
        stat_headers.join("\n"),
        body.join("\n")
    )
}

pub fn build_report_chart_svg(model: &ReportModel) -> String {
    let rows: Vec<&ReportRow> = model.rows.iter().take(12).collect();
    let max_score = 1.max(rows.iter().map(|r| if r.total_score != 0 { r.total_score } else { r.total_points }).max().unwrap_or(0));
    let width = 1200u32;
    let height = 160 + rows.len() as u32 * 44 + 110;
    let (chart_x, chart_width) = (280i64, 680i64);
    let mut row_svgs = Vec::new();
    for (index, row) in rows.iter().enumerate() {
        let y = 130 + index as i64 * 44;
        let value = if row.total_score != 0 { row.total_score } else { row.total_points };
        let bar_width = ((value as f64 / max_score as f64) * chart_width as f64).round() as i64;
        let fill = if row.blue_kill_percent >= 75.0 {
            "#0f766e"
        } else if row.blue_kill_percent >= 40.0 {
            "#d97706"
        } else {
            "#2563eb"
        };
        row_svgs.push(format!(
            "<text x=\"34\" y=\"{}\" font-family=\"Arial\" font-size=\"17\" fill=\"#111827\">#{} {}</text><rect x=\"{chart_x}\" y=\"{y}\" width=\"{}\" height=\"28\" fill=\"{fill}\" rx=\"5\"/><text x=\"{}\" y=\"{}\" font-family=\"Arial\" font-size=\"15\" fill=\"#111827\">{value}</text><text x=\"1000\" y=\"{}\" font-family=\"Arial\" font-size=\"15\" fill=\"#111827\">{}/{} blues ({}%)</text>",
            y + 23,
            row.rank,
            escape_xml(&preview_text(&row.name, 24)),
            2.max(bar_width),
            chart_x + bar_width + 12,
            y + 21,
            y + 21,
            row.blues_killed,
            row.possible_blues,
            fmt_num(row.blue_kill_percent)
        ));
    }
    format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{width}\" height=\"{height}\" viewBox=\"0 0 {width} {height}\">\n<rect width=\"100%\" height=\"100%\" fill=\"#ffffff\"/>\n<rect width=\"{width}\" height=\"86\" fill=\"#0f766e\"/>\n<text x=\"34\" y=\"34\" font-family=\"Arial\" font-size=\"25\" font-weight=\"700\" fill=\"#ffffff\">{} {} Report</text>\n<text x=\"34\" y=\"62\" font-family=\"Arial\" font-size=\"16\" fill=\"#d1fae5\">{} vs {}</text>\n<text x=\"34\" y=\"112\" font-family=\"Arial\" font-size=\"18\" font-weight=\"700\" fill=\"#111827\">Top driver scores and blue kills</text>\n{}\n</svg>",
        escape_xml(&model.team_name),
        if model.period == "weekly" { "Weekly" } else { "Monthly" },
        escape_xml(&model.event_name),
        escape_xml(&model.enemy_teams.join(", ")),
        row_svgs.join("")
    )
}

pub fn build_report_table_svg(model: &ReportModel) -> String {
    if model.period == "weekly" {
        return build_weekly_table_svg(model);
    }
    let enemy = model.enemy_teams.join(", ");
    struct Col {
        label: &'static str,
        width: u32,
    }
    let columns = [
        Col { label: "Rank", width: 70 },
        Col { label: "Driver", width: 250 },
        Col { label: "Enemy", width: 230 },
        Col { label: "Best", width: 70 },
        Col { label: "Score", width: 120 },
        Col { label: "Pts", width: 90 },
        Col { label: "Blues", width: 100 },
        Col { label: "Blue %", width: 90 },
        Col { label: "#KAB", width: 80 },
    ];
    let (row_height, margin, title_height) = (34u32, 28u32, 150u32);
    let table_width: u32 = columns.iter().map(|c| c.width).sum();
    let width = table_width + margin * 2;
    let height = title_height + row_height * (model.rows.len() as u32 + 1) + 48;
    let mut x = margin;
    let header: Vec<String> = columns
        .iter()
        .map(|c| {
            let svg = format!(
                "<rect x=\"{x}\" y=\"{title_height}\" width=\"{}\" height=\"{row_height}\" fill=\"#1f2937\" stroke=\"#111827\"/>\n<text x=\"{}\" y=\"{}\" font-family=\"Arial\" font-size=\"14\" font-weight=\"700\" fill=\"#ffffff\">{}</text>",
                c.width,
                x + 9,
                title_height + 23,
                escape_xml(c.label)
            );
            x += c.width;
            svg
        })
        .collect();
    let mut body = Vec::new();
    for (index, row) in model.rows.iter().enumerate() {
        let y = title_height + row_height * (index as u32 + 1);
        let fill = if row.kab > 0 {
            "#fff2cc"
        } else if index % 2 == 0 {
            "#ffffff"
        } else {
            "#f8fafc"
        };
        let values = [
            row.rank.to_string(),
            row.name.clone(),
            enemy.clone(),
            row.best_rank.map(|r| r.to_string()).unwrap_or_default(),
            row.total_score.to_string(),
            row.total_points.to_string(),
            format!("{}/{}", row.blues_killed, row.possible_blues),
            format!("{}%", fmt_num(row.blue_kill_percent)),
            row.kab.to_string(),
        ];
        let mut cell_x = margin;
        for (c, v) in columns.iter().zip(values.iter()) {
            let text = preview_text(v, if c.width > 180 { 28 } else { 14 });
            body.push(format!(
                "<rect x=\"{cell_x}\" y=\"{y}\" width=\"{}\" height=\"{row_height}\" fill=\"{fill}\" stroke=\"#d1d5db\"/>\n<text x=\"{}\" y=\"{}\" font-family=\"Arial\" font-size=\"13\" fill=\"#111827\">{}</text>",
                c.width,
                cell_x + 9,
                y + 22,
                escape_xml(&text)
            ));
            cell_x += c.width;
        }
    }
    format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{width}\" height=\"{height}\" viewBox=\"0 0 {width} {height}\">\n<rect width=\"100%\" height=\"100%\" fill=\"#ffffff\"/>\n<rect width=\"{width}\" height=\"112\" fill=\"#0f766e\"/>\n<text x=\"{margin}\" y=\"38\" font-family=\"Arial\" font-size=\"25\" font-weight=\"700\" fill=\"#ffffff\">{} {} Report</text>\n<text x=\"{margin}\" y=\"70\" font-family=\"Arial\" font-size=\"16\" fill=\"#d1fae5\">{}</text>\n<text x=\"{margin}\" y=\"98\" font-family=\"Arial\" font-size=\"15\" fill=\"#d1fae5\">Enemy team(s): {}</text>\n{}\n{}\n</svg>",
        escape_xml(&model.team_name),
        if model.period == "weekly" { "Weekly" } else { "Monthly" },
        escape_xml(&model.event_name),
        escape_xml(&enemy),
        header.join("\n"),
        body.join("\n")
    )
}

/// Embed text for the Discord report embed.
pub fn report_embed_description(model: &ReportModel) -> String {
    let top: Vec<String> = model
        .rows
        .iter()
        .take(8)
        .map(|r| format!("#{} {} - score {}, pts {}, blues {}/{} ({}%), #KAB {}", r.rank, r.name, r.total_score, r.total_points, r.blues_killed, r.possible_blues, fmt_num(r.blue_kill_percent), r.kab))
        .collect();
    let text = [
        format!("Period: **{}**", model.period_label),
        format!("Team event: **{}**", clean_text(&model.event_name, "Team Event", 120)),
        format!("Enemy team(s): **{}**", model.enemy_teams.join(", ")),
        format!("Sessions: **{}** | Drivers: **{}** | Blues killed: **{}/{}**", model.totals.events, model.totals.players, model.totals.blues_killed, model.totals.possible_blues),
        String::new(),
        "**Top drivers**".to_string(),
        if top.is_empty() { "No drivers found.".to_string() } else { top.join("\n") },
    ]
    .join("\n");
    text.chars().take(4000).collect()
}

pub fn parse_anchor_date(value: &str) -> Result<DateTime<Utc>, String> {
    let date = NaiveDate::parse_from_str(value, "%Y-%m-%d").map_err(|_| "anchor_date must use YYYY-MM-DD format.".to_string())?;
    Ok(Utc.from_utc_datetime(&date.and_hms_opt(12, 0, 0).unwrap()))
}

#[cfg(test)]
mod differential_tests {
    use super::*;

    /// `DCA_BOUNDS_OUT=<file>` dumps the period bounds for many dates (diffed against the old bot's `periodBounds`).
    #[test]
    fn dump_period_bounds() {
        let Ok(path) = std::env::var("DCA_BOUNDS_OUT") else { return };
        let mut out = Vec::new();
        let mut day = Utc.with_ymd_and_hms(2025, 12, 20, 13, 30, 0).unwrap();
        for _ in 0..500 {
            for period in ["weekly", "monthly"] {
                let b = period_bounds(period, day);
                out.push(serde_json::json!({ "anchor": day.to_rfc3339(), "period": period, "start": b.start.to_rfc3339(), "end": b.end.to_rfc3339(), "key": b.key, "label": b.label }));
            }
            day += Duration::days(3);
        }
        std::fs::write(path, serde_json::to_string(&out).unwrap()).unwrap();
    }
}

#[cfg(test)]
mod model_differential_tests {
    use super::*;
    use dca_state::models::SpreadsheetSession;

    /// `DCA_REPORT_IN=<file>` (`{team, period, anchor, sessions}` cases) -> `DCA_REPORT_OUT`: the report model the Rust
    /// side builds, diffed against the old bot's `buildReportModel`.
    #[test]
    fn report_models() {
        let (Ok(input), Ok(output)) = (std::env::var("DCA_REPORT_IN"), std::env::var("DCA_REPORT_OUT")) else { return };
        let cases: Vec<serde_json::Value> = serde_json::from_str(&std::fs::read_to_string(input).unwrap()).unwrap();
        let mut results = Vec::new();
        for case in cases {
            let sessions: Vec<SpreadsheetSession> = serde_json::from_value(case["sessions"].clone()).unwrap();
            let aliases: Vec<String> = serde_json::from_value(case["team"]["ownPlayerAliases"].clone()).unwrap_or_default();
            let period = case["period"].as_str().unwrap();
            let anchor = DateTime::parse_from_rfc3339(case["anchor"].as_str().unwrap()).unwrap().with_timezone(&Utc);
            let bounds = period_bounds(period, anchor);
            let refs: Vec<&SpreadsheetSession> = sessions.iter().collect();
            let model = build_report_model(&TeamInfo { id: "t", name: "Team", own_player_aliases: &aliases }, &refs, period, &bounds, &sessions);
            results.push(serde_json::json!({
                "eventName": model.event_name,
                "enemyTeams": model.enemy_teams,
                "events": model.events.iter().map(|e| serde_json::json!({ "label": e.label, "eventName": e.event_name, "date": e.date, "opponentTeams": e.opponent_teams, "opponentCount": e.opponent_count, "topOpponentRank": e.top_opponent_rank, "maxScore": e.max_score, "maxPoints": e.max_points, "ownTeamScore": e.own_team_score, "opponentTeamScore": e.opponent_team_score })).collect::<Vec<_>>(),
                "rows": model.rows.iter().map(|r| serde_json::json!({
                    "name": r.name, "attended": r.attended, "missed": r.missed, "kab": r.kab, "totalScore": r.total_score, "totalPoints": r.total_points,
                    "bluesKilled": r.blues_killed, "possibleBlues": r.possible_blues, "blueKillPercent": r.blue_kill_percent, "bestRank": r.best_rank, "rank": r.rank,
                    "events": r.events.iter().map(|c| serde_json::json!({ "rank": c.rank, "score": c.score, "points": c.points, "bluesKilled": c.blues_killed, "possibleBlues": c.possible_blues, "blueKillPercent": c.blue_kill_percent, "kab": c.kab })).collect::<Vec<_>>()
                })).collect::<Vec<_>>(),
            }));
        }
        std::fs::write(output, serde_json::to_string_pretty(&results).unwrap()).unwrap();
    }
}
