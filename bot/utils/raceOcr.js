const fs = require("fs");
const path = require("path");
const { spawn } = require("child_process");

const DEFAULT_TIMEOUT_MS = 120000;

function commandName(value, fallback) {
    const text = String(value || "").trim();
    return text || fallback;
}

function isMissingExecutable(error) {
    return error?.code === "ENOENT" || /ENOENT|not recognized|cannot find/i.test(error?.message || "");
}

function runCommand(command, args, options = {}) {
    return new Promise((resolve, reject) => {
        const child = spawn(command, args, {
            windowsHide: true,
            cwd: options.cwd || undefined
        });
        const stdout = [];
        const stderr = [];
        const timeout = setTimeout(() => {
            child.kill("SIGKILL");
            reject(new Error(`${path.basename(command)} timed out.`));
        }, options.timeoutMs || DEFAULT_TIMEOUT_MS);

        child.stdout.on("data", chunk => stdout.push(chunk));
        child.stderr.on("data", chunk => stderr.push(chunk));
        child.on("error", error => {
            clearTimeout(timeout);
            reject(error);
        });
        child.on("close", code => {
            clearTimeout(timeout);
            const out = Buffer.concat(stdout).toString("utf8");
            const err = Buffer.concat(stderr).toString("utf8");
            if (code !== 0) {
                reject(new Error(err.trim() || `${path.basename(command)} exited with code ${code}.`));
                return;
            }
            resolve({ stdout: out, stderr: err });
        });
    });
}

function parsePsmList(value) {
    const list = String(value || "6,11")
        .split(/[,\s]+/)
        .map(item => Number.parseInt(item, 10))
        .filter(value => Number.isInteger(value) && value >= 3 && value <= 13);
    return list.length ? [...new Set(list)] : [6, 11];
}

async function preprocessImageWithSharp(imagePath, outputPath) {
    try {
        const sharp = require("sharp");
        await sharp(imagePath, { animated: false })
            .rotate()
            .resize({
                width: 2200,
                height: 2200,
                fit: "inside",
                withoutEnlargement: true
            })
            .grayscale()
            .normalize()
            .toFile(outputPath);
        return fs.existsSync(outputPath) ? outputPath : imagePath;
    } catch {
        return imagePath;
    }
}

async function preprocessImage(imagePath, settings = {}) {
    if (settings.preprocess === false) return imagePath;

    const magick = commandName(settings.imageMagickPath || process.env.IMAGEMAGICK_PATH, "");
    const parsed = path.parse(imagePath);
    const outputPath = path.join(parsed.dir, `${parsed.name}.ocr-normalized.png`);

    if (magick) {
        try {
            await runCommand(magick, [
                imagePath,
                "-auto-orient",
                "-resize",
                "2200x2200>",
                "-colorspace",
                "Gray",
                "-contrast-stretch",
                "0.5%x0.5%",
                outputPath
            ], { timeoutMs: 60000 });
            return fs.existsSync(outputPath) ? outputPath : imagePath;
        } catch {
            return preprocessImageWithSharp(imagePath, outputPath);
        }
    }

    return preprocessImageWithSharp(imagePath, outputPath);
}

function normalizeOcrText(text) {
    return String(text || "")
        .replace(/\r/g, "\n")
        .replace(/[ \t\u00a0]+/g, " ")
        .split("\n")
        .map(line => line.trim())
        .filter(Boolean)
        .join("\n");
}

