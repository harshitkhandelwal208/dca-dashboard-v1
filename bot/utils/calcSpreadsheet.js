const fs = require("fs");
const path = require("path");
const { runCommand } = require("./raceOcr");

const OWN_COLOR = "#fff2cc";
const OPPONENT_COLOR = "#cfe2ff";
const HEADER_COLOR = "#1f2937";

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

function sessionKabMap(session) {
    const players = session.players || [];
    const own = players.filter(player => player.teamType === "own");
    const opponentRanks = players
        .filter(player => player.teamType !== "own")
        .map(player => Number(player.rank))
        .filter(rank => Number.isFinite(rank));
    const topOpponentRank = opponentRanks.length ? Math.min(...opponentRanks) : null;
    const maxScore = Math.max(0, ...players.map(player => Number(player.points ?? player.score ?? 0) || 0));
    const map = new Map();

    for (const player of own) {
        const rank = Number(player.rank);
        const score = Number(player.points ?? player.score ?? 0) || 0;
        const kab = topOpponentRank !== null
            ? Number.isFinite(rank) && rank < topOpponentRank
            : score >= maxScore && maxScore > 0;
        map.set(player.playerName, kab ? 1 : 0);
    }

    return map;
}

function summaryRows(session) {
    const stats = session.stats || {};
    const metadata = session.metadata || {};
    const teamScores = metadata.teamScores || {};
    const kabMap = sessionKabMap(session);
    const rows = [
        row(["Metric", "Value"], "HeaderCell"),
        row(["Session", metadata.title || session.id]),
        row(["Team", metadata.ownTeamName || session.teamName || session.teamId]),
        row(["Submission ID", session.id]),
        row(["Images", (session.images || []).length]),
        row(["Total players parsed", stats.totalPlayers || 0]),
        row(["Own team players", stats.ownPlayers || 0]),
        row(["Opponents", stats.opponents || 0]),
        row(["Own average rank", stats.ownAverageRank ?? ""]),
        row(["Opponent average rank", stats.opponentAverageRank ?? ""]),
        row(["Own points", stats.ownPoints || 0]),
        row(["Opponent points", stats.opponentPoints || 0]),
        row(["Own score", stats.ownScore || 0]),
        row(["Opponent score", stats.opponentScore || 0]),
        row(["#KAB this event", [...kabMap.values()].reduce((total, value) => total + value, 0)]),
        row(["#KAB definition", "Player ranked above every opponent in this event."])
    ];

    if (teamScores.rawLine) {
        rows.push(row(["Team score line", teamScores.rawLine]));
        rows.push(row(["Detected own team score", teamScores.own ?? ""]));
        rows.push(row(["Detected opponent score", teamScores.opponent ?? ""]));
    }

    rows.push(row(["", ""]));
    rows.push(row(["Podium", "Player", "Team", "Points", "Score"], "HeaderCell"));
    for (const player of stats.podium || []) {
        rows.push(row([player.rank, player.playerName, player.teamLabel, player.points ?? "", player.score ?? ""]));
    }

    rows.push(row(["", ""]));
    rows.push(row(["Own Player", "Rank", "Opponents Below"], "HeaderCell"));
    for (const item of stats.opponentsBelowByPlayer || []) {
        rows.push(row([item.playerName, item.rank, item.opponentsBelow]));
    }

    return rows.join("\n");
}

function resultRows(session) {
    const headers = [
        "Rank",
        "Player",
        "Team",
        "Color",
        "Event Points",
        "Score",
        "Opponents Below",
        "#KAB",
        "Source",
        "Raw OCR Line"
    ];
    const stats = session.stats || {};
    const belowMap = new Map((stats.opponentsBelowByPlayer || []).map(item => [item.playerName, item.opponentsBelow]));
    const kabMap = sessionKabMap(session);
    const rows = [row(headers, "HeaderCell")];

    for (const player of session.players || []) {
        const style = player.teamType === "own" ? "OwnCell" : "OpponentCell";
        rows.push(`<table:table-row>${
            [
                cell(player.rank, style),
                cell(player.playerName, style),
                cell(player.teamLabel, style),
                cell(player.teamColor === "yellow" ? "Yellow - own team" : "Blue - opponent", style),
                cell(player.points ?? "", style),
                cell(player.score ?? "", style),
                cell(belowMap.get(player.playerName) ?? "", style),
                cell(kabMap.get(player.playerName) || 0, style),
                cell(player.sourceImage || "", style),
                cell(player.rawLine || "", style)
            ].join("")
        }</table:table-row>`);
    }

    return rows.join("\n");
}

