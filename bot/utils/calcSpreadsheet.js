const fs = require("fs");
const path = require("path");
const { runCommand } = require("./raceOcr");
const {
    blueKillPercent,
    bluesKilledForRank,
    eventMaxPoints,
    eventMaxScore,
    eventNameForSession,
    opponentCount,
    opponentPlayersForSession,
    ownPlayersForSession,
    pointsValue,
    scoreValue,
    sessionKabMap,
    sessionOpponentTeams,
    teamScoreFromMetadata
} = require("./spreadsheetMetrics");

const OWN_COLOR = "#fff2cc";
const OPPONENT_COLOR = "#dbeafe";
const HEADER_COLOR = "#1f2937";
const TITLE_COLOR = "#0f766e";
const ACCENT_GREEN = "#d9ead3";
const ACCENT_RED = "#fee2e2";
const ACCENT_BLUE = "#dbeafe";
const KAB_COLOR = "#0f7dba";
const IMAGE_HEADER_COLORS = ["#0f766e", "#2563eb", "#16a34a", "#7c3aed", "#0891b2", "#d97706", "#be123c"];

function escapeXml(value) {
    return String(value ?? "")
        .replace(/&/g, "&amp;")
        .replace(/</g, "&lt;")
        .replace(/>/g, "&gt;")
        .replace(/"/g, "&quot;")
        .replace(/'/g, "&apos;");
}

function safeSheetName(value) {
    return String(value || "Sheet")
        .replace(/[:\\/?*\[\]]/g, " ")
        .replace(/\s+/g, " ")
        .trim()
        .slice(0, 31) || "Sheet";
}

function safeFileName(value, fallback = "spreadsheet") {
    const clean = String(value || fallback)
        .replace(/[/\\?%*:|"<>]/g, "-")
        .replace(/\s+/g, "-")
        .replace(/^-+|-+$/g, "")
        .slice(0, 90);
    return clean || fallback;
}

function cell(value, styleName = "Cell") {
    if (value === null || value === undefined || value === "") {
        return `<table:table-cell table:style-name="${styleName}" office:value-type="string"><text:p></text:p></table:table-cell>`;
    }

    if (typeof value === "number" && Number.isFinite(value)) {
        return `<table:table-cell table:style-name="${styleName}" office:value-type="float" office:value="${value}"><text:p>${value}</text:p></table:table-cell>`;
    }

    return `<table:table-cell table:style-name="${styleName}" office:value-type="string"><text:p>${escapeXml(value)}</text:p></table:table-cell>`;
}

function row(values, styleName = "Cell") {
    return `<table:table-row>${values.map(value => cell(value, styleName)).join("")}</table:table-row>`;
}

function table(name, rowsXml) {
    return [
        `<table:table table:name="${escapeXml(safeSheetName(name))}">`,
        '<table:table-column table:number-columns-repeated="12" table:style-name="Column"/>',
        rowsXml,
        "</table:table>"
    ].join("\n");
}

function eventSummary(session) {
    const stats = session.stats || {};
    const kabMap = sessionKabMap(session);
    const ownPlayers = ownPlayersForSession(session);
    const opponents = opponentPlayersForSession(session);
    const topDriver = ownPlayers
        .slice()
        .sort((a, b) => Number(a.rank || 999) - Number(b.rank || 999))[0];

    return {
        eventName: eventNameForSession(session),
        teamName: session.metadata?.ownTeamName || session.teamName || session.teamId || "Team",
        opponentTeams: sessionOpponentTeams(session),
        ownPlayers,
        opponents,
        opponentCount: opponentCount(session),
        ownPoints: stats.ownPoints || ownPlayers.reduce((total, player) => total + pointsValue(player), 0),
        opponentPoints: stats.opponentPoints || opponents.reduce((total, player) => total + pointsValue(player), 0),
        ownScore: stats.ownScore || ownPlayers.reduce((total, player) => total + scoreValue(player), 0),
        opponentScore: stats.opponentScore || opponents.reduce((total, player) => total + scoreValue(player), 0),
        ownTeamScore: teamScoreFromMetadata(session, "own"),
        opponentTeamScore: teamScoreFromMetadata(session, "opponent"),
        eventMaxScore: eventMaxScore(session),
        eventMaxPoints: eventMaxPoints(session),
        kabCount: [...kabMap.values()].reduce((total, value) => total + value, 0),
        topDriver
    };
}

function playerDisplayRows(session) {
    const kabMap = sessionKabMap(session);
    const possibleBlues = opponentCount(session);

    return (session.players || [])
        .slice()
        .sort((a, b) => Number(a.rank || 999) - Number(b.rank || 999))
        .map(player => {
            const own = player.teamType === "own";
            const killed = own ? bluesKilledForRank(session, player.rank) : "";
            const killedPercent = own ? blueKillPercent(killed, possibleBlues) : "";

            return {
                own,
                values: [
                    player.rank ?? "",
                    player.playerName || "",
                    player.teamLabel || (own ? session.teamName || session.teamId || "Own team" : "Opponent"),
                    own ? "Own" : "Opponent",
                    pointsValue(player) || "",
                    scoreValue(player) || "",
                    killed,
                    killedPercent === "" ? "" : `${killedPercent}%`,
                    kabMap.get(player.playerName) || 0
                ]
            };
        });
}

function summaryRows(session) {
    const summary = eventSummary(session);
    const rows = [
        row(["Metric", "Value"], "HeaderCell"),
        row(["Team Event", summary.eventName]),
        row(["Own Team", summary.teamName]),
        row(["Enemy Team(s)", summary.opponentTeams.join(", ")]),
        row(["Submission ID", session.id]),
        row(["Images", (session.images || []).length]),
        row(["Own Drivers", summary.ownPlayers.length]),
        row(["Enemy Drivers", summary.opponentCount]),
        row(["Own Event Points", summary.ownPoints]),
        row(["Enemy Event Points", summary.opponentPoints]),
        row(["Own Score", summary.ownScore]),
        row(["Enemy Score", summary.opponentScore]),
        row(["Own Team Score", summary.ownTeamScore || ""]),
        row(["Enemy Team Score", summary.opponentTeamScore || ""]),
        row(["#KAB", summary.kabCount])
    ];

    if (summary.topDriver) {
        rows.push(row(["Top Own Driver", `#${summary.topDriver.rank} ${summary.topDriver.playerName}`]));
    }

    return rows.join("\n");
}

function resultRows(session) {
    const headers = [
        "Rank",
        "Player",
        "Team",
        "Type",
        "Event Points",
        "Score",
        "Blues Killed",
        "Blue Kill %",
        "#KAB"
    ];
    const rows = [row(headers, "HeaderCell")];

    for (const item of playerDisplayRows(session)) {
        rows.push(row(item.values, item.own ? "OwnCell" : "OpponentCell"));
    }

    return rows.join("\n");
}

function summaryWorkbookRows(session) {
    const summary = eventSummary(session);
    const rows = [
        { values: ["Metric", "Value"], header: true },
        { values: ["Team Event", summary.eventName], fill: ACCENT_GREEN },
        { values: ["Own Team", summary.teamName] },
        { values: ["Enemy Team(s)", summary.opponentTeams.join(", ")], fill: ACCENT_BLUE },
        { values: ["Submission ID", session.id] },
        { values: ["Images", (session.images || []).length] },
        { values: ["Own Drivers", summary.ownPlayers.length] },
        { values: ["Enemy Drivers", summary.opponentCount] },
        { values: ["Own Event Points", summary.ownPoints] },
        { values: ["Enemy Event Points", summary.opponentPoints] },
        { values: ["Own Score", summary.ownScore] },
        { values: ["Enemy Score", summary.opponentScore] },
        { values: ["Own Team Score", summary.ownTeamScore || ""] },
        { values: ["Enemy Team Score", summary.opponentTeamScore || ""] },
        { values: ["#KAB", summary.kabCount], fill: summary.kabCount ? "#cfe2ff" : "" }
    ];

    if (summary.topDriver) {
        rows.push({ values: ["Top Own Driver", `#${summary.topDriver.rank} ${summary.topDriver.playerName}`], fill: "#e2f0d9" });
    }

    return rows;
}

function resultWorkbookRows(session) {
    const headers = [
        "Rank",
        "Player",
        "Team",
        "Type",
        "Event Points",
        "Score",
        "Blues Killed",
        "Blue Kill %",
        "#KAB"
    ];

    return [
        { values: headers, header: true },
        ...playerDisplayRows(session).map(item => ({
            values: item.values,
            fill: item.own ? OWN_COLOR : OPPONENT_COLOR,
            kab: Number(item.values[8]) > 0,
            own: item.own
        }))
    ];
}

function attendanceWorkbookRows(session) {
    const missing = (session.attendance?.missingPlayers || []).filter(Boolean);
    const attended = ownPlayersForSession(session);

    return [
        { values: ["Player", "Status", "Event Points", "Score", "Rank"], header: true },
        ...attended.map(player => ({
            values: [
                player.playerName,
                "attended",
                pointsValue(player) || "",
                scoreValue(player) || "",
                player.rank ?? ""
            ],
            fill: OWN_COLOR
        })),
        ...missing.map(name => ({
            values: [name, "missed", 0, 0, ""],
            fill: ACCENT_RED
        }))
    ];
}

function buildFods(session) {
    return `<?xml version="1.0" encoding="UTF-8"?>
<office:document xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:style="urn:oasis:names:tc:opendocument:xmlns:style:1.0" xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0" xmlns:table="urn:oasis:names:tc:opendocument:xmlns:table:1.0" xmlns:fo="urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0" office:version="1.2">
<office:automatic-styles>
<style:style style:name="Column" style:family="table-column"><style:table-column-properties style:column-width="1.35in"/></style:style>
<style:style style:name="Cell" style:family="table-cell"><style:table-cell-properties fo:border="0.5pt solid #d1d5db" fo:padding="0.03in"/><style:text-properties fo:font-size="10pt"/></style:style>
<style:style style:name="HeaderCell" style:family="table-cell"><style:table-cell-properties fo:background-color="${HEADER_COLOR}" fo:border="0.5pt solid #111827" fo:padding="0.04in"/><style:text-properties fo:color="#ffffff" fo:font-weight="bold" fo:font-size="10pt"/></style:style>
<style:style style:name="OwnCell" style:family="table-cell"><style:table-cell-properties fo:background-color="${OWN_COLOR}" fo:border="0.5pt solid #d6b656" fo:padding="0.03in"/><style:text-properties fo:font-size="10pt"/></style:style>
<style:style style:name="OpponentCell" style:family="table-cell"><style:table-cell-properties fo:background-color="${OPPONENT_COLOR}" fo:border="0.5pt solid #6ea8fe" fo:padding="0.03in"/><style:text-properties fo:font-size="10pt"/></style:style>
</office:automatic-styles>
<office:body>
<office:spreadsheet>
${table("Summary", summaryRows(session))}
${table("Results", resultRows(session))}
${table("Attendance", attendanceWorkbookRows(session).map(item => row(item.values, item.header ? "HeaderCell" : "Cell")).join("\n"))}
</office:spreadsheet>
</office:body>
</office:document>`;
}

function previewTextValue(value, maxLength = 28) {
    const text = String(value ?? "")
        .replace(/\s+/g, " ")
        .trim();
    if (text.length <= maxLength) return text;
    return `${text.slice(0, Math.max(0, maxLength - 3))}...`;
}

function headerStatPill(x, y, item) {
    const width = item.width || 110;
    const label = previewTextValue(item.label, 12).toUpperCase();
    const value = previewTextValue(item.value, item.maxLength || 18);
    const labelWidth = Math.max(42, Math.min(76, label.length * 7 + 14));

    return `<g>
<rect x="${x}" y="${y}" width="${width}" height="24" fill="rgba(255,255,255,0.14)" stroke="rgba(255,255,255,0.24)" rx="12"/>
<text x="${x + 12}" y="${y + 16}" font-family="Arial" font-size="10" font-weight="700" fill="#d1fae5">${escapeXml(label)}</text>
<text x="${x + labelWidth}" y="${y + 17}" font-family="Arial" font-size="13" font-weight="700" fill="#ffffff">${escapeXml(value)}</text>
</g>`;
}

function headerStatPills(items, startX, y, gap = 8) {
    let x = startX;
    return items.map(item => {
        const svg = headerStatPill(x, y, item);
        x += (item.width || 110) + gap;
        return svg;
    }).join("\n");
}

function buildChartSvg(session) {
    const summary = eventSummary(session);
    const buckets = session.stats?.buckets || [];
    const chartMatchup = `${summary.teamName} vs ${summary.opponentTeams.join(", ")}`;
    const chartHeaderPills = headerStatPills([
        { label: "Drivers", value: summary.ownPlayers.length, width: 92 },
        { label: "KAB", value: summary.kabCount, width: 70 }
    ], 970, 25);
    const metricGroups = [
        {
            title: "Score",
            y: 126,
            max: Math.max(1, summary.ownScore || 0, summary.opponentScore || 0),
            rows: [
                { label: "Own score", value: summary.ownScore || 0, color: "#0f766e" },
                { label: "Enemy score", value: summary.opponentScore || 0, color: "#2563eb" }
            ]
        },
        {
            title: "Event points",
            y: 252,
            max: Math.max(1, summary.ownPoints || 0, summary.opponentPoints || 0),
            rows: [
                { label: "Own event pts", value: summary.ownPoints || 0, color: "#d97706" },
                { label: "Enemy event pts", value: summary.opponentPoints || 0, color: "#7c3aed" }
            ]
        }
    ];
    const width = 1180;
    const height = 670;
    const chartX = 230;
    const chartWidth = 720;
    const rowHeight = 42;
    const barRows = metricGroups.map(group => {
        const rows = group.rows.map((item, index) => {
            const y = group.y + index * rowHeight;
            const rawWidth = Math.round((item.value / group.max) * chartWidth);
            const barWidth = item.value > 0 ? Math.max(6, rawWidth) : 0;
            const valueX = Math.min(chartX + barWidth + 12, width - 116);
            return [
                `<text x="52" y="${y + 23}" font-family="Arial" font-size="18" fill="#111827">${escapeXml(item.label)}</text>`,
                `<rect x="${chartX}" y="${y}" width="${chartWidth}" height="30" fill="#eef2ff" rx="7"/>`,
                `<rect x="${chartX}" y="${y}" width="${barWidth}" height="30" fill="${item.color}" rx="7"/>`,
                `<text x="${valueX}" y="${y + 22}" font-family="Arial" font-size="17" font-weight="700" fill="#111827">${item.value}</text>`
            ].join("");
        }).join("\n");
        return [
            `<text x="36" y="${group.y - 18}" font-family="Arial" font-size="20" font-weight="700" fill="#111827">${escapeXml(group.title)}</text>`,
            rows
        ].join("");
    }).join("\n");
    const bucketRows = buckets.map((bucket, index) => {
        const x = 150 + index * 260;
        const bucketMax = Math.max(1, bucket.own || 0, bucket.opponents || 0);
        const ownHeight = Math.round(((bucket.own || 0) / bucketMax) * 132);
        const opponentHeight = Math.round(((bucket.opponents || 0) / bucketMax) * 132);
        const baseline = 594;
        return [
            `<rect x="${x - 70}" y="444" width="140" height="174" fill="#f8fafc" stroke="#e2e8f0" rx="8"/>`,
            `<text x="${x}" y="636" font-family="Arial" font-size="17" text-anchor="middle" fill="#111827">${escapeXml(bucket.label)}</text>`,
            `<rect x="${x - 45}" y="${baseline - ownHeight}" width="40" height="${ownHeight}" fill="#f59e0b" rx="5"/>`,
            `<rect x="${x + 6}" y="${baseline - opponentHeight}" width="40" height="${opponentHeight}" fill="#2563eb" rx="5"/>`,
            `<text x="${x - 25}" y="${baseline - ownHeight - 8}" font-family="Arial" font-size="14" font-weight="700" text-anchor="middle" fill="#111827">${bucket.own || 0}</text>`,
            `<text x="${x + 26}" y="${baseline - opponentHeight - 8}" font-family="Arial" font-size="14" font-weight="700" text-anchor="middle" fill="#111827">${bucket.opponents || 0}</text>`
        ].join("");
    }).join("\n");

    return `<svg xmlns="http://www.w3.org/2000/svg" width="${width}" height="${height}" viewBox="0 0 ${width} ${height}">
<defs>
<linearGradient id="chartTitle" x1="0" y1="0" x2="1" y2="0"><stop offset="0%" stop-color="#0f766e"/><stop offset="50%" stop-color="#2563eb"/><stop offset="100%" stop-color="#7c3aed"/></linearGradient>
</defs>
<rect width="100%" height="100%" fill="#f8fafc"/>
<rect x="0" y="0" width="${width}" height="76" fill="url(#chartTitle)"/>
<text x="34" y="31" font-family="Arial" font-size="24" font-weight="700" fill="#ffffff">${escapeXml(previewTextValue(summary.eventName, 50))}</text>
<text x="34" y="58" font-family="Arial" font-size="16" font-weight="600" fill="#d1fae5">${escapeXml(previewTextValue(chartMatchup, 72))}</text>
${chartHeaderPills}
${barRows}
<text x="36" y="396" font-family="Arial" font-size="22" font-weight="700" fill="#111827">Placement buckets</text>
<line x1="70" y1="594" x2="1070" y2="594" stroke="#cbd5e1"/>
${bucketRows}
</svg>`;
}

function buildSpreadsheetImageSvg(session) {
    const summary = eventSummary(session);
    const teamPointText = (summary.ownTeamScore || summary.opponentTeamScore)
        ? `${summary.ownTeamScore || 0}-${summary.opponentTeamScore || 0}`
        : "n/a";
    const scoreText = `${summary.ownScore || 0}-${summary.opponentScore || 0}`;
    const versusText = `${summary.teamName} vs ${summary.opponentTeams.join(", ")}`;
    const tableHeaderPills = headerStatPills([
        { label: "Drivers", value: summary.ownPlayers.length, width: 94 },
        { label: "KAB", value: summary.kabCount, width: 72 },
        { label: "Team pts", value: teamPointText, width: 154 },
        { label: "Score", value: scoreText, width: 188 }
    ], 28, 84);
    const rankFill = item => {
        const rank = Number(item.values[0]);
        if (rank === 1) return "#fde68a";
        if (rank <= 3) return "#e0f2fe";
        if (rank <= 10) return "#dcfce7";
        return "";
    };
    const columns = [
        { label: "Rank", width: 70, value: item => item.values[0], align: "center", fill: rankFill },
        { label: "Player", width: 280, value: item => item.values[1], maxLength: 34 },
        { label: "Pts", width: 95, value: item => item.values[4], align: "center", fill: () => "#dcfce7" },
        { label: "Score", width: 135, value: item => item.values[5], align: "center", fill: () => "#ede9fe" },
        { label: "Blues", width: 95, value: item => item.values[6], align: "center", fill: () => "#dbeafe" },
        { label: "Blue %", width: 95, value: item => item.values[7], align: "center", fill: () => "#ccfbf1" },
        {
            label: "#KAB",
            width: 85,
            value: item => item.values[8],
            align: "center",
            fill: item => Number(item.values[8]) > 0 ? KAB_COLOR : "#fce7f3",
            textColor: item => Number(item.values[8]) > 0 ? "#ffffff" : "#111827"
        }
    ];
    const rows = playerDisplayRows(session).filter(item => item.own);
    const rowHeight = 34;
    const margin = 28;
    const titleHeight = 156;
    const tableWidth = columns.reduce((total, column) => total + column.width, 0);
    const width = tableWidth + margin * 2;
    const height = titleHeight + rowHeight * (Math.max(rows.length, 1) + 1) + 62;

    let x = margin;
    const headerCells = columns.map((column, index) => {
        const headerFill = IMAGE_HEADER_COLORS[index % IMAGE_HEADER_COLORS.length];
        const textAnchor = column.align === "center" ? "middle" : "start";
        const textX = column.align === "center" ? x + column.width / 2 : x + 10;
        const cellSvg = `<rect x="${x}" y="${titleHeight}" width="${column.width}" height="${rowHeight}" fill="${headerFill}" stroke="#111827"/>
<text x="${textX}" y="${titleHeight + 23}" font-family="Arial" font-size="14" font-weight="700" text-anchor="${textAnchor}" fill="#ffffff">${escapeXml(column.label)}</text>`;
        x += column.width;
        return cellSvg;
    }).join("\n");

    const bodyRows = rows.length ? rows.map((item, rowIndex) => {
        const y = titleHeight + rowHeight * (rowIndex + 1);
        const baseFill = Number(item.values[8]) > 0
            ? "#fef3c7"
            : rowIndex % 2 === 0 ? "#fffbeb" : "#f8fafc";
        let cellX = margin;
        const cells = columns.map(column => {
            const text = previewTextValue(column.value(item), column.maxLength || (column.width > 180 ? 30 : 16));
            const fill = column.fill ? column.fill(item) || baseFill : baseFill;
            const textColor = column.textColor ? column.textColor(item) : "#111827";
            const textAnchor = column.align === "center" ? "middle" : "start";
            const textX = column.align === "center" ? cellX + column.width / 2 : cellX + 10;
            const cellSvg = `<rect x="${cellX}" y="${y}" width="${column.width}" height="${rowHeight}" fill="${fill}" stroke="#d1d5db"/>
<text x="${textX}" y="${y + 22}" font-family="Arial" font-size="13" font-weight="${column.align === "center" ? "700" : "400"}" text-anchor="${textAnchor}" fill="${textColor}">${escapeXml(text)}</text>`;
            cellX += column.width;
            return cellSvg;
        }).join("\n");
        return cells;
    }).join("\n") : `<rect x="${margin}" y="${titleHeight + rowHeight}" width="${tableWidth}" height="${rowHeight}" fill="#f8fafc" stroke="#d1d5db"/>
<text x="${margin + 12}" y="${titleHeight + rowHeight + 22}" font-family="Arial" font-size="13" fill="#64748b">No own drivers found</text>`;

    return `<svg xmlns="http://www.w3.org/2000/svg" width="${width}" height="${height}" viewBox="0 0 ${width} ${height}">
<defs>
<linearGradient id="tableTitle" x1="0" y1="0" x2="1" y2="0"><stop offset="0%" stop-color="#0f766e"/><stop offset="55%" stop-color="#2563eb"/><stop offset="100%" stop-color="#d97706"/></linearGradient>
</defs>
<rect width="100%" height="100%" fill="#f8fafc"/>
<rect x="0" y="0" width="${width}" height="118" fill="url(#tableTitle)"/>
<text x="${margin}" y="35" font-family="Arial" font-size="25" font-weight="700" fill="#ffffff">${escapeXml(previewTextValue(summary.eventName, 54))}</text>
<text x="${margin}" y="66" font-family="Arial" font-size="16" font-weight="600" fill="#d1fae5">${escapeXml(previewTextValue(versusText, 62))}</text>
${tableHeaderPills}
${headerCells}
${bodyRows}
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

async function generateChartImage(session, outputDir, baseName) {
    return saveSvgAsPng(buildChartSvg(session), path.join(outputDir, `${baseName}-summary-chart.png`));
}

async function generateSpreadsheetImage(session, outputDir, baseName) {
    return saveSvgAsPng(buildSpreadsheetImageSvg(session), path.join(outputDir, `${baseName}-spreadsheet.png`));
}

async function convertWithLibreOffice(fodsPath, outputDir, settings = {}) {
    const binary = String(settings.libreOfficePath || process.env.LIBREOFFICE_PATH || "soffice").trim() || "soffice";
    await runCommand(binary, [
        "--headless",
        "--convert-to",
        "xlsx",
        "--outdir",
        outputDir,
        fodsPath
    ], { timeoutMs: 120000 });
    const converted = path.join(outputDir, `${path.basename(fodsPath, path.extname(fodsPath))}.xlsx`);
    if (!fs.existsSync(converted)) {
        throw new Error("LibreOffice did not create the expected XLSX file.");
    }
    return converted;
}

function excelColor(hex) {
    return `FF${String(hex || "#ffffff").replace("#", "").toUpperCase()}`;
}

function styleExcelRow(row, options = {}) {
    row.eachCell({ includeEmpty: true }, (cell, colNumber) => {
        cell.border = {
            top: { style: "thin", color: { argb: "FFD1D5DB" } },
            left: { style: "thin", color: { argb: "FFD1D5DB" } },
            bottom: { style: "thin", color: { argb: "FFD1D5DB" } },
            right: { style: "thin", color: { argb: "FFD1D5DB" } }
        };
        cell.alignment = { vertical: "middle", horizontal: colNumber === 2 ? "left" : "center", wrapText: true };
        cell.font = { name: "Aptos", size: 11, color: { argb: "FF111827" } };

        if (options.header) {
            cell.fill = {
                type: "pattern",
                pattern: "solid",
                fgColor: { argb: excelColor(HEADER_COLOR) }
            };
            cell.font = { name: "Aptos", bold: true, color: { argb: "FFFFFFFF" } };
        } else if (options.fill) {
            cell.fill = {
                type: "pattern",
                pattern: "solid",
                fgColor: { argb: excelColor(options.fill) }
            };
        }
    });

    if (options.kab) {
        const cell = row.getCell(row.cellCount);
        cell.fill = { type: "pattern", pattern: "solid", fgColor: { argb: excelColor(KAB_COLOR) } };
        cell.font = { name: "Aptos", bold: true, color: { argb: "FFFFFFFF" } };
    }
}

function addWorksheetRows(worksheet, rows) {
    for (const item of rows) {
        const rowRef = worksheet.addRow(item.values);
        styleExcelRow(rowRef, item);
    }

    worksheet.views = [{ state: "frozen", ySplit: 1 }];
    worksheet.columns.forEach((column, index) => {
        let width = index === 1 ? 22 : 12;
        column.eachCell({ includeEmpty: true }, cellRef => {
            const text = String(cellRef.value ?? "");
            width = Math.max(width, Math.min(index === 1 ? 34 : 22, text.length + 2));
        });
        column.width = width;
    });
}

async function generateXlsxWithExcelJs(session, outputDir, baseName) {
    const ExcelJS = require("exceljs");
    const workbook = new ExcelJS.Workbook();
    workbook.creator = "DCA Bot";
    workbook.created = new Date();
    workbook.modified = new Date();

    addWorksheetRows(workbook.addWorksheet("Summary"), summaryWorkbookRows(session));
    addWorksheetRows(workbook.addWorksheet("Results"), resultWorkbookRows(session));
    addWorksheetRows(workbook.addWorksheet("Attendance"), attendanceWorkbookRows(session));

    const xlsxPath = path.join(outputDir, `${baseName}.xlsx`);
    await workbook.xlsx.writeFile(xlsxPath);
    if (!fs.existsSync(xlsxPath)) {
        throw new Error("ExcelJS did not create the expected XLSX file.");
    }
    return xlsxPath;
}

async function generateSpreadsheetArtifacts(session, outputDir, settings = {}) {
    await fs.promises.mkdir(outputDir, { recursive: true });
    const baseName = `${safeFileName(session.teamId || "team")}-${safeFileName(session.id || "session")}`;
    const fodsPath = path.join(outputDir, `${baseName}.fods`);
    const chartPath = await generateChartImage(session, outputDir, baseName);
    const spreadsheetImagePath = await generateSpreadsheetImage(session, outputDir, baseName);

    await fs.promises.writeFile(fodsPath, buildFods(session), "utf8");

    let spreadsheetPath = fodsPath;
    let conversionError = "";
    if ((settings.outputFormat || "xlsx") === "xlsx") {
        try {
            spreadsheetPath = await generateXlsxWithExcelJs(session, outputDir, baseName);
        } catch (error) {
            try {
                spreadsheetPath = await convertWithLibreOffice(fodsPath, outputDir, settings);
                conversionError = `ExcelJS fallback used LibreOffice after direct XLSX failed: ${error.message}`;
            } catch (fallbackError) {
                conversionError = `${error.message}; LibreOffice fallback failed: ${fallbackError.message}`;
            }
        }
    }

    return {
        fodsPath,
        spreadsheetPath,
        spreadsheetImagePath,
        chartPath,
        conversionError,
        generatedAt: new Date().toISOString()
    };
}

module.exports = {
    generateSpreadsheetArtifacts
};