async function ocrImage(imagePath, settings = {}) {
    const configuredTesseract = commandName(settings.tesseractPath || process.env.TESSERACT_PATH, "");
    const tesseract = commandName(configuredTesseract, "tesseract");
    const lang = commandName(settings.tesseractLang || process.env.TESSERACT_LANG, "eng");
    const psmList = parsePsmList(settings.tesseractPsm);
    const preparedPath = await preprocessImage(imagePath, settings);

    if (!configuredTesseract || /^tesseract\.js$/i.test(configuredTesseract)) {
        return ocrImageWithTesseractJs(imagePath, preparedPath, lang, psmList);
    }

    const runs = [];

    try {
        for (const psm of psmList) {
            const result = await runCommand(tesseract, [
                preparedPath,
                "stdout",
                "-l",
                lang,
                "--psm",
                String(psm),
                "--oem",
                "1",
                "-c",
                "preserve_interword_spaces=1"
            ], { timeoutMs: settings.timeoutMs || DEFAULT_TIMEOUT_MS });

            runs.push({
                psm,
                text: normalizeOcrText(result.stdout),
                stderr: result.stderr
            });
        }
    } catch (error) {
        if (isMissingExecutable(error)) {
            return ocrImageWithTesseractJs(imagePath, preparedPath, lang, psmList);
        }
        throw error;
    }

    const best = runs
        .slice()
        .sort((a, b) => b.text.length - a.text.length)[0] || { psm: psmList[0], text: "" };

    return {
        imagePath,
        preparedPath,
        psm: best.psm,
        text: best.text,
        runs
    };
}

async function ocrImageWithTesseractJs(imagePath, preparedPath, lang, psmList) {
    const { createWorker } = require("tesseract.js");
    const cachePath = path.join(__dirname, "..", "data", "tesseract-cache");
    await fs.promises.mkdir(cachePath, { recursive: true });
    const worker = await createWorker(lang, 1, { cachePath });
    const runs = [];

    try {
        for (const psm of psmList) {
            await worker.setParameters({
                tessedit_pageseg_mode: String(psm),
                preserve_interword_spaces: "1"
            });
            const result = await worker.recognize(preparedPath);
            runs.push({
                psm,
                text: normalizeOcrText(result?.data?.text || ""),
                stderr: ""
            });
        }
    } finally {
        await worker.terminate().catch(() => null);
    }

    const best = runs
        .slice()
        .sort((a, b) => b.text.length - a.text.length)[0] || { psm: psmList[0], text: "" };

    return {
        imagePath,
        preparedPath,
        psm: best.psm,
        text: best.text,
        runs
    };
}

async function ocrImages(imagePaths, settings = {}) {
    const results = [];
    for (const imagePath of imagePaths) {
        results.push(await ocrImage(imagePath, settings));
    }
    return results;
}

function normalizeForMatch(value) {
    return String(value || "")
        .toLowerCase()
        .replace(/[\u00ae\u2122]/g, "")
        .replace(/[^a-z0-9]+/g, " ")
        .replace(/\s+/g, " ")
        .trim();
}

function parseNumber(value) {
    const text = String(value || "").trim().replace(/\s+/g, "");
    if (!text) return null;

    if (/^\d+[,.]\d+$/.test(text)) {
        const decimal = Number.parseFloat(text.replace(",", "."));
        return Number.isFinite(decimal) ? decimal : null;
    }

    const digits = text.replace(/\D/g, "");
    if (!digits) return null;
    const number = Number.parseInt(digits, 10);
    return Number.isFinite(number) ? number : null;
}

function escapeNameArtifacts(value) {
    return String(value || "")
        .replace(/[^\S\r\n]+/g, " ")
        .replace(/^[^A-Za-z0-9\[\]|]+/, "")
        .replace(/\s+/g, " ")
        .trim();
}

function extractTrailingScore(value) {
    const text = String(value || "").trim();
    const match = text.match(/(?:^|[\s:])(\d{1,3}(?:[\s,.]\d{3})+|\d{4,6})(?:\D*)$/);
    if (!match) return { score: null, rest: text };

    const score = parseNumber(match[1]);
    const rest = text.slice(0, match.index).trim();
    return { score, rest };
}