function chartRows(session) {
    const stats = session.stats || {};
    const maxPoints = Math.max(1, stats.ownPoints || 0, stats.opponentPoints || 0);
    const bar = value => "#".repeat(Math.max(1, Math.round((Number(value || 0) / maxPoints) * 30)));
    const rows = [
        row(["Score Comparison", "Value", "Bar"], "HeaderCell"),
        row(["Own points", stats.ownPoints || 0, bar(stats.ownPoints)]),
        row(["Opponent points", stats.opponentPoints || 0, bar(stats.opponentPoints)]),
        row(["Own score", stats.ownScore || 0, bar(stats.ownScore)]),
        row(["Opponent score", stats.opponentScore || 0, bar(stats.opponentScore)]),
        row(["", ""]),
        row(["Placement Bucket", "Own", "Opponents"], "HeaderCell")
    ];

    for (const bucket of stats.buckets || []) {
        rows.push(row([bucket.label, bucket.own, bucket.opponents]));
    }

    return rows.join("\n");
}

function rawRows(session) {
    return [
        row(["Image", "OCR Text"], "HeaderCell"),
        ...(session.ocrResults || []).map((result, index) =>
            row([`Image ${index + 1}`, result.text || ""])
        )
    ].join("\n");
}

function summaryWorkbookRows(session) {
    const stats = session.stats || {};
    const metadata = session.metadata || {};
    const teamScores = metadata.teamScores || {};
    const kabMap = sessionKabMap(session);
    const rows = [
        { values: ["Metric", "Value"], header: true },
        { values: ["Session", metadata.title || session.id] },
        { values: ["Team", metadata.ownTeamName || session.teamName || session.teamId] },
        { values: ["Submission ID", session.id] },
        { values: ["Images", (session.images || []).length] },
        { values: ["Total players parsed", stats.totalPlayers || 0] },
        { values: ["Own team players", stats.ownPlayers || 0] },
        { values: ["Opponents", stats.opponents || 0] },
        { values: ["Own average rank", stats.ownAverageRank ?? ""] },
        { values: ["Opponent average rank", stats.opponentAverageRank ?? ""] },
        { values: ["Own points", stats.ownPoints || 0] },
        { values: ["Opponent points", stats.opponentPoints || 0] },
        { values: ["Own score", stats.ownScore || 0] },
        { values: ["Opponent score", stats.opponentScore || 0] },
        { values: ["#KAB this event", [...kabMap.values()].reduce((total, value) => total + value, 0)] },
        { values: ["#KAB definition", "Player ranked above every opponent in this event."] }
    ];

    if (teamScores.rawLine) {
        rows.push({ values: ["Team score line", teamScores.rawLine] });
        rows.push({ values: ["Detected own team score", teamScores.own ?? ""] });
        rows.push({ values: ["Detected opponent score", teamScores.opponent ?? ""] });
    }

    rows.push({ values: ["", ""] });
    rows.push({ values: ["Podium", "Player", "Team", "Points", "Score"], header: true });
    for (const player of stats.podium || []) {
        rows.push({ values: [player.rank, player.playerName, player.teamLabel, player.points ?? "", player.score ?? ""] });
    }

    rows.push({ values: ["", ""] });
    rows.push({ values: ["Own Player", "Rank", "Opponents Below"], header: true });
    for (const item of stats.opponentsBelowByPlayer || []) {
        rows.push({ values: [item.playerName, item.rank, item.opponentsBelow] });
    }

    return rows;
}

function resultWorkbookRows(session) {
    const headers = [
        "Rank",
        "Player",
        "Team",
        "Color",
        "Event Points",
        "Score",
        "Opponents Below",
        "#KAB",
        "Source",
        "Raw OCR Line"
    ];
    const stats = session.stats || {};
    const belowMap = new Map((stats.opponentsBelowByPlayer || []).map(item => [item.playerName, item.opponentsBelow]));
    const kabMap = sessionKabMap(session);
    const rows = [{ values: headers, header: true }];

    for (const player of session.players || []) {
        rows.push({
            values: [
                player.rank,
                player.playerName,
                player.teamLabel,
                player.teamColor === "yellow" ? "Yellow - own team" : "Blue - opponent",
                player.points ?? "",
                player.score ?? "",
                belowMap.get(player.playerName) ?? "",
                kabMap.get(player.playerName) || 0,
                player.sourceImage || "",
                player.rawLine || ""
            ],
            fill: player.teamType === "own" ? OWN_COLOR : OPPONENT_COLOR
        });
    }

    return rows;
}

function chartWorkbookRows(session) {
    const stats = session.stats || {};
    const maxPoints = Math.max(1, stats.ownPoints || 0, stats.opponentPoints || 0);
    const bar = value => "#".repeat(Math.max(1, Math.round((Number(value || 0) / maxPoints) * 30)));
    const rows = [
        { values: ["Score Comparison", "Value", "Bar"], header: true },
        { values: ["Own points", stats.ownPoints || 0, bar(stats.ownPoints)] },
        { values: ["Opponent points", stats.opponentPoints || 0, bar(stats.opponentPoints)] },
        { values: ["Own score", stats.ownScore || 0, bar(stats.ownScore)] },
        { values: ["Opponent score", stats.opponentScore || 0, bar(stats.opponentScore)] },
        { values: ["", ""] },
        { values: ["Placement Bucket", "Own", "Opponents"], header: true }
    ];

    for (const bucket of stats.buckets || []) {
        rows.push({ values: [bucket.label, bucket.own, bucket.opponents] });
    }

    return rows;
}

