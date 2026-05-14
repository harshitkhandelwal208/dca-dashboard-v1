const fs = require("fs");
const path = require("path");
const { AttachmentBuilder, EmbedBuilder } = require("discord.js");
const { listSpreadsheetSessions } = require("./spreadsheetStore");
const {
    blueKillPercent,
    bluesKilledForRank,
    cleanText,
    eventMaxPoints,
    eventMaxScore,
    eventNameForSession,
    normalizeKey,
    opponentCount,
    ownPlayersForSession,
    playerKey,
    pointsValue,
    scoreValue,
    sessionDate,
    sessionKabMap,
    sessionOpponentTeams,
    teamScoreFromMetadata,
    topOpponentRank
} = require("./spreadsheetMetrics");

const HEADER_COLOR = "FF1F2937";
const HEADER_TEXT = "FFFFFFFF";
const TITLE_FILL = "FF0F766E";
const OWN_FILL = "FFFFF2CC";
const ALT_FILL = "FFEAF2FF";
const KAB_FILL = "FF0F7DBA";
const MISSED_FILL = "FFFEE2E2";
const GOOD_FILL = "FFD9EAD3";
const WARN_FILL = "FFFFF2CC";

function safeFileName(value, fallback = "report") {
    const clean = String(value || fallback)
        .replace(/[/\\?%*:|"<>]/g, "-")
        .replace(/\s+/g, "-")
        .replace(/^-+|-+$/g, "")
        .slice(0, 90);
    return clean || fallback;
}

function escapeXml(value) {
    return String(value ?? "")
        .replace(/&/g, "&amp;")
        .replace(/</g, "&lt;")
        .replace(/>/g, "&gt;")
        .replace(/"/g, "&quot;")
        .replace(/'/g, "&apos;");
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

function samePeriod(session, bounds) {
    const date = sessionDate(session);
    return date >= bounds.start && date < bounds.end;
}

function findAnchorSession(sessions, bounds, anchorDate) {
    const processed = sessions
        .filter(session => session.status === "processed")
        .filter(session => samePeriod(session, bounds))
        .sort((a, b) => sessionDate(a) - sessionDate(b));
    if (!processed.length) return null;

    const anchor = new Date(anchorDate);
    return processed
        .slice()
        .reverse()
        .find(session => sessionDate(session) <= anchor) || processed[processed.length - 1];
}

function filterSessionsForReport(allSessions, period, bounds, anchorDate, options = {}) {
    const processed = allSessions
        .filter(session => session.status === "processed")
        .filter(session => samePeriod(session, bounds))
        .sort((a, b) => sessionDate(a) - sessionDate(b));

    if (period !== "weekly") return processed;

    const anchorSession = options.anchorSession || findAnchorSession(allSessions, bounds, anchorDate);
    const anchorEvent = normalizeKey(options.eventName || eventNameForSession(anchorSession || {}));
    if (!anchorEvent) return processed;

    return processed.filter(session => normalizeKey(eventNameForSession(session)) === anchorEvent);
}

function collectEnemyTeams(events) {
    const names = [];
    const seen = new Set();

    for (const event of events) {
        for (const name of event.opponentTeams) {
            const key = normalizeKey(name);
            if (!key || seen.has(key)) continue;
            seen.add(key);
            names.push(name);
        }
    }

    return names.length ? names : ["Opponent"];
}

function eventLabel(session, index, usedLabels) {
    const opponent = sessionOpponentTeams(session).join(", ");
    const date = sessionDate(session).toISOString().slice(0, 10);
    let label = `${date} vs ${opponent}`;
    if (label.length > 48) label = `${label.slice(0, 45).trimEnd()}...`;
    const base = label;
    let suffix = 2;
    while (usedLabels.has(label)) {
        label = `${base} ${suffix}`;
        suffix += 1;
    }
    usedLabels.add(label);
    return label;
}

function buildReportModel(teamConfig, sessions, period, bounds, allSessions = sessions) {
    const usedLabels = new Set();
    const eventName = period === "weekly"
        ? eventNameForSession(sessions[0] || {}, "Team Event")
        : "Multiple team events";
    const events = sessions.map((session, index) => ({
        id: session.id,
        label: eventLabel(session, index, usedLabels),
        eventName: eventNameForSession(session, `Event ${index + 1}`),
        date: sessionDate(session).toISOString().slice(0, 10),
        opponentTeams: sessionOpponentTeams(session),
        opponentCount: opponentCount(session),
        topOpponentRank: topOpponentRank(session),
        maxScore: eventMaxScore(session),
        maxPoints: eventMaxPoints(session),
        ownTeamScore: teamScoreFromMetadata(session, "own"),
        opponentTeamScore: teamScoreFromMetadata(session, "opponent"),
        session
    }));
    const enemyTeams = collectEnemyTeams(events);
    const playerMap = new Map();

    for (const alias of teamConfig.ownPlayerAliases || []) {
        const key = playerKey(alias);
        if (key && !playerMap.has(key)) {
            playerMap.set(key, { name: alias });
        }
    }

    for (const session of allSessions.filter(item => item.status === "processed")) {
        for (const player of ownPlayersForSession(session)) {
            const key = playerKey(player.playerName);
            if (key && !playerMap.has(key)) playerMap.set(key, { name: player.playerName });
        }
    }

    const rows = [...playerMap.entries()].map(([key, value]) => ({
        key,
        name: value.name,
        events: events.map(event => ({
            eventId: event.id,
            label: event.label,
            opponentTeams: event.opponentTeams,
            rank: null,
            score: 0,
            points: 0,
            bluesKilled: 0,
            possibleBlues: event.opponentCount,
            blueKillPercent: 0,
            kab: 0
        })),
        attended: 0,
        missed: events.length,
        kab: 0,
        totalScore: 0,
        totalPoints: 0,
        bluesKilled: 0,
        possibleBlues: events.reduce((total, event) => total + event.opponentCount, 0),
        blueKillPercent: 0,
        bestRank: null
    }));
    const rowByKey = new Map(rows.map(row => [row.key, row]));

    events.forEach((event, eventIndex) => {
        const kabMap = sessionKabMap(event.session);
        for (const player of ownPlayersForSession(event.session)) {
            const key = playerKey(player.playerName);
            if (!key) continue;

            if (!rowByKey.has(key)) {
                const next = {
                    key,
                    name: player.playerName,
                    events: events.map(item => ({
                        eventId: item.id,
                        label: item.label,
                        opponentTeams: item.opponentTeams,
                        rank: null,
                        score: 0,
                        points: 0,
                        bluesKilled: 0,
                        possibleBlues: item.opponentCount,
                        blueKillPercent: 0,
                        kab: 0
                    })),
                    attended: 0,
                    missed: events.length,
                    kab: 0,
                    totalScore: 0,
                    totalPoints: 0,
                    bluesKilled: 0,
                    possibleBlues: events.reduce((total, item) => total + item.opponentCount, 0),
                    blueKillPercent: 0,
                    bestRank: null
                };
                rowByKey.set(key, next);
                rows.push(next);
            }

            const row = rowByKey.get(key);
            const rank = Number.isFinite(Number(player.rank)) ? Number(player.rank) : null;
            const killed = rank === null ? 0 : bluesKilledForRank(event.session, rank);
            row.events[eventIndex] = {
                ...row.events[eventIndex],
                rank,
                score: scoreValue(player),
                points: pointsValue(player),
                bluesKilled: killed,
                possibleBlues: event.opponentCount,
                blueKillPercent: blueKillPercent(killed, event.opponentCount),
                kab: kabMap.get(player.playerName) || 0
            };
        }
    });

    for (const row of rows) {
        row.attended = row.events.filter(event => event.rank !== null).length;
        row.missed = Math.max(0, events.length - row.attended);
        row.kab = row.events.reduce((total, event) => total + event.kab, 0);
        row.totalScore = row.events.reduce((total, event) => total + event.score, 0);
        row.totalPoints = row.events.reduce((total, event) => total + event.points, 0);
        row.bluesKilled = row.events.reduce((total, event) => total + event.bluesKilled, 0);
        row.possibleBlues = row.events.reduce((total, event) => total + event.possibleBlues, 0);
        row.blueKillPercent = blueKillPercent(row.bluesKilled, row.possibleBlues);
        row.bestRank = row.events
            .map(event => event.rank)
            .filter(rank => rank !== null)
            .sort((a, b) => a - b)[0] || null;
    }

    rows.sort((a, b) =>
        b.totalScore - a.totalScore ||
        b.totalPoints - a.totalPoints ||
        b.bluesKilled - a.bluesKilled ||
        b.kab - a.kab ||
        a.name.localeCompare(b.name)
    );

    let rank = 0;
    let previousScore = null;
    let previousPoints = null;
    rows.forEach((row, index) => {
        if (row.totalScore !== previousScore || row.totalPoints !== previousPoints) rank = index + 1;
        row.rank = rank;
        previousScore = row.totalScore;
        previousPoints = row.totalPoints;
    });

    const periodKey = period === "weekly"
        ? `${bounds.key}-${safeFileName(eventName, "team-event").toLowerCase()}`
        : bounds.key;

    return {
        teamId: teamConfig.id,
        teamName: teamConfig.name || teamConfig.id,
        period,
        periodLabel: bounds.label,
        periodKey,
        start: bounds.start,
        end: bounds.end,
        eventName,
        enemyTeams,
        events,
        rows,
        totals: {
            events: events.length,
            players: rows.length,
            totalScore: rows.reduce((total, row) => total + row.totalScore, 0),
            totalPoints: rows.reduce((total, row) => total + row.totalPoints, 0),
            totalKab: rows.reduce((total, row) => total + row.kab, 0),
            totalMissed: rows.reduce((total, row) => total + row.missed, 0),
            bluesKilled: rows.reduce((total, row) => total + row.bluesKilled, 0),
            possibleBlues: rows.reduce((total, row) => total + row.possibleBlues, 0)
        }
    };
}

function styleCell(cell, options = {}) {
    cell.border = {
        top: { style: "thin", color: { argb: "FFCBD5E1" } },
        left: { style: "thin", color: { argb: "FFCBD5E1" } },
        bottom: { style: "thin", color: { argb: "FFCBD5E1" } },
        right: { style: "thin", color: { argb: "FFCBD5E1" } }
    };
    cell.alignment = { vertical: "middle", horizontal: options.horizontal || "center", wrapText: true };
    cell.font = { name: "Aptos", size: options.size || 11, bold: Boolean(options.bold), color: { argb: options.color || "FF111827" } };

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
    const headers = [
        "Driver Rank",
        "Driver",
        "Team Event",
        "Enemy Team(s)",
        "Best Rank",
        "Score",
        "Event Points",
        "Blues Killed",
        "Blue Kill %",
        "#KAB",
        "Missed",
        "Attended"
    ];

    const title = `${model.teamName} ${model.period === "weekly" ? "Weekly" : "Monthly"} Report`;
    const titleRow = addStyledRow(sheet, [title, ...Array(headers.length - 1).fill("")], {
        fill: TITLE_FILL,
        bold: true,
        color: HEADER_TEXT,
        size: 18
    });
    sheet.mergeCells(titleRow.number, 1, titleRow.number, headers.length);
    addStyledRow(sheet, [
        "Team Event",
        model.eventName,
        "Enemy Team(s)",
        model.enemyTeams.join(", "),
        "Period",
        model.periodLabel,
        "Sessions",
        model.events.length,
        "Drivers",
        model.rows.length,
        "",
        ""
    ], { fill: GOOD_FILL, bold: true });
    addStyledRow(sheet, headers, { fill: HEADER_COLOR, color: HEADER_TEXT, bold: true });

    model.rows.forEach((entry, index) => {
        const row = addStyledRow(sheet, [
            entry.rank,
            entry.name,
            model.eventName,
            model.enemyTeams.join(", "),
            entry.bestRank || "",
            entry.totalScore,
            entry.totalPoints,
            entry.bluesKilled,
            `${entry.blueKillPercent}%`,
            entry.kab,
            entry.missed,
            entry.attended
        ], {
            fill: index % 2 === 0 ? ALT_FILL : "FFFFFFFF"
        });
        row.getCell(2).alignment = { vertical: "middle", horizontal: "left", wrapText: true };
        if (entry.rank <= 3) {
            row.getCell(1).fill = { type: "pattern", pattern: "solid", fgColor: { argb: WARN_FILL } };
            row.getCell(1).font = { bold: true, color: { argb: "FF111827" } };
        }
        if (entry.blueKillPercent >= 75) {
            row.getCell(9).fill = { type: "pattern", pattern: "solid", fgColor: { argb: GOOD_FILL } };
        }
        if (entry.kab > 0) {
            row.getCell(10).fill = { type: "pattern", pattern: "solid", fgColor: { argb: KAB_FILL } };
            row.getCell(10).font = { bold: true, color: { argb: "FFFFFFFF" } };
        }
        if (entry.missed > 0) {
            row.getCell(11).fill = { type: "pattern", pattern: "solid", fgColor: { argb: MISSED_FILL } };
        }
    });

    sheet.views = [{ state: "frozen", ySplit: 3 }];
    sheet.columns = [
        { width: 12 },
        { width: 24 },
        { width: 28 },
        { width: 28 },
        { width: 11 },
        { width: 14 },
        { width: 14 },
        { width: 14 },
        { width: 13 },
        { width: 10 },
        { width: 10 },
        { width: 11 }
    ];
}

function addDetailsSheet(workbook, model) {
    const sheet = workbook.addWorksheet("Details");
    addStyledRow(sheet, ["Metric", "Value"], { fill: HEADER_COLOR, color: HEADER_TEXT, bold: true });
    [
        ["Team", model.teamName],
        ["Period", model.periodLabel],
        ["Team Event", model.eventName],
        ["Enemy Team(s)", model.enemyTeams.join(", ")],
        ["Sessions included", model.totals.events],
        ["Drivers included", model.totals.players],
        ["Total score", model.totals.totalScore],
        ["Total event points", model.totals.totalPoints],
        ["Blues killed", `${model.totals.bluesKilled}/${model.totals.possibleBlues}`],
        ["Blue kill %", `${blueKillPercent(model.totals.bluesKilled, model.totals.possibleBlues)}%`]
    ].forEach(item => addStyledRow(sheet, item));

    sheet.addRow([]);
    addStyledRow(sheet, ["Date", "Team Event", "Enemy Team(s)", "Own Team Score", "Enemy Team Score", "Enemy Drivers", "Max Score", "Max Event Points", "Session ID"], {
        fill: HEADER_COLOR,
        color: HEADER_TEXT,
        bold: true
    });
    for (const event of model.events) {
        addStyledRow(sheet, [
            event.date,
            event.eventName,
            event.opponentTeams.join(", "),
            event.ownTeamScore || "",
            event.opponentTeamScore || "",
            event.opponentCount,
            event.maxScore,
            event.maxPoints,
            event.id
        ]);
    }

    sheet.addRow([]);
    addStyledRow(sheet, ["Driver", "Date", "Enemy Team(s)", "Rank", "Score", "Event Points", "Blues Killed", "Blue Kill %", "#KAB"], {
        fill: HEADER_COLOR,
        color: HEADER_TEXT,
        bold: true
    });
    for (const row of model.rows) {
        for (const event of row.events) {
            addStyledRow(sheet, [
                row.name,
                model.events.find(item => item.id === event.eventId)?.date || "",
                event.opponentTeams.join(", "),
                event.rank || "",
                event.score,
                event.points,
                event.bluesKilled,
                `${event.blueKillPercent}%`,
                event.kab
            ], {
                fill: event.rank === null ? MISSED_FILL : event.kab ? OWN_FILL : ""
            });
        }
    }

    sheet.views = [{ state: "frozen", ySplit: 1 }];
    sheet.columns = [
        { width: 22 },
        { width: 14 },
        { width: 28 },
        { width: 10 },
        { width: 14 },
        { width: 14 },
        { width: 14 },
        { width: 12 },
        { width: 10 }
    ];
}

function previewTextValue(value, maxLength = 28) {
    const text = String(value ?? "")
        .replace(/\s+/g, " ")
        .trim();
    if (text.length <= maxLength) return text;
    return `${text.slice(0, Math.max(0, maxLength - 3))}...`;
}

function buildReportChartSvg(model) {
    const rows = model.rows.slice(0, 12);
    const maxScore = Math.max(1, ...rows.map(row => row.totalScore || row.totalPoints || 0));
    const width = 1200;
    const height = 160 + rows.length * 44 + 110;
    const chartX = 280;
    const chartWidth = 680;
    const rowSvgs = rows.map((row, index) => {
        const y = 130 + index * 44;
        const value = row.totalScore || row.totalPoints || 0;
        const barWidth = Math.round((value / maxScore) * chartWidth);
        const fill = row.blueKillPercent >= 75 ? "#0f766e" : row.blueKillPercent >= 40 ? "#d97706" : "#2563eb";
        return [
            `<text x="34" y="${y + 23}" font-family="Arial" font-size="17" fill="#111827">#${row.rank} ${escapeXml(previewTextValue(row.name, 24))}</text>`,
            `<rect x="${chartX}" y="${y}" width="${Math.max(2, barWidth)}" height="28" fill="${fill}" rx="5"/>`,
            `<text x="${chartX + barWidth + 12}" y="${y + 21}" font-family="Arial" font-size="15" fill="#111827">${value}</text>`,
            `<text x="1000" y="${y + 21}" font-family="Arial" font-size="15" fill="#111827">${row.bluesKilled}/${row.possibleBlues} blues (${row.blueKillPercent}%)</text>`
        ].join("");
    }).join("\n");

    return `<svg xmlns="http://www.w3.org/2000/svg" width="${width}" height="${height}" viewBox="0 0 ${width} ${height}">
<rect width="100%" height="100%" fill="#ffffff"/>
<rect width="${width}" height="86" fill="#0f766e"/>
<text x="34" y="34" font-family="Arial" font-size="25" font-weight="700" fill="#ffffff">${escapeXml(model.teamName)} ${model.period === "weekly" ? "Weekly" : "Monthly"} Report</text>
<text x="34" y="62" font-family="Arial" font-size="16" fill="#d1fae5">${escapeXml(model.eventName)} vs ${escapeXml(model.enemyTeams.join(", "))}</text>
<text x="34" y="112" font-family="Arial" font-size="18" font-weight="700" fill="#111827">Top driver scores and blue kills</text>
${rowSvgs}
</svg>`;
}

function buildReportTableSvg(model) {
    const columns = [
        { label: "Rank", width: 70, value: row => row.rank },
        { label: "Driver", width: 250, value: row => row.name },
        { label: "Enemy", width: 230, value: () => model.enemyTeams.join(", ") },
        { label: "Best", width: 70, value: row => row.bestRank || "" },
        { label: "Score", width: 120, value: row => row.totalScore },
        { label: "Pts", width: 90, value: row => row.totalPoints },
        { label: "Blues", width: 100, value: row => `${row.bluesKilled}/${row.possibleBlues}` },
        { label: "Blue %", width: 90, value: row => `${row.blueKillPercent}%` },
        { label: "#KAB", width: 80, value: row => row.kab }
    ];
    const rows = model.rows;
    const rowHeight = 34;
    const margin = 28;
    const titleHeight = 150;
    const tableWidth = columns.reduce((total, column) => total + column.width, 0);
    const width = tableWidth + margin * 2;
    const height = titleHeight + rowHeight * (rows.length + 1) + 48;

    let x = margin;
    const header = columns.map(column => {
        const svg = `<rect x="${x}" y="${titleHeight}" width="${column.width}" height="${rowHeight}" fill="#1f2937" stroke="#111827"/>
<text x="${x + 9}" y="${titleHeight + 23}" font-family="Arial" font-size="14" font-weight="700" fill="#ffffff">${escapeXml(column.label)}</text>`;
        x += column.width;
        return svg;
    }).join("\n");

    const body = rows.map((row, index) => {
        const y = titleHeight + rowHeight * (index + 1);
        const fill = row.kab > 0 ? "#fff2cc" : index % 2 === 0 ? "#ffffff" : "#f8fafc";
        let cellX = margin;
        return columns.map(column => {
            const text = previewTextValue(column.value(row), column.width > 180 ? 28 : 14);
            const svg = `<rect x="${cellX}" y="${y}" width="${column.width}" height="${rowHeight}" fill="${fill}" stroke="#d1d5db"/>
<text x="${cellX + 9}" y="${y + 22}" font-family="Arial" font-size="13" fill="#111827">${escapeXml(text)}</text>`;
            cellX += column.width;
            return svg;
        }).join("\n");
    }).join("\n");

    return `<svg xmlns="http://www.w3.org/2000/svg" width="${width}" height="${height}" viewBox="0 0 ${width} ${height}">
<rect width="100%" height="100%" fill="#ffffff"/>
<rect width="${width}" height="112" fill="#0f766e"/>
<text x="${margin}" y="38" font-family="Arial" font-size="25" font-weight="700" fill="#ffffff">${escapeXml(model.teamName)} ${model.period === "weekly" ? "Weekly" : "Monthly"} Report</text>
<text x="${margin}" y="70" font-family="Arial" font-size="16" fill="#d1fae5">${escapeXml(model.eventName)}</text>
<text x="${margin}" y="98" font-family="Arial" font-size="15" fill="#d1fae5">Enemy team(s): ${escapeXml(model.enemyTeams.join(", "))}</text>
${header}
${body}
</svg>`;
}

async function saveSvgAsPng(svg, outputPath) {
    try {
        const sharp = require("sharp");
        await sharp(Buffer.from(svg)).png().toFile(outputPath);
        if (fs.existsSync(outputPath)) return outputPath;
    } catch {
        // Keep a viewable fallback if rasterization is unavailable in the host.
    }

    const svgPath = outputPath.replace(/\.png$/i, ".svg");
    await fs.promises.writeFile(svgPath, svg, "utf8");
    return svgPath;
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

    const baseName = `${safeFileName(model.teamId)}-${model.period}-${model.periodKey}`;
    const filePath = path.join(outputDir, `${baseName}.xlsx`);
    await workbook.xlsx.writeFile(filePath);
    const chartPath = await saveSvgAsPng(buildReportChartSvg(model), path.join(outputDir, `${baseName}-chart.png`));
    const tableImagePath = await saveSvgAsPng(buildReportTableSvg(model), path.join(outputDir, `${baseName}-drivers.png`));

    return { filePath, chartPath, tableImagePath };
}

function outputDirFor(teamConfig) {
    return path.join(__dirname, "..", "data", "spreadsheets", safeFileName(teamConfig.id, "team"), "reports");
}

async function generatePeriodReport(teamConfig, period, options = {}) {
    const anchor = options.anchorDate ? new Date(options.anchorDate) : new Date();
    const anchorDate = Number.isNaN(anchor.getTime()) ? new Date() : anchor;
    const bounds = periodBounds(period, anchorDate);
    const allSessions = options.sessions || await listSpreadsheetSessions({ teamId: teamConfig.id });
    const sessions = filterSessionsForReport(allSessions, period, bounds, anchorDate, options);
    if (!sessions.length) return null;

    const model = buildReportModel(teamConfig, sessions, period, bounds, allSessions);
    const outputs = await writeReportWorkbook(model, options.outputDir || outputDirFor(teamConfig));

    return {
        ...model,
        ...outputs
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

function attachmentFor(filePath) {
    if (!filePath || !fs.existsSync(filePath)) return null;
    return new AttachmentBuilder(filePath, { name: path.basename(filePath) });
}

function reportAttachment(report) {
    return attachmentFor(report?.filePath);
}

function reportAttachments(report) {
    return [
        attachmentFor(report?.filePath),
        attachmentFor(report?.tableImagePath),
        attachmentFor(report?.chartPath)
    ].filter(Boolean);
}

function buildPeriodReportEmbed(report) {
    const topRows = report.rows.slice(0, 8).map(row =>
        `#${row.rank} ${row.name} - score ${row.totalScore}, pts ${row.totalPoints}, blues ${row.bluesKilled}/${row.possibleBlues} (${row.blueKillPercent}%), #KAB ${row.kab}`
    );

    return new EmbedBuilder()
        .setTitle(`${report.teamName} ${report.period === "weekly" ? "Weekly" : "Monthly"} Report`)
        .setColor(0x0f7dba)
        .setDescription([
            `Period: **${report.periodLabel}**`,
            `Team event: **${cleanText(report.eventName, "Team Event", 120)}**`,
            `Enemy team(s): **${report.enemyTeams.join(", ")}**`,
            `Sessions: **${report.totals.events}** | Drivers: **${report.totals.players}** | Blues killed: **${report.totals.bluesKilled}/${report.totals.possibleBlues}**`,
            "",
            "**Top drivers**",
            topRows.length ? topRows.join("\n") : "No drivers found."
        ].join("\n").slice(0, 4000))
        .setTimestamp(new Date());
}

module.exports = {
    buildPeriodReportEmbed,
    buildReportModel,
    generatePeriodReport,
    generateStandardPeriodReports,
    periodBounds,
    reportAttachment,
    reportAttachments
};
