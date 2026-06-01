const assert = require("assert");
const fs = require("fs");
const path = require("path");
const ExcelJS = require("exceljs");
const { generateSpreadsheetArtifacts } = require("../utils/calcSpreadsheet");
const { detectRaceRowColors, parseRaceScreenshots, prepareImageForGemini } = require("../utils/raceOcr");
const { classifyRecruitmentVisibleText } = require("../utils/recruitmentOcr");
const { generatePeriodReport } = require("../utils/spreadsheetReports");
const { cleanTeamName, sessionOpponentTeams } = require("../utils/spreadsheetMetrics");

async function workbookSheets(filePath) {
    const workbook = new ExcelJS.Workbook();
    await workbook.xlsx.readFile(filePath);
    return workbook.worksheets.map(sheet => sheet.name);
}

async function writeRowColorFixture(filePath) {
    const sharp = require("sharp");
    const rows = [
        { fill: "#eadf8b", flag: "#2563eb" },
        { fill: "#3e9bed", flag: "#facc15" },
        { fill: "#ddd37e", flag: "#dc2626" },
        { fill: "#4aa3f1", flag: "#16a34a" }
    ];
    const rowHeight = 52;
    const svgRows = rows.map((row, index) => {
        const y = index * rowHeight;
        return [
            `<rect x="0" y="${y}" width="700" height="44" fill="${row.fill}"/>`,
            `<rect x="172" y="${y + 8}" width="62" height="28" fill="${row.flag}" transform="rotate(-10 203 ${y + 22})"/>`,
            `<rect x="584" y="${y + 8}" width="94" height="28" fill="rgba(0,0,0,0.25)"/>`,
            `<rect x="0" y="${y + 44}" width="700" height="8" fill="#66737a"/>`
        ].join("");
    }).join("");
    const svg = `<svg xmlns="http://www.w3.org/2000/svg" width="700" height="${rows.length * rowHeight}">${svgRows}</svg>`;
    await sharp(Buffer.from(svg)).png().toFile(filePath);
}

async function writeUncroppedStandingsFixture(filePath) {
    const sharp = require("sharp");
    const width = 2392;
    const height = 1080;
    const rows = ["#4aa3f1", "#eadf8b", "#ddd37e", "#eadf8b", "#3e9bed", "#4aa3f1", "#3e9bed"];
    const rowHeight = 72;
    const rowGap = 14;
    const rowY = 310;
    const tableX = 400;
    const tableWidth = 1100;
    const rowRects = rows.map((fill, index) => {
        const y = rowY + index * (rowHeight + rowGap);
        return [
            `<rect x="${tableX}" y="${y}" width="${tableWidth}" height="${rowHeight}" fill="${fill}"/>`,
            `<rect x="${tableX + 820}" y="${y + 14}" width="100" height="34" fill="rgba(0,0,0,0.22)"/>`,
            `<text x="${tableX + 140}" y="${y + 45}" font-family="Arial" font-size="34" font-weight="700" fill="#111">Driver ${index + 1}</text>`
        ].join("");
    }).join("");
    const rewards = [260, 450, 640].map(y =>
        `<rect x="1550" y="${y}" width="350" height="130" rx="8" fill="#f2d36a"/>`
    ).join("");
    const svg = [
        `<svg xmlns="http://www.w3.org/2000/svg" width="${width}" height="${height}">`,
        `<rect width="${width}" height="${height}" fill="#3a2a50"/>`,
        `<rect x="0" y="0" width="260" height="${height}" fill="#201735"/>`,
        `<rect x="${tableX}" y="210" width="${tableWidth}" height="76" fill="#5a4630"/>`,
        rewards,
        rowRects,
        `<rect x="1010" y="960" width="390" height="90" fill="#4caf35"/>`,
        `</svg>`
    ].join("");
    await sharp(Buffer.from(svg)).jpeg().toFile(filePath);
}