function parsePlacementLine(line) {
    const text = String(line || "")
        .replace(/[|]+/g, " ")
        .replace(/\s+/g, " ")
        .trim();
    const rankMatch = text.match(/(?:^|\s)(\d{1,2})\s*[\.)]\s+(.+)$/);
    if (!rankMatch) return null;

    const rank = Number.parseInt(rankMatch[1], 10);
    if (!Number.isInteger(rank) || rank < 1 || rank > 80) return null;

    const prefix = text.slice(0, rankMatch.index).trim();
    const prefixNumbers = prefix.match(/\d+(?:[,.]\d+)?/g) || [];
    const points = prefixNumbers.length ? parseNumber(prefixNumbers[prefixNumbers.length - 1]) : null;
    const scoreData = extractTrailingScore(rankMatch[2]);
    const name = escapeNameArtifacts(scoreData.rest)
        .replace(/\b(?:cc|challenger|legendary|vanguard)\b.*$/i, "")
        .trim();

    if (!name || /^\d+$/.test(name)) return null;

    return {
        rank,
        placement: rank,
        playerName: name.slice(0, 80),
        points,
        score: scoreData.score,
        rawLine: line
    };
}

function classifyPlayer(row, teamConfig = {}) {
    const aliases = [
        teamConfig.name,
        ...(teamConfig.ownTeamAliases || [])
    ].filter(Boolean);
    const normalizedLine = normalizeForMatch(`${row.playerName} ${row.rawLine || ""}`);
    const matchedAlias = aliases.find(alias => {
        const normalized = normalizeForMatch(alias);
        return normalized && normalizedLine.includes(normalized);
    });
    const dcPrefix = /\b(?:dc|dca)\s*[\]|]/i.test(row.playerName) || /^\s*(?:\[?dc\]?|\|?dc\|)/i.test(row.playerName);
    const own = Boolean(matchedAlias || dcPrefix);

    return {
        ...row,
        teamType: own ? "own" : "opponent",
        teamLabel: own ? (teamConfig.name || "Own team") : "Opponent",
        teamColor: own ? "yellow" : "blue",
        classificationSource: matchedAlias ? `alias:${matchedAlias}` : dcPrefix ? "name-prefix" : "default-opponent"
    };
}

function parseMetadata(ocrResults, teamConfig = {}) {
    const firstText = ocrResults[0]?.text || "";
    const firstLines = firstText.split("\n").map(line => line.trim()).filter(Boolean);
    const title = firstLines.find(line =>
        /[A-Za-z]/.test(line) &&
        !/winner|touch|continue|cup points|season points|garage power/i.test(line) &&
        !parsePlacementLine(line)
    ) || "Race Session";
    const scoreLine = firstLines.find(line => {
        const numbers = line.match(/\d[\d\s,.]{1,8}/g) || [];
        return numbers.length >= 2 && /vs|discord|power|academy|team/i.test(line);
    }) || "";
    const numbers = (scoreLine.match(/\d[\d\s,.]{1,8}/g) || [])
        .map(parseNumber)
        .filter(number => Number.isFinite(number));

    return {
        title: title.slice(0, 120),
        podiumFirst: true,
        ownTeamName: teamConfig.name || "",
        teamScores: numbers.length >= 2 ? {
            own: numbers[0],
            opponent: numbers[1],
            rawLine: scoreLine
        } : null
    };
}

