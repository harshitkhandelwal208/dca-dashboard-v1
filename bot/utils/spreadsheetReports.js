const fs = require("fs");
const path = require("path");
const { AttachmentBuilder, EmbedBuilder } = require("discord.js");
const { listSpreadsheetSessions } = require("./spreadsheetStore");

const HEADER_COLOR = "FF1F2937";
const HEADER_TEXT = "FFFFFFFF";
const OWN_FILL = "FFFFF2CC";
const ALT_FILL = "FFEAF2FF";
const KAB_FILL = "FF0F7DBA";
const MISSED_FILL = "FFFEE2E2";

function safeFileName(value, fallback = "report") {
    const clean = String(value || fallback)
        .replace(/[/\\?%*:|"<>]/g, "-")
        .replace(/\s+/g, "-")
        .replace(/^-+|-+$/g, "")
        .slice(0, 90);
    return clean || fallback;
}

function sessionDate(session) {
    const value = session.processedAt || session.updatedAt || session.createdAt || new Date().toISOString();
    const date = new Date(value);
    return Number.isNaN(date.getTime()) ? new Date() : date;
}

function startOfUtcDay(date) {
    return new Date(Date.UTC(date.getUTCFullYear(), date.getUTCMonth(), date.getUTCDate()));
}

function periodBounds(period, anchor = new Date()) {
    const day = startOfUtcDay(anchor);

    if (period === "weekly") {
        const start = new Date(day);
        const offset = (start.getUTCDay() + 6) % 7;
        start.setUTCDate(start.getUTCDate() - offset);
        const end = new Date(start);
        end.setUTCDate(end.getUTCDate() + 7);
        return {
            start,
            end,
            key: `${start.getUTCFullYear()}-W${isoWeekNumber(start)}`,
            label: `Week of ${start.toISOString().slice(0, 10)}`
        };
    }

    const start = new Date(Date.UTC(day.getUTCFullYear(), day.getUTCMonth(), 1));
    const end = new Date(Date.UTC(day.getUTCFullYear(), day.getUTCMonth() + 1, 1));
    return {
        start,
        end,
        key: `${start.getUTCFullYear()}-${String(start.getUTCMonth() + 1).padStart(2, "0")}`,
        label: start.toLocaleString("en-US", { month: "long", year: "numeric", timeZone: "UTC" })
    };
}

function isoWeekNumber(date) {
    const target = new Date(Date.UTC(date.getUTCFullYear(), date.getUTCMonth(), date.getUTCDate()));
    const dayNumber = target.getUTCDay() || 7;
    target.setUTCDate(target.getUTCDate() + 4 - dayNumber);
    const yearStart = new Date(Date.UTC(target.getUTCFullYear(), 0, 1));
    return String(Math.ceil((((target - yearStart) / 86400000) + 1) / 7)).padStart(2, "0");
}

function eventName(session, index, usedNames) {
    const raw = session.metadata?.title || session.teamEventName || session.eventName || `Event ${index + 1}`;
    let name = String(raw).replace(/\s+/g, " ").trim() || `Event ${index + 1}`;
    if (name.length > 42) name = name.slice(0, 39).trimEnd() + "...";

    const base = name;
    let suffix = 2;
    while (usedNames.has(name)) {
        name = `${base} ${suffix}`;
        suffix += 1;
    }
    usedNames.add(name);
    return name;
}

function playerKey(name) {
    return String(name || "").toLowerCase().replace(/[^a-z0-9]+/g, " ").trim();
}

function eventScore(player) {
    const value = player?.points ?? player?.score ?? 0;
    const number = Number(value);
    return Number.isFinite(number) ? number : 0;
}

function eventMax(session) {
    return Math.max(0, ...(session.players || []).map(eventScore));
}

function topOpponentRank(session) {
    const opponents = (session.players || [])
        .filter(player => player.teamType !== "own")
        .map(player => Number(player.rank))
        .filter(rank => Number.isFinite(rank));
    return opponents.length ? Math.min(...opponents) : null;
}

function ownPlayersForSession(session) {
    return (session.players || []).filter(player => player.teamType === "own");
}

function buildReportModel(teamConfig, sessions, period, bounds, allSessions = sessions) {
    const usedNames = new Set();
    const events = sessions.map((session, index) => ({
        id: session.id,
        name: eventName(session, index, usedNames),
        date: sessionDate(session).toISOString().slice(0, 10),
        max: eventMax(session),
        topOpponentRank: topOpponentRank(session),
        session
    }));
    const playerMap = new Map();

    for (const session of allSessions.filter(item => item.status === "processed")) {
        for (const player of ownPlayersForSession(session)) {
            const key = playerKey(player.playerName);
            if (!key || playerMap.has(key)) continue;
            playerMap.set(key, {
                name: player.playerName,
                scores: Array(events.length).fill(0),
                ranks: Array(events.length).fill(null),
                attended: 0,
                missed: 0,
                kab: 0,
                total: 0,
                max: 0,
                percent: 0
            });
        }
    }

    events.forEach((event, eventIndex) => {
        for (const player of ownPlayersForSession(event.session)) {
            const key = playerKey(player.playerName);
            if (!key) continue;

            if (!playerMap.has(key)) {
                playerMap.set(key, {
                    name: player.playerName,
                    scores: Array(events.length).fill(0),
                    ranks: Array(events.length).fill(null),
                    attended: 0,
                    missed: 0,
                    kab: 0,
                    total: 0,
                    max: 0,
                    percent: 0
                });
            }

            const row = playerMap.get(key);
            const score = eventScore(player);
            row.scores[eventIndex] = score;
            row.ranks[eventIndex] = Number.isFinite(Number(player.rank)) ? Number(player.rank) : null;
        }
    });

    const maxTotal = events.reduce((total, event) => total + event.max, 0);
    const rows = [...playerMap.values()].map(row => {
        row.total = row.scores.reduce((total, value) => total + value, 0);
        row.max = maxTotal;
        row.attended = row.ranks.filter(rank => rank !== null).length;
        row.missed = Math.max(0, events.length - row.attended);
        row.percent = row.max > 0 ? Math.round((row.total / row.max) * 100) : 0;
        row.kab = events.reduce((total, event, index) => {
            const rank = row.ranks[index];
            if (rank === null) return total;
            if (event.topOpponentRank !== null) {
                return rank < event.topOpponentRank ? total + 1 : total;
            }
            return total;
        }, 0);
        return row;
    }).sort((a, b) =>
        b.total - a.total ||
        b.kab - a.kab ||
        b.percent - a.percent ||
        a.name.localeCompare(b.name)
    );

    let rank = 0;
    let previousTotal = null;
    rows.forEach((row, index) => {
        if (row.total !== previousTotal) rank = index + 1;
        row.rank = rank;
        previousTotal = row.total;
    });

    return {
        teamId: teamConfig.id,
        teamName: teamConfig.name || teamConfig.id,
        period,
        periodLabel: bounds.label,
        periodKey: bounds.key,
        start: bounds.start,
        end: bounds.end,
        events,
        rows,
        totals: {
            events: events.length,
            players: rows.length,
            maxTotal,
            totalScores: rows.reduce((total, row) => total + row.total, 0),
            totalKab: rows.reduce((total, row) => total + row.kab, 0),
            totalMissed: rows.reduce((total, row) => total + row.missed, 0)
        }
    };
}

function styleCell(cell, options = {}) {
    cell.border = {
        top: { style: "thin", color: { argb: "FF4F7FD9" } },
        left: { style: "thin", color: { argb: "FF4F7FD9" } },
        bottom: { style: "thin", color: { argb: "FF4F7FD9" } },
        right: { style: "thin", color: { argb: "FF4F7FD9" } }
    };
    cell.alignment = { vertical: "middle", horizontal: options.horizontal || "center", wrapText: true };
    cell.font = { name: "Georgia", size: options.size || 12, bold: Boolean(options.bold), color: { argb: options.color || "FF000000" } };

    if (options.fill) {
        cell.fill = { type: "pattern", pattern: "solid", fgColor: { argb: options.fill } };
    }
}

function addStyledRow(worksheet, values, options = {}) {
    const row = worksheet.addRow(values);
    row.eachCell({ includeEmpty: true }, cell => styleCell(cell, options));
    return row;
}

function addReportSheet(workbook, model) {
    const sheet = workbook.addWorksheet("Report");
    const statStart = 3 + model.events.length;
    const headers = [
        "Rank",
        "Name",
        ...model.events.map(event => event.name),
        "%kill",
        "total",
        "max",
        "#KAB",
        "missed",
        "attended"
    ];

    const titleRow = addStyledRow(sheet, [`${model.teamName} - ${model.period === "weekly" ? "Weekly" : "Monthly"} Report`, ...Array(headers.length - 1).fill("")], {
        fill: "FFC6E0B4",
        bold: true,
        color: "FF0070C0",
        size: 20
    });
    sheet.mergeCells(titleRow.number, 1, titleRow.number, headers.length);

    addStyledRow(sheet, [
        "Event max",
        "",
        ...model.events.map(event => event.max),
        "",
        "",
        model.totals.maxTotal,
        "",
        "",
        ""
    ], { fill: "FFD9EAD3", bold: true });
    addStyledRow(sheet, headers, { fill: HEADER_COLOR, color: HEADER_TEXT, bold: true });

    model.rows.forEach((entry, index) => {
        const values = [
            entry.rank,
            entry.name,
            ...entry.scores,
            `${entry.percent} %`,
            entry.total,
            entry.max,
            entry.kab,
            entry.missed,
            entry.attended
        ];
        const row = addStyledRow(sheet, values, { fill: index % 2 === 0 ? ALT_FILL : "FFFFFFFF" });
        row.getCell(2).alignment = { vertical: "middle", horizontal: "left", wrapText: true };

        entry.scores.forEach((score, eventIndex) => {
            const cell = row.getCell(3 + eventIndex);
            if (entry.ranks[eventIndex] === null) {
                cell.value = 0;
                cell.fill = { type: "pattern", pattern: "solid", fgColor: { argb: MISSED_FILL } };
            } else if (model.events[eventIndex].topOpponentRank !== null && entry.ranks[eventIndex] < model.events[eventIndex].topOpponentRank) {
                cell.fill = { type: "pattern", pattern: "solid", fgColor: { argb: OWN_FILL } };
            } else if (score >= model.events[eventIndex].max && model.events[eventIndex].max > 0) {
                cell.fill = { type: "pattern", pattern: "solid", fgColor: { argb: OWN_FILL } };
            }
        });

        row.getCell(statStart + 3).fill = {
            type: "pattern",
            pattern: "solid",
            fgColor: { argb: entry.kab > 0 ? KAB_FILL : "FFDDEBF7" }
        };
        row.getCell(statStart + 3).font = { bold: true, color: { argb: entry.kab > 0 ? "FFFFFFFF" : "FF000000" } };
    });

    sheet.views = [{ state: "frozen", xSplit: 2, ySplit: 3 }];
    sheet.columns = headers.map((header, index) => ({
        width: index === 1 ? 24 : Math.max(9, Math.min(18, String(header).length + 2))
    }));
}

function addDetailsSheet(workbook, model) {
    const sheet = workbook.addWorksheet("Details");
    addStyledRow(sheet, ["Metric", "Value"], { fill: HEADER_COLOR, color: HEADER_TEXT, bold: true });
    [
        ["Team", model.teamName],
        ["Period", model.periodLabel],
        ["Events included", model.totals.events],
        ["Players included", model.totals.players],
        ["Max score", model.totals.maxTotal],
        ["#KAB definition", "Count of events where the player ranked above every opponent. If no opponent rows were detected, #KAB is 0 for that event because opponent order cannot be proven."],
        ["Missing event rule", "Missing players receive a score of 0 for that event and the full event max still counts against max."]
    ].forEach(item => addStyledRow(sheet, item));

    sheet.addRow([]);
    addStyledRow(sheet, ["Date", "Event name", "Session ID", "Max score", "Players parsed"], {
        fill: HEADER_COLOR,
        color: HEADER_TEXT,
        bold: true
    });
    for (const event of model.events) {
        addStyledRow(sheet, [
            event.date,
            event.name,
            event.id,
            event.max,
            event.session.players?.length || 0
        ]);
    }

    sheet.columns = [
        { width: 18 },
        { width: 42 },
        { width: 28 },
        { width: 12 },
        { width: 14 }
    ];
}

async function writeReportWorkbook(model, outputDir) {
    const ExcelJS = require("exceljs");
    await fs.promises.mkdir(outputDir, { recursive: true });

    const workbook = new ExcelJS.Workbook();
    workbook.creator = "DCA Bot";
    workbook.created = new Date();
    workbook.modified = new Date();
    addReportSheet(workbook, model);
    addDetailsSheet(workbook, model);

    const fileName = `${safeFileName(model.teamId)}-${model.period}-${model.periodKey}.xlsx`;
    const filePath = path.join(outputDir, fileName);
    await workbook.xlsx.writeFile(filePath);
    return filePath;
}

function outputDirFor(teamConfig) {
    return path.join(__dirname, "..", "data", "spreadsheets", safeFileName(teamConfig.id, "team"), "reports");
}

function filterSessionsForPeriod(sessions, bounds) {
    return sessions
        .filter(session => session.status === "processed")
        .filter(session => {
            const date = sessionDate(session);
            return date >= bounds.start && date < bounds.end;
        })
        .sort((a, b) => sessionDate(a) - sessionDate(b));
}

async function generatePeriodReport(teamConfig, period, options = {}) {
    const anchor = options.anchorDate ? new Date(options.anchorDate) : new Date();
    const bounds = periodBounds(period, Number.isNaN(anchor.getTime()) ? new Date() : anchor);
    const allSessions = options.sessions || await listSpreadsheetSessions({ teamId: teamConfig.id });
    const sessions = filterSessionsForPeriod(allSessions, bounds);
    if (!sessions.length) return null;

    const model = buildReportModel(teamConfig, sessions, period, bounds, allSessions);
    const filePath = await writeReportWorkbook(model, outputDirFor(teamConfig));

    return {
        ...model,
        filePath
    };
}

async function generateStandardPeriodReports(teamConfig, anchorSession) {
    const anchorDate = sessionDate(anchorSession || {});
    const sessions = await listSpreadsheetSessions({ teamId: teamConfig.id });
    const reports = [];

    for (const period of ["weekly", "monthly"]) {
        const report = await generatePeriodReport(teamConfig, period, { anchorDate, sessions });
        if (report) reports.push(report);
    }

    return reports;
}

function reportAttachment(report) {
    if (!report?.filePath || !fs.existsSync(report.filePath)) return null;
    return new AttachmentBuilder(report.filePath, {
        name: path.basename(report.filePath)
    });
}

function buildPeriodReportEmbed(report) {
    const topRows = report.rows.slice(0, 8).map(row =>
        `#${row.rank} ${row.name} - ${row.total}/${row.max} (${row.percent}%), #KAB ${row.kab}, missed ${row.missed}`
    );
    const eventNames = report.events.map(event => `${event.date}: ${event.name}`);

    return new EmbedBuilder()
        .setTitle(`${report.teamName} ${report.period === "weekly" ? "Weekly" : "Monthly"} Report`)
        .setColor(0x0f7dba)
        .setDescription([
            `Period: **${report.periodLabel}**`,
            `Events: **${report.totals.events}** | Players: **${report.totals.players}** | #KAB total: **${report.totals.totalKab}**`,
            "",
            "**Top performers**",
            topRows.length ? topRows.join("\n") : "No players found.",
            "",
            "**Team events**",
            eventNames.slice(0, 12).join("\n") || "No event names found."
        ].join("\n").slice(0, 4000))
        .setFooter({ text: "#KAB counts events where the player ranked above every opponent. Missed events score 0." })
        .setTimestamp(new Date());
}

module.exports = {
    buildPeriodReportEmbed,
    buildReportModel,
    generatePeriodReport,
    generateStandardPeriodReports,
    periodBounds,
    reportAttachment
};