async function main() {
    const stamp = new Date().toISOString().replace(/[-:.TZ]/g, "").slice(0, 14);
    const outputDir = path.join(__dirname, "..", "data", "spreadsheets", "_test", stamp);
    await fs.promises.mkdir(outputDir, { recursive: true });

    const rowColorFixture = path.join(outputDir, "row-colors.png");
    await writeRowColorFixture(rowColorFixture);
    const rowColorHints = await detectRaceRowColors(rowColorFixture);
    assert.deepStrictEqual(
        rowColorHints.map(row => row.color),
        ["yellow", "blue", "yellow", "blue"],
        "row color detection should sample the clean right-middle background, not flag colors"
    );

    const colorParsed = parseRaceScreenshots([{
        imagePath: "color-fixture.png",
        text: "",
        rowColorHints: [
            { rowIndex: 1, color: "yellow", confidence: 0.9 },
            { rowIndex: 2, color: "blue", confidence: 0.9 }
        ],
        structured: {
            eventName: "Color Fixture",
            players: [
                { rank: 10, playerName: "FlagMadeMeBlue", teamType: "opponent", points: 1, score: 100 },
                { rank: 11, playerName: "DC|FlagMadeMeYellow", teamLabel: "Discord", teamType: "own", points: 1, score: 99 }
            ]
        }
    }], { name: "Discord", ownTeamAliases: [], ownPlayerAliases: [] });
    assert.strictEqual(colorParsed.players.find(row => row.rank === 10).teamType, "own");
    assert.strictEqual(colorParsed.players.find(row => row.rank === 11).teamType, "opponent");

    const uncroppedFixture = path.join(outputDir, "uncropped-standings.jpg");
    await writeUncroppedStandingsFixture(uncroppedFixture);
    const uncroppedHints = await detectRaceRowColors(uncroppedFixture);
    assert.deepStrictEqual(
        uncroppedHints.slice(0, 7).map(row => row.color),
        ["blue", "yellow", "yellow", "yellow", "blue", "blue", "blue"],
        "uncropped full-screen standings should be auto-cropped before row color sampling"
    );
    assert(
        uncroppedHints.every(row => row.imageCrop?.source === "auto-standings-table"),
        "row color hints from uncropped screenshots should record the automatic standings crop"
    );
    const preparedUncropped = await prepareImageForGemini(uncroppedFixture, { imageKind: "team-event-score" });
    const originalMeta = await require("sharp")(uncroppedFixture).metadata();
    const preparedMeta = await require("sharp")(preparedUncropped).metadata();
    assert(
        preparedMeta.width < originalMeta.width && preparedMeta.height <= originalMeta.height,
        "uncropped screenshots should be cropped before Gemini preprocessing"
    );

    assert.strictEqual(
        classifyRecruitmentVisibleText("Garage power 6411 Cup points Legendary S7 Season points Grand Master III Best win streak 288 Achievements").kind,
        "driver-license",
        "driver license screenshots should be recognized from profile labels"
    );
    assert.strictEqual(
        classifyRecruitmentVisibleText("Sky-Rock Samba Final Standings WIN BONUS REWARDS 60. DC|BlackWing 20 555 NEXT").kind,
        "team-event-score",
        "team event score screenshots should be recognized from standings labels"
    );

    const multiImageColorParsed = parseRaceScreenshots([{
        imagePath: "color-page-1.png",
        text: "",
        rowColorHints: [
            { rowIndex: 1, color: "yellow", confidence: 0.9 },
            { rowIndex: 2, color: "blue", confidence: 0.9 }
        ],
        structured: {
            eventName: "Multi Image Color Fixture",
            players: [
                { rank: 1, playerName: "WrongBlue1", teamType: "opponent", points: 4, score: 400, sourceImageIndex: 1 },
                { rank: 2, playerName: "WrongYellow2", teamType: "own", points: 3, score: 300, sourceImageIndex: 1 },
                { rank: 3, playerName: "DC|WrongYellow3", teamType: "own", points: 2, score: 200, sourceImageIndex: 2 },
                { rank: 4, playerName: "WrongBlue4", teamType: "opponent", points: 1, score: 100, sourceImageIndex: 2 }
            ]
        }
    }, {
        imagePath: "color-page-2.png",
        text: "",
        rowColorHints: [
            { rowIndex: 1, color: "blue", confidence: 0.9 },
            { rowIndex: 2, color: "yellow", confidence: 0.9 }
        ]
    }], { name: "Discord", ownTeamAliases: [], ownPlayerAliases: [] });
    assert.deepStrictEqual(
        multiImageColorParsed.players.map(row => row.teamType),
        ["own", "opponent", "opponent", "own"],
        "row color hints should map to the exact screenshot named by sourceImageIndex"
    );

    const unorderedNoSourceImageParsed = parseRaceScreenshots([{
        imagePath: "unordered-page-2.png",
        text: "",
        rowColorHints: [
            { rowIndex: 1, color: "blue", confidence: 0.9 },
            { rowIndex: 2, color: "blue", confidence: 0.9 }
        ],
        structured: {
            eventName: "Unordered Image Fixture",
            players: [
                { rank: 1, playerName: "NoSource1", teamType: "opponent", points: 4, score: 400 },
                { rank: 2, playerName: "NoSource2", teamType: "opponent", points: 3, score: 300 },
                { rank: 3, playerName: "NoSource3", teamType: "own", points: 2, score: 200 },
                { rank: 4, playerName: "NoSource4", teamType: "own", points: 1, score: 100 }
            ]
        }
    }, {
        imagePath: "unordered-page-1.png",
        text: "",
        rowColorHints: [
            { rowIndex: 1, color: "yellow", confidence: 0.9 },
            { rowIndex: 2, color: "yellow", confidence: 0.9 }
        ]
    }], { name: "Discord", ownTeamAliases: [], ownPlayerAliases: [] });
    assert.deepStrictEqual(
        unorderedNoSourceImageParsed.players.map(row => row.teamType),
        ["opponent", "opponent", "own", "own"],
        "multi-image row color hints should not be globally remapped when screenshot order is ambiguous"
    );

    const teamScoreParsed = parseRaceScreenshots([{
        imagePath: "summary.png",
        text: [
            "Airborne Anguish",
            "Discord",
            "1 574",
            "EMPIRE",
            "2 946",
            "WINNER!"
        ].join("\n"),
        structured: {
            eventName: "Airborne Anguish",
            players: []
        }
    }], { name: "Discord", ownTeamAliases: ["Discord"], ownPlayerAliases: [] });
    assert.deepStrictEqual(
        teamScoreParsed.metadata.teamScores,
        { own: 1574, opponent: 2946, rawLine: "Discord 1 574 vs EMPIRE 2 946" },
        "team-event point totals should be recovered from podium/summary visible text"
    );
    assert.strictEqual(cleanTeamName("Discord 1 574 2 946 EMPIRE", "Discord"), "EMPIRE");
    assert.deepStrictEqual(
        sessionOpponentTeams({
            teamName: "Discord",
            metadata: {
                teamScores: { rawLine: "Discord 1 574 2 946 EMPIRE" },
                teams: [{ label: "Discord 1 574 2 946 EMPIRE", teamType: "opponent" }]
            },
            players: []
        }),
        ["EMPIRE"],
        "opponent team labels should not include score-line OCR fragments"
    );

    const rankPointParsed = parseRaceScreenshots([{
        imagePath: "rank-points.png",
        text: "",
        structured: {
            eventName: "Rank Points Fixture",
            players: [
                { rank: 1, playerName: "First", teamType: "opponent", points: 1, score: 50000 },
                { rank: 6, playerName: "Sixth", teamType: "own", score: 49000 },
                { rank: 37, playerName: "ThirtySeven", teamType: "own", points: 999, score: 48000 },
                { rank: 94, playerName: "NinetyFour", teamType: "own", points: 0, score: 19000 },
                { rank: 95, playerName: "NoPoints", teamType: "opponent", score: null }
            ]
        }
    }], { name: "Discord", ownTeamAliases: [], ownPlayerAliases: [] });
    assert.deepStrictEqual(
        rankPointParsed.players.map(row => row.points),
        [300, 213, 25, 1, 0],
        "individual player event points should be normalized from the HCR2 rank table"
    );

    const teamConfig = {
        id: "spreadsheet-test",
        name: "Discord",
        ownTeamAliases: ["Discord"],
        ownPlayerAliases: ["jacco", "NoPrefixOwn"]
    };
    const processedAt = "2026-05-14T08:20:36.000Z";
    const ocrResults = [{
        imagePath: "fixture.png",
        text: "Chip Happens\nDiscord 2866\nMIDGARD 1654",
        model: "fixture",
        rawGeminiText: "",
        structured: {
            eventName: "Chip Happens",
            game: "Hill Climb Racing 2",
            teams: [
                { label: "Discord", teamType: "own", score: 2866 },
                { label: "MIDGARD", teamType: "opponent", score: 1654 }
            ],
            players: [
                { rank: 1, playerName: "DC|hype", teamLabel: "Discord", teamType: "own", points: 300, score: 51816 },
                { rank: 2, playerName: "M|clovin", teamLabel: "MIDGARD", teamType: "opponent", points: 280, score: 51714 },
                { rank: 3, playerName: "jacco", teamLabel: "", teamType: "opponent", points: 262, score: 51665 },
                { rank: 4, playerName: "NoPrefixOwn", teamLabel: "", teamType: "unknown", points: 244, score: 51661 },
                { rank: 5, playerName: "Beefeater", teamLabel: "MIDGARD", teamType: "opponent", points: 228, score: 51635 }
            ],
            metadata: { ownTeamScore: 2866, opponentTeamScore: 1654 }
        }
    }];

    const parsed = parseRaceScreenshots(ocrResults, teamConfig);
    const ownNames = parsed.players
        .filter(player => player.teamType === "own")
        .map(player => player.playerName);
    assert(ownNames.includes("jacco"), "known own player jacco should be classified as own");
    assert(ownNames.includes("NoPrefixOwn"), "known own player NoPrefixOwn should be classified as own");

    const session = {
        id: `spreadsheet-test-${stamp}`,
        teamId: teamConfig.id,
        teamName: teamConfig.name,
        status: "processed",
        processedAt,
        images: [{ name: "fixture.png" }],
        metadata: parsed.metadata,
        teamEventName: parsed.metadata.eventName,
        players: parsed.players,
        stats: parsed.stats,
        attendance: {
            roster: ownNames,
            attendedPlayers: ownNames,
            missingPlayers: ["KnownMissing"]
        }
    };

    const artifacts = await generateSpreadsheetArtifacts(session, path.join(outputDir, "event"), {
        outputFormat: "xlsx"
    });
    for (const filePath of [artifacts.spreadsheetPath, artifacts.spreadsheetImagePath, artifacts.chartPath]) {
        assert(filePath && fs.existsSync(filePath), `expected artifact to exist: ${filePath}`);
    }

    const eventSheets = await workbookSheets(artifacts.spreadsheetPath);
    assert.deepStrictEqual(eventSheets, ["Summary", "Results", "Attendance"]);

    const report = await generatePeriodReport(teamConfig, "weekly", {
        anchorDate: new Date(processedAt),
        sessions: [session],
        outputDir: path.join(outputDir, "reports")
    });
    assert(report, "weekly report should be generated");
    for (const filePath of [report.filePath, report.tableImagePath, report.chartPath]) {
        assert(filePath && fs.existsSync(filePath), `expected report artifact to exist: ${filePath}`);
    }

    const reportSheets = await workbookSheets(report.filePath);
    assert.deepStrictEqual(reportSheets, ["Report", "Details"]);
    assert.strictEqual(report.eventName, "Chip Happens");
    assert(report.enemyTeams.includes("MIDGARD"));

    console.log("Spreadsheet smoke test passed.");
    console.log(`Artifacts: ${outputDir}`);
}

main().catch(error => {
    console.error(error);
    process.exit(1);
});