function summarize(players) {
    const sorted = players.slice().sort((a, b) => a.rank - b.rank);
    const own = sorted.filter(player => player.teamType === "own");
    const opponents = sorted.filter(player => player.teamType !== "own");
    const sum = (items, key) => items.reduce((total, item) => total + (Number(item[key]) || 0), 0);
    const average = (items, key) => items.length
        ? Math.round((sum(items, key) / items.length) * 100) / 100
        : null;
    const buckets = [
        { id: "podium", label: "1-3", min: 1, max: 3 },
        { id: "top10", label: "4-10", min: 4, max: 10 },
        { id: "mid", label: "11-20", min: 11, max: 20 },
        { id: "lower", label: "21+", min: 21, max: 999 }
    ].map(bucket => ({
        ...bucket,
        own: own.filter(player => player.rank >= bucket.min && player.rank <= bucket.max).length,
        opponents: opponents.filter(player => player.rank >= bucket.min && player.rank <= bucket.max).length
    }));

    return {
        totalPlayers: sorted.length,
        ownPlayers: own.length,
        opponents: opponents.length,
        ownAverageRank: average(own, "rank"),
        opponentAverageRank: average(opponents, "rank"),
        ownPoints: sum(own, "points"),
        opponentPoints: sum(opponents, "points"),
        ownScore: sum(own, "score"),
        opponentScore: sum(opponents, "score"),
        podium: sorted.filter(player => player.rank <= 3),
        ownTop10: own.filter(player => player.rank <= 10).length,
        opponentTop10: opponents.filter(player => player.rank <= 10).length,
        buckets,
        opponentsBelowByPlayer: own.map(player => ({
            playerName: player.playerName,
            rank: player.rank,
            opponentsBelow: opponents.filter(opponent => opponent.rank > player.rank).length
        }))
    };
}

function parseRaceScreenshots(ocrResults, teamConfig = {}) {
    const rowMap = new Map();
    for (const result of ocrResults) {
        const lines = normalizeOcrText(result.text).split("\n");
        for (const line of lines) {
            const row = parsePlacementLine(line);
            if (!row) continue;
            const key = row.rank;
            const current = rowMap.get(key);
            if (!current || String(row.rawLine || "").length > String(current.rawLine || "").length) {
                rowMap.set(key, {
                    ...row,
                    sourceImage: path.basename(result.imagePath || "")
                });
            }
        }
    }

    const players = [...rowMap.values()]
        .sort((a, b) => a.rank - b.rank)
        .map(row => classifyPlayer(row, teamConfig));

    const stats = summarize(players);
    return {
        metadata: parseMetadata(ocrResults, teamConfig),
        players,
        stats,
        rawText: ocrResults.map((result, index) => `--- Image ${index + 1} / PSM ${result.psm} ---\n${result.text}`).join("\n\n")
    };
}

function knownTeamNames(config) {
    return [
        ...(config?.recruitment?.teams || []),
        ...(config?.memberCounts?.teams || []).flatMap(team => [team.name, ...(team.aliases || [])])
    ].filter(Boolean);
}

function analyzeRecruitLicenseText(text, config = {}) {
    const lines = normalizeOcrText(text).split("\n");
    const teamNames = knownTeamNames(config);
    let sourceTeam = "";
    let sourceIndex = -1;

    for (let index = 0; index < lines.length; index += 1) {
        const lineNorm = normalizeForMatch(lines[index]);
        const match = teamNames.find(team => {
            const teamNorm = normalizeForMatch(team);
            return teamNorm && lineNorm.includes(teamNorm);
        });
        if (match && !/cup points|season points|rank|rewards/i.test(lines[index])) {
            sourceTeam = match;
            sourceIndex = index;
            break;
        }
    }

    const nameLine = sourceIndex > 0
        ? lines.slice(Math.max(0, sourceIndex - 3), sourceIndex).reverse().find(line =>
            /[A-Za-z0-9]/.test(line) &&
            !/vs|medium|copy|points|rank|garage|achievement|season/i.test(line)
        )
        : lines.find(line => /(?:^|\b)(?:dc|dca)\s*[\]|]/i.test(line) || /[A-Za-z0-9].*[|].*[A-Za-z0-9]/.test(line));

    return {
        inGameName: escapeNameArtifacts(nameLine || "").slice(0, 80),
        sourceTeam: sourceTeam || "",
        rawText: normalizeOcrText(text)
    };
}

module.exports = {
    analyzeRecruitLicenseText,
    normalizeOcrText,
    ocrImage,
    ocrImages,
    parseRaceScreenshots,
    runCommand,
    summarizePlayers: summarize
};
