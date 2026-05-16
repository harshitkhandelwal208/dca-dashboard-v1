const assert = require("assert");
const fs = require("fs");
const path = require("path");
const ExcelJS = require("exceljs");
const { generateSpreadsheetArtifacts } = require("../utils/calcSpreadsheet");
const {
    parseEventPointsFromLeaderboardText,
    parseRaceScreenshots,
    parseTeamScoresFromOcr
} = require("../utils/raceOcr");
const { teamScoreFromMetadata } = require("../utils/spreadsheetMetrics");
const { generatePeriodReport } = require("../utils/spreadsheetReports");

async function workbookSheets(filePath) {
    const workbook = new ExcelJS.Workbook();
    await workbook.xlsx.readFile(filePath);
    return workbook.worksheets.map(sheet => sheet.name);
}

async function main() {
    const stamp = new Date().toISOString().replace(/[-:.TZ]/g, "").slice(0, 14);
    const outputDir = path.join(__dirname, "..", "data", "spreadsheets", "_test", stamp);
    await fs.promises.mkdir(outputDir, { recursive: true });

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

    const parsed = await parseRaceScreenshots(ocrResults, teamConfig);
    const ownNames = parsed.players
        .filter(player => player.teamType === "own")
        .map(player => player.playerName);
    assert(ownNames.includes("jacco"), "known own player jacco should be classified as own");
    assert(ownNames.includes("NoPrefixOwn"), "known own player NoPrefixOwn should be classified as own");
    assert(
        parsed.players.filter(player => player.teamType === "own").every(player => player.teamLabel === teamConfig.name),
        "own-team players should use the configured team name"
    );

    const scoreSession = {
        metadata: {
            teamScores: parseTeamScoresFromOcr([{
                text: "Chip Happens\nDiscord 2866\nMIDGARD 1654"
            }], teamConfig)
        }
    };
    assert.strictEqual(teamScoreFromMetadata(scoreSession, "own"), 2866);
    assert.strictEqual(teamScoreFromMetadata(scoreSession, "opponent"), 1654);
    assert.strictEqual(parseEventPointsFromLeaderboardText("300 1. DC|hype 51816"), 300);

    const pointsFallback = await parseRaceScreenshots([{
        imagePath: "fixture-leaderboard.png",
        text: "300 1. DC|hype 51816\n280 2. Enemy 51714",
        structured: {
            eventName: "Chip Happens",
            players: [
                { rank: 1, playerName: "DC|hype", teamType: "own", score: 51816 },
                { rank: 2, playerName: "Enemy", teamType: "opponent", score: 51714 }
            ]
        }
    }], teamConfig);
    assert.strictEqual(pointsFallback.players[0].points, 300);
    assert.strictEqual(pointsFallback.players[1].points, 280);

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

    const workbook = new ExcelJS.Workbook();
    await workbook.xlsx.readFile(artifacts.spreadsheetPath);
    const results = workbook.getWorksheet("Results");
    const headers = [];
    results.getRow(1).eachCell({ includeEmpty: true }, cell => headers.push(String(cell.value || "")));
    assert(!headers.includes("Type"), "Results sheet should not include a Type column");
    assert(headers.includes("Event Points"), "Results sheet should include Event Points");

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