function rawWorkbookRows(session) {
    return [
        { values: ["Image", "OCR Text"], header: true },
        ...(session.ocrResults || []).map((result, index) => ({
            values: [`Image ${index + 1}`, result.text || ""]
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
${table("Charts", chartRows(session))}
${table("Raw OCR", rawRows(session))}
</office:spreadsheet>
</office:body>
</office:document>`;
}

function buildSvgChart(session) {
    const stats = session.stats || {};
    const values = [
        { label: "Own pts", value: stats.ownPoints || 0, color: "#d9a300" },
        { label: "Opp pts", value: stats.opponentPoints || 0, color: "#3b82f6" },
        { label: "Own score", value: stats.ownScore || 0, color: "#f59e0b" },
        { label: "Opp score", value: stats.opponentScore || 0, color: "#2563eb" }
    ];
    const max = Math.max(1, ...values.map(item => item.value));
    const width = 900;
    const height = 360;
    const chartX = 160;
    const chartWidth = 660;
    const rowHeight = 60;
    const bars = values.map((item, index) => {
        const y = 58 + index * rowHeight;
        const barWidth = Math.round((item.value / max) * chartWidth);
        return [
            `<text x="24" y="${y + 25}" font-family="Arial" font-size="20" fill="#111827">${escapeXml(item.label)}</text>`,
            `<rect x="${chartX}" y="${y}" width="${barWidth}" height="34" fill="${item.color}" rx="4"/>`,
            `<text x="${chartX + barWidth + 12}" y="${y + 25}" font-family="Arial" font-size="18" fill="#111827">${item.value}</text>`
        ].join("");
    }).join("");

    return `<svg xmlns="http://www.w3.org/2000/svg" width="${width}" height="${height}" viewBox="0 0 ${width} ${height}">
<rect width="100%" height="100%" fill="#ffffff"/>
<text x="24" y="32" font-family="Arial" font-size="24" font-weight="700" fill="#111827">${escapeXml(session.metadata?.title || "Race Summary")}</text>
${bars}
<text x="24" y="330" font-family="Arial" font-size="16" fill="#6b7280">Yellow = own team, blue = opponents. Generated from OCR and corrections.</text>
</svg>`;
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
    row.eachCell({ includeEmpty: true }, cell => {
        cell.border = {
            top: { style: "thin", color: { argb: "FFD1D5DB" } },
            left: { style: "thin", color: { argb: "FFD1D5DB" } },
            bottom: { style: "thin", color: { argb: "FFD1D5DB" } },
            right: { style: "thin", color: { argb: "FFD1D5DB" } }
        };
        cell.alignment = { vertical: "top", wrapText: true };

        if (options.header) {
            cell.fill = {
                type: "pattern",
                pattern: "solid",
                fgColor: { argb: excelColor(HEADER_COLOR) }
            };
            cell.font = { bold: true, color: { argb: "FFFFFFFF" } };
        } else if (options.fill) {
            cell.fill = {
                type: "pattern",
                pattern: "solid",
                fgColor: { argb: excelColor(options.fill) }
            };
        }
    });
}

function addWorksheetRows(worksheet, rows) {
    for (const item of rows) {
        const rowRef = worksheet.addRow(item.values);
        styleExcelRow(rowRef, item);
    }

    worksheet.views = [{ state: "frozen", ySplit: 1 }];
    worksheet.columns.forEach(column => {
        let width = 12;
        column.eachCell({ includeEmpty: true }, cell => {
            const text = String(cell.value ?? "");
            width = Math.max(width, Math.min(48, text.length + 2));
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
    addWorksheetRows(workbook.addWorksheet("Charts"), chartWorkbookRows(session));
    addWorksheetRows(workbook.addWorksheet("Raw OCR"), rawWorkbookRows(session));

    const xlsxPath = path.join(outputDir, `${baseName}.xlsx`);
    await workbook.xlsx.writeFile(xlsxPath);
    if (!fs.existsSync(xlsxPath)) {
        throw new Error("ExcelJS did not create the expected XLSX file.");
    }
    return xlsxPath;
}

async function generateSpreadsheetArtifacts(session, outputDir, settings = {}) {
    await fs.promises.mkdir(outputDir, { recursive: true });
    const baseName = `${session.teamId || "team"}-${session.id}`;
    const fodsPath = path.join(outputDir, `${baseName}.fods`);
    const chartPath = path.join(outputDir, `${baseName}-summary.svg`);

    await fs.promises.writeFile(fodsPath, buildFods(session), "utf8");
    await fs.promises.writeFile(chartPath, buildSvgChart(session), "utf8");

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
        chartPath,
        conversionError,
        generatedAt: new Date().toISOString()
    };
}

module.exports = {
    generateSpreadsheetArtifacts
};
