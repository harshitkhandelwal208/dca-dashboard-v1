const fs = require("fs");
const path = require("path");
const { spawn } = require("child_process");

const DEFAULT_TIMEOUT_MS = 300000;
const DEFAULT_GEMINI_MODEL = "gemini-2.5-flash";
const DEFAULT_MAX_RETRIES = 4;
const ROW_COLOR_SCAN_REGION = { xStart: 0.58, xEnd: 0.7 };
const ROW_COLOR_MIN_CONFIDENCE = 0.35;
const ROW_COLOR_STRONG_CONFIDENCE = 0.58;
const HCR2_TEAM_EVENT_POINTS_BY_RANK = [
    300, 280, 262, 244, 228, 213, 198, 185, 173, 161,
    150, 140, 131, 122, 114, 107, 99, 93, 87, 81,
    75, 70, 66, 61, 57, 54, 50, 47, 44, 41,
    38, 35, 33, 31, 29, 27, 25, 24, 22, 21,
    19, 18, 17, 16, 15, 14, 13, 12, 11, 10,
    9, 9, 9, 8, 8, 7, 7, 6, 6, 6,
    5, 5, 5, 4, 4, 4, 4, 3, 3, 3,
    3, 3, 3, 2, 2, 2, 2, 2, 2, 2,
    2, 2, 2, 1, 1, 1, 1, 1, 1, 1,
    1, 1, 1, 1
];

function commandName(value, fallback) {
    const text = String(value || "").trim();
    return text || fallback;
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

function normalizeOcrText(text) {
    return String(text || "")
        .replace(/\r/g, "\n")
        .replace(/[ \t\u00a0]+/g, " ")
        .split("\n")
        .map(line => line.trim())
        .filter(Boolean)
        .join("\n");
}

function normalizeForMatch(value) {
    return String(value || "")
        .toLowerCase()
        .replace(/[\u00ae\u2122]/g, "")
        .replace(/[^a-z0-9]+/g, " ")
        .replace(/\s+/g, " ")
        .trim();
}

function normalizeRowColor(value) {
    const text = normalizeForMatch(value);
    if (!text) return "";
    if (/\b(?:yellow|gold|golden|orange)\b/.test(text)) return "yellow";
    if (/\bblue\b/.test(text)) return "blue";
    return "";
}

function teamTypeFromRowColor(value) {
    const color = normalizeRowColor(value);
    if (color === "yellow") return "own";
    if (color === "blue") return "opponent";
    return "";
}

function parseNumber(value) {
    if (value === null || value === undefined) return null;
    if (typeof value === "number") return Number.isFinite(value) ? value : null;

    const text = String(value).trim().replace(/\s+/g, "");
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

function firstNumber(...values) {
    for (const value of values) {
        const parsed = parseNumber(value);
        if (parsed !== null) return parsed;
    }
    return null;
}

function hcr2TeamEventPointsForRank(rank) {
    const value = HCR2_TEAM_EVENT_POINTS_BY_RANK[Number(rank) - 1];
    return Number.isFinite(value) ? value : 0;
}

function normalizeEventPointsForRank(rank, extractedPoints) {
    const expected = hcr2TeamEventPointsForRank(rank);
    if (expected > 0) return expected;
    return extractedPoints === null ? 0 : extractedPoints;
}

function cleanText(value, maxLength = 120) {
    return String(value || "")
        .replace(/[^\S\r\n]+/g, " ")
        .replace(/\s+/g, " ")
        .trim()
        .slice(0, maxLength);
}

function escapeNameArtifacts(value) {
    return cleanText(value, 120)
        .replace(/^[^A-Za-z0-9\[\]|]+/, "")
        .replace(/\b(?:cc|challenger|legendary|vanguard)\b.*$/i, "")
        .trim();
}

function extractTrailingScore(value) {
    const text = String(value || "").trim();
    const match = text.match(/(?:^|[\s:])(\d{1,3}(?:[\s,.]\d{3})+|\d{4,6})(?:\D*)$/);
    if (!match) return { score: null, rest: text };

    return {
        score: parseNumber(match[1]),
        rest: text.slice(0, match.index).trim()
    };
}

function parsePlacementLine(line) {
    const text = String(line || "")
        .replace(/[|]+/g, " ")
        .replace(/\s+/g, " ")
        .trim();
    const rankMatch = text.match(/(?:^|\s)(\d{1,2})\s*[\.)]\s+(.+)$/);
    if (!rankMatch) return null;

    const rank = Number.parseInt(rankMatch[1], 10);
    if (!Number.isInteger(rank) || rank < 1 || rank > 100) return null;

    const prefix = text.slice(0, rankMatch.index).trim();
    const prefixNumbers = prefix.match(/\d+(?:[,.]\d+)?/g) || [];
    const points = prefixNumbers.length ? parseNumber(prefixNumbers[prefixNumbers.length - 1]) : null;
    const scoreData = extractTrailingScore(rankMatch[2]);
    const name = escapeNameArtifacts(scoreData.rest);

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

function geminiApiKey(settings = {}) {
    return String(
        settings.geminiApiKey ||
        process.env.GEMINI_API_KEY ||
        process.env.GOOGLE_GEMINI_API_KEY ||
        ""
    ).trim();
}

function geminiModel(settings = {}) {
    return String(
        settings.geminiModel ||
        process.env.GEMINI_FLASH_MODEL ||
        process.env.GEMINI_MODEL ||
        DEFAULT_GEMINI_MODEL
    ).trim().replace(/^models\//, "") || DEFAULT_GEMINI_MODEL;
}

function geminiEndpoint(settings = {}) {
    const base = String(settings.geminiApiBaseUrl || process.env.GEMINI_API_BASE_URL || "https://generativelanguage.googleapis.com").replace(/\/+$/, "");
    return `${base}/v1beta/models/${encodeURIComponent(geminiModel(settings))}:generateContent`;
}

function mimeForPath(filePath) {
    const ext = path.extname(filePath || "").toLowerCase();
    if (ext === ".png") return "image/png";
    if (ext === ".webp") return "image/webp";
    if (ext === ".gif") return "image/gif";
    return "image/jpeg";
}

function clamp(value, min, max) {
    return Math.max(min, Math.min(max, value));
}

function median(values) {
    const sorted = values
        .map(Number)
        .filter(Number.isFinite)
        .sort((a, b) => a - b);
    if (!sorted.length) return null;
    const middle = Math.floor(sorted.length / 2);
    return sorted.length % 2
        ? sorted[middle]
        : (sorted[middle - 1] + sorted[middle]) / 2;
}

function averagePixels(data, info, box, options = {}) {
    const channels = info.channels || 4;
    const x0 = clamp(Math.round(box.x0), 0, info.width - 1);
    const x1 = clamp(Math.round(box.x1), x0 + 1, info.width);
    const y0 = clamp(Math.round(box.y0), 0, info.height - 1);
    const y1 = clamp(Math.round(box.y1), y0 + 1, info.height);
    const xStep = Math.max(1, Math.round((x1 - x0) / (options.samplesX || 36)));
    const yStep = Math.max(1, Math.round((y1 - y0) / (options.samplesY || 8)));
    let r = 0;
    let g = 0;
    let b = 0;
    let count = 0;

    for (let y = y0; y < y1; y += yStep) {
        for (let x = x0; x < x1; x += xStep) {
            const offset = (y * info.width + x) * channels;
            const alpha = channels > 3 ? data[offset + 3] : 255;
            if (alpha < 128) continue;

            const red = data[offset];
            const green = data[offset + 1];
            const blue = data[offset + 2];
            const max = Math.max(red, green, blue);
            const min = Math.min(red, green, blue);

            if (options.skipInk && (max < 35 || (max > 235 && max - min < 28))) continue;

            r += red;
            g += green;
            b += blue;
            count += 1;
        }
    }

    return count ? { r: r / count, g: g / count, b: b / count, count } : { r: 0, g: 0, b: 0, count: 0 };
}

function classifyBackgroundColor(stats) {
    if (!stats.count) return { color: "unknown", confidence: 0 };

    const yellowScore = Math.min(stats.r, stats.g) - stats.b;
    const blueScore = (stats.b - stats.r) + Math.max(0, stats.b - stats.g) * 0.25;
    const yellowConfidence = clamp((yellowScore - 18) / 90, 0, 1);
    const blueConfidence = clamp((blueScore - 30) / 110, 0, 1);

    if (yellowScore > 25 && yellowScore > blueScore && stats.r > 120 && stats.g > 120) {
        return { color: "yellow", confidence: yellowConfidence };
    }

    if (blueScore > 40 && blueScore > yellowScore && stats.b > 135 && stats.g > 75) {
        return { color: "blue", confidence: blueConfidence };
    }

    return { color: "unknown", confidence: 0 };
}

function rowColorScanRegion(settings = {}) {
    const xStart = Number(settings.rowColorSampleXStart ?? ROW_COLOR_SCAN_REGION.xStart);
    const xEnd = Number(settings.rowColorSampleXEnd ?? ROW_COLOR_SCAN_REGION.xEnd);
    return {
        xStart: clamp(Number.isFinite(xStart) ? xStart : ROW_COLOR_SCAN_REGION.xStart, 0, 0.95),
        xEnd: clamp(Number.isFinite(xEnd) ? xEnd : ROW_COLOR_SCAN_REGION.xEnd, 0.05, 1)
    };
}

function splitTallColorBands(bands) {
    const typicalHeight = median(bands.map(band => band.end - band.start + 1));
    if (!typicalHeight) return bands;

    const output = [];
    for (const band of bands) {
        const height = band.end - band.start + 1;
        const parts = height > typicalHeight * 1.35
            ? clamp(Math.round(height / typicalHeight), 2, 8)
            : 1;

        for (let part = 0; part < parts; part += 1) {
            const start = Math.round(band.start + (height * part) / parts);
            const end = Math.round(band.start + (height * (part + 1)) / parts) - 1;
            output.push({
                ...band,
                start,
                end,
                splitPart: parts > 1 ? part + 1 : undefined,
                splitParts: parts > 1 ? parts : undefined
            });
        }
    }

    return output;
}

function detectRowColorBands(data, info, settings = {}) {
    const region = rowColorScanRegion(settings);
    const x0 = Math.round(info.width * Math.min(region.xStart, region.xEnd - 0.02));
    const x1 = Math.round(info.width * Math.max(region.xEnd, region.xStart + 0.02));
    const minHeight = Math.max(14, Math.round(info.height * 0.011));
    const runs = [];
    let current = null;

    for (let y = 0; y < info.height; y += 1) {
        const stats = averagePixels(data, info, {
            x0,
            x1,
            y0: y,
            y1: y + 1
        }, { samplesX: 42, samplesY: 1 });
        const classified = classifyBackgroundColor(stats);
        const color = classified.confidence >= ROW_COLOR_MIN_CONFIDENCE ? classified.color : "unknown";

        if (!current || current.color !== color) {
            if (current) runs.push(current);
            current = {
                color,
                start: y,
                end: y,
                confidenceTotal: classified.confidence,
                confidenceSamples: 1
            };
        } else {
            current.end = y;
            current.confidenceTotal += classified.confidence;
            current.confidenceSamples += 1;
        }
    }
    if (current) runs.push(current);

    const bands = runs
        .filter(run => run.color !== "unknown")
        .map(run => ({
            color: run.color,
            start: run.start,
            end: run.end,
            confidence: run.confidenceTotal / Math.max(1, run.confidenceSamples)
        }))
        .filter(run => run.end - run.start + 1 >= minHeight && run.confidence >= ROW_COLOR_MIN_CONFIDENCE);

    if (bands.length < 2) return [];

    return splitTallColorBands(bands)
        .map((band, index) => {
            const height = band.end - band.start + 1;
            const stats = averagePixels(data, info, {
                x0,
                x1,
                y0: band.start + height * 0.22,
                y1: band.end - height * 0.22
            }, { samplesX: 48, samplesY: 12, skipInk: true });
            const classified = classifyBackgroundColor(stats);
            const color = classified.color === "unknown" ? band.color : classified.color;
            const confidence = Math.max(band.confidence, classified.confidence);

            return {
                rowIndex: index + 1,
                color,
                confidence: Math.round(confidence * 1000) / 1000,
                yStart: band.start,
                yEnd: band.end,
                sampleRegion: {
                    xStart: region.xStart,
                    xEnd: region.xEnd
                }
            };
        })
        .filter(row => normalizeRowColor(row.color) && row.confidence >= ROW_COLOR_MIN_CONFIDENCE);
}

async function detectRaceRowColors(imagePath, settings = {}) {
    if (settings.detectRowColors === false) return [];

    const sharp = require("sharp");
    const { data, info } = await sharp(imagePath, { animated: false })
        .rotate()
        .ensureAlpha()
        .raw()
        .toBuffer({ resolveWithObject: true });

    return detectRowColorBands(data, info, settings);
}

async function detectRaceRowColorsSafe(imagePath, settings = {}) {
    try {
        return await detectRaceRowColors(imagePath, settings);
    } catch {
        return [];
    }
}

async function prepareImageForGemini(imagePath, settings = {}) {
    if (settings.preprocess === false) return imagePath;

    try {
        const sharp = require("sharp");
        const parsed = path.parse(imagePath);
        const outputPath = path.join(parsed.dir, `${parsed.name}.gemini.jpg`);
        await sharp(imagePath, { animated: false })
            .rotate()
            .resize({
                width: 2400,
                height: 2400,
                fit: "inside",
                withoutEnlargement: true
            })
            .jpeg({ quality: 92, mozjpeg: true })
            .toFile(outputPath);
        return fs.existsSync(outputPath) ? outputPath : imagePath;
    } catch {
        return imagePath;
    }
}

async function imagePart(filePath) {
    return {
        inline_data: {
            mime_type: mimeForPath(filePath),
            data: (await fs.promises.readFile(filePath)).toString("base64")
        }
    };
}

function responseText(payload) {
    return (payload?.candidates || [])
        .flatMap(candidate => candidate?.content?.parts || [])
        .map(part => part.text || "")
        .join("\n")
        .trim();
}

async function sleep(ms) {
    await new Promise(resolve => setTimeout(resolve, ms));
}

function geminiTimeoutMs(settings = {}) {
    const value = Number(settings.geminiTimeoutMs ?? settings.timeoutMs ?? process.env.GEMINI_TIMEOUT_MS ?? DEFAULT_TIMEOUT_MS);
    return Number.isFinite(value) && value > 0 ? Math.round(value) : DEFAULT_TIMEOUT_MS;
}

function geminiMaxRetries(settings = {}) {
    const value = Number(settings.geminiMaxRetries ?? process.env.GEMINI_MAX_RETRIES ?? DEFAULT_MAX_RETRIES);
    return Number.isFinite(value) && value >= 0 ? Math.min(10, Math.round(value)) : DEFAULT_MAX_RETRIES;
}

function geminiRetryDelayMs(attempt) {
    return Math.min(15000, 1000 * (2 ** attempt));
}

function isRetryableGeminiError(error) {
    return error?.name === "AbortError" || [408, 429, 500, 502, 503, 504].includes(Number(error?.status));
}

function geminiAttemptMessage(error, attempt, maxRetries, timeoutMs) {
    const prefix = `Gemini Flash attempt ${attempt + 1}/${maxRetries + 1}`;
    const status = Number(error?.status);

    if (error?.name === "AbortError") {
        return `${prefix} timed out after ${Math.round(timeoutMs / 1000)}s.`;
    }

    if (status === 429) return `${prefix} was rate limited (429).`;
    if ([500, 502, 503, 504].includes(status)) return `${prefix} hit a temporary API error (${status}).`;

    return `${prefix} failed: ${error?.message || "Unknown error"}`;
}

async function callGemini(parts, settings = {}) {
    const key = geminiApiKey(settings);
    if (!key) {
        throw new Error("GEMINI_API_KEY or GOOGLE_GEMINI_API_KEY is required for team-event screenshot extraction.");
    }

    const body = {
        contents: [{ role: "user", parts }],
        generationConfig: {
            temperature: 0,
            responseMimeType: "application/json"
        }
    };
    const url = geminiEndpoint(settings);
    const timeoutMs = geminiTimeoutMs(settings);
    const maxRetries = geminiMaxRetries(settings);
    let lastError;

    for (let attempt = 0; attempt <= maxRetries; attempt += 1) {
        const controller = new AbortController();
        const timeout = setTimeout(() => controller.abort(), timeoutMs);

        try {
            const response = await fetch(url, {
                method: "POST",
                headers: {
                    "Content-Type": "application/json",
                    "x-goog-api-key": key
                },
                body: JSON.stringify(body),
                signal: controller.signal
            });
            const text = await response.text();
            clearTimeout(timeout);

            if (!response.ok) {
                const error = new Error(`Gemini Flash request failed (${response.status}): ${text.slice(0, 500)}`);
                error.status = response.status;
                throw error;
            }

            const json = JSON.parse(text);
            return {
                model: geminiModel(settings),
                text: responseText(json),
                raw: json,
                usageMetadata: json.usageMetadata || null
            };
        } catch (error) {
            clearTimeout(timeout);
            lastError = error;
            const retryable = isRetryableGeminiError(error);
            const canRetry = retryable && attempt < maxRetries;
            const message = geminiAttemptMessage(error, attempt, maxRetries, timeoutMs);

            if (canRetry) {
                const delayMs = geminiRetryDelayMs(attempt);
                console.warn(`${message} Retrying in ${delayMs}ms.`);
                await sleep(delayMs);
                continue;
            }

            console.warn(message);
            break;
        }
    }

    if (lastError?.name === "AbortError") {
        throw new Error(`Gemini Flash extraction timed out after ${Math.round(timeoutMs / 1000)} seconds. Try fewer screenshots, lower-resolution screenshots, rerun Gemini for the session, or raise GEMINI_TIMEOUT_MS.`);
    }

    const status = Number(lastError?.status);
    if (status === 429) {
        throw new Error("Gemini Flash API rate-limited this request (429). Wait a minute and rerun Gemini for the session, or lower concurrent spreadsheet processing.");
    }
    if ([500, 502, 503, 504].includes(status)) {
        throw new Error(`Gemini Flash API is temporarily unavailable (${status}). Rerun Gemini for the session after the API recovers, or increase GEMINI_MAX_RETRIES.`);
    }

    throw lastError;
}

function parseGeminiJson(text) {
    const raw = String(text || "").trim();
    if (!raw) return null;

    const withoutFence = raw
        .replace(/^```(?:json)?/i, "")
        .replace(/```$/i, "")
        .trim();

    const candidates = [withoutFence];
    const firstBrace = withoutFence.indexOf("{");
    const lastBrace = withoutFence.lastIndexOf("}");
    if (firstBrace !== -1 && lastBrace > firstBrace) {
        candidates.push(withoutFence.slice(firstBrace, lastBrace + 1));
    }

    for (const candidate of candidates) {
        try {
            return JSON.parse(candidate);
        } catch {
            // Try the next shape.
        }
    }

    return null;
}

function teamAliases(teamConfig = {}) {
    return [
        teamConfig.name,
        ...(teamConfig.ownTeamAliases || [])
    ].filter(Boolean);
}

function ownPlayerAliases(teamConfig = {}) {
    return [
        ...(teamConfig.ownPlayerAliases || []),
        ...(teamConfig.ownPlayerNames || []),
        ...(teamConfig.knownOwnPlayers || [])
    ].filter(Boolean);
}

function playerAliasMatches(playerName, teamConfig = {}) {
    const normalizedName = normalizeForMatch(playerName);
    if (!normalizedName) return "";

    const nameTokens = new Set(normalizedName.split(" ").filter(Boolean));
    return ownPlayerAliases(teamConfig).find(alias => {
        const aliasNorm = normalizeForMatch(alias);
        if (!aliasNorm) return false;
        if (normalizedName === aliasNorm) return true;
        if (aliasNorm.length < 3) return false;
        if (nameTokens.has(aliasNorm)) return true;
        return normalizedName.split(" ").join("").endsWith(aliasNorm.split(" ").join(""));
    }) || "";
}

function buildTeamEventPrompt(teamConfig = {}) {
    const aliases = teamAliases(teamConfig);
    const knownPlayers = ownPlayerAliases(teamConfig).slice(0, 150);
    return [
        "You extract structured data from Hill Climb Racing 2 team-event screenshots and generated team-event spreadsheets.",
        "All images in this request are from one Discord submission/session and may include podium, team score summary, final standings, cropped ranking lists, or spreadsheet screenshots.",
        "Return only valid JSON. Do not use markdown.",
        "",
        "Configured own team:",
        `- name: ${teamConfig.name || "unknown"}`,
        `- aliases: ${aliases.length ? aliases.join(", ") : "none"}`,
        `- known own players: ${knownPlayers.length ? knownPlayers.join(", ") : "none"}`,
        "",
        "Important rules:",
        "- Identify the event name from the screenshots. Use the visible event title, not the Discord channel name.",
        "- Extract every visible ranking/player row, even from cropped screenshots or tablet screenshots.",
        "- Preserve placements/ranks exactly. Tied ranks are allowed.",
        "- Extract event points/cup points separately from total score when both are visible.",
        "- Extract the team-event point totals from podium/summary screens into metadata.ownTeamScore and metadata.opponentTeamScore. Example: Discord 1 574 vs EMPIRE 2 946 means ownTeamScore 1574 and opponentTeamScore 2946. These are not player score sums.",
        "- For scores with spaces, output the numeric value without spaces, for example '52 723' becomes 52723.",
        "- Classify each row as own, opponent, or unknown by using team labels, aliases, colors, side of the versus screen, highlighted rows, prefixes like DC, and surrounding layout context.",
        "- When using standings row color, ignore flags, country icons, player names, badges, scores, and borders. Judge only the clean right-middle background strip between the player-name area and the badge/score columns.",
        "- For standings rows, yellow/gold background rows are own-team rows and blue background rows are opponent rows. Use unknown if the row background is not clear.",
        "- If a visible player name exactly matches a configured known own player, classify that player as own even when the row has no visible team label.",
        "- Do not mix team assignments. If a row cannot be verified as the configured own team, use opponent or unknown.",
        "- Do not calculate #KAB. The bot calculates it after parsing.",
        "- Include raw visible text for debugging.",
        "",
        "JSON shape:",
        "{",
        "  \"eventName\": \"string\",",
        "  \"game\": \"Hill Climb Racing 2\",",
        "  \"screenshotTypes\": [\"podium\", \"standings\", \"spreadsheet\", \"profile\", \"other\"],",
        "  \"teams\": [{\"label\":\"string\", \"teamType\":\"own|opponent|unknown\", \"score\": 0, \"side\":\"left|right|none\", \"reason\":\"string\"}],",
        "  \"players\": [{",
        "    \"rank\": 1,",
        "    \"playerName\": \"string\",",
        "    \"teamLabel\": \"string\",",
        "    \"teamType\": \"own|opponent|unknown\",",
        "    \"rowColor\": \"yellow|blue|unknown\",",
        "    \"points\": 0,",
        "    \"score\": 0,",
        "    \"sourceImageIndex\": 1,",
        "    \"rawText\": \"string\",",
        "    \"classificationReason\": \"string\",",
        "    \"confidence\": 0.0",
        "  }],",
        "  \"podium\": [{\"rank\":1, \"playerName\":\"string\", \"teamType\":\"own|opponent|unknown\"}],",
        "  \"metadata\": {\"winner\":\"string\", \"ownTeamScore\":0, \"opponentTeamScore\":0, \"teamScoreLine\":\"string\", \"notes\":\"string\"},",
        "  \"rawVisibleTextByImage\": [{\"imageIndex\":1, \"text\":\"string\"}]",
        "}"
    ].join("\n");
}

function buildVisibleTextPrompt() {
    return [
        "Extract all visible text from this Hill Climb Racing 2 screenshot.",
        "Return only JSON with this shape:",
        "{",
        "  \"text\": \"all visible text in reading order\",",
        "  \"playerName\": \"visible player name if this is a profile/license screen\",",
        "  \"teamName\": \"visible team name if present\"",
        "}"
    ].join("\n");
}

function buildRecruitmentApplicationPrompt(settings = {}) {
    const knownTeams = Array.isArray(settings.knownTeams) ? settings.knownTeams.filter(Boolean) : [];
    return [
        "You extract recruitment logging data from Hill Climb Racing 2 application screenshots.",
        "Images labeled Driver License show the applicant's license/profile. Images labeled Team Event Scores show optional team-event result screenshots from the same applicant.",
        "Return only valid JSON. Do not use markdown.",
        "",
        "Context:",
        `- applicantDiscordId: ${settings.applicantDiscordId || "unknown"}`,
        `- selectedTeamJoined: ${settings.acceptedTeam || "Rejected"}`,
        `- knownRecruitmentTeams: ${knownTeams.length ? knownTeams.join(", ") : "none"}`,
        "",
        "Extraction rules:",
        "- From the driver license/profile image, extract the in-game player name, previous/current team, and garage power.",
        "- Garage power can be labeled Garage Power, GP, Power, or shown as a large profile stat. Preserve the visible number.",
        "- From each team-event score screenshot, extract the visible event name and the applicant's visible rank, event points, and score when present.",
        "- Do not invent values. Use an empty string for fields that are not visible.",
        "- Include raw visible text for debugging.",
        "",
        "JSON shape:",
        "{",
        "  \"driverLicense\": {",
        "    \"playerName\": \"string\",",
        "    \"previousTeam\": \"string\",",
        "    \"garagePower\": \"string\",",
        "    \"rawVisibleText\": \"string\"",
        "  },",
        "  \"teamEvents\": [{",
        "    \"eventName\": \"string\",",
        "    \"rank\": \"string\",",
        "    \"eventPoints\": \"string\",",
        "    \"score\": \"string\",",
        "    \"rawVisibleText\": \"string\"",
        "  }],",
        "  \"rawVisibleTextByImage\": [{\"imageIndex\":1, \"label\":\"string\", \"text\":\"string\"}]",
        "}"
    ].join("\n");
}

function normalizeRecruitmentImageInput(input, index) {
    if (typeof input === "string") {
        return {
            path: input,
            label: `Image ${index + 1}`,
            kind: "image"
        };
    }

    return {
        path: input.path || input.filePath || input.imagePath,
        label: input.label || `Image ${index + 1}`,
        kind: input.kind || "image"
    };
}

async function extractTeamEventSession(imagePaths, teamConfig = {}, settings = {}) {
    const prepared = [];
    const parts = [{ text: buildTeamEventPrompt(teamConfig) }];

    for (let index = 0; index < imagePaths.length; index += 1) {
        const preparedPath = await prepareImageForGemini(imagePaths[index], settings);
        prepared.push(preparedPath);
        parts.push({ text: `Image ${index + 1}: ${path.basename(imagePaths[index])}` });
        parts.push(await imagePart(preparedPath));
    }

    const rowColorHintsPromise = Promise.all(imagePaths.map(imagePath => detectRaceRowColorsSafe(imagePath, settings)));
    const [response, rowColorHints] = await Promise.all([
        callGemini(parts, settings),
        rowColorHintsPromise
    ]);
    const structured = parseGeminiJson(response.text);
    const rawVisibleTextByImage = Array.isArray(structured?.rawVisibleTextByImage)
        ? structured.rawVisibleTextByImage
        : [];

    const ocrResults = imagePaths.map((imagePath, index) => {
        const imageText = rawVisibleTextByImage.find(item => Number(item.imageIndex) === index + 1)?.text || "";
        return {
            imagePath,
            preparedPath: prepared[index],
            psm: "gemini-flash",
            text: normalizeOcrText(imageText || response.text),
            model: response.model,
            usageMetadata: response.usageMetadata,
            structured: index === 0 ? structured : null,
            rawGeminiText: index === 0 ? response.text : "",
            rowColorHints: rowColorHints[index] || [],
            extractionType: "gemini-flash"
        };
    });

    return {
        model: response.model,
        structured,
        rawResponseText: response.text,
        rawResponse: response.raw,
        usageMetadata: response.usageMetadata,
        ocrResults
    };
}

async function extractRecruitmentApplication(images, settings = {}) {
    const prepared = [];
    const parts = [{ text: buildRecruitmentApplicationPrompt(settings) }];
    const inputs = images
        .map(normalizeRecruitmentImageInput)
        .filter(input => input.path);

    for (let index = 0; index < inputs.length; index += 1) {
        const input = inputs[index];
        const preparedPath = await prepareImageForGemini(input.path, settings);
        prepared.push(preparedPath);
        parts.push({ text: `${input.label} (${input.kind}): ${path.basename(input.path)}` });
        parts.push(await imagePart(preparedPath));
    }

    const response = await callGemini(parts, settings);
    const structured = parseGeminiJson(response.text);

    return {
        model: response.model,
        structured,
        rawResponseText: response.text,
        rawResponse: response.raw,
        usageMetadata: response.usageMetadata,
        preparedPaths: prepared
    };
}

async function extractVisibleText(imagePath, settings = {}) {
    const preparedPath = await prepareImageForGemini(imagePath, settings);
    const response = await callGemini([
        { text: buildVisibleTextPrompt() },
        await imagePart(preparedPath)
    ], settings);
    const parsed = parseGeminiJson(response.text);
    const text = parsed?.text || response.text;

    return {
        imagePath,
        preparedPath,
        psm: "gemini-flash",
        text: normalizeOcrText(text),
        model: response.model,
        structured: parsed,
        rawGeminiText: response.text,
        extractionType: "gemini-flash"
    };
}

async function ocrImage(imagePath, settings = {}) {
    return extractVisibleText(imagePath, settings);
}

async function ocrImages(imagePaths, settings = {}) {
    const extracted = await extractTeamEventSession(imagePaths, settings.teamConfig || {}, settings);
    return extracted.ocrResults;
}

function normalizeTeamType(value) {
    const text = normalizeForMatch(value);
    if (["own", "our", "team", "teammate", "ally", "allied", "yellow", "gold", "golden"].includes(text)) return "own";
    if (["opponent", "enemy", "opposing", "rival", "other", "blue"].includes(text)) return "opponent";
    return "unknown";
}

function aliasMatches(text, aliases) {
    const normalized = normalizeForMatch(text);
    return aliases.find(alias => {
        const aliasNorm = normalizeForMatch(alias);
        return aliasNorm && normalized.includes(aliasNorm);
    });
}

function classifyPlayer(row, teamConfig = {}) {
    const aliases = teamAliases(teamConfig);
    const haystack = `${row.playerName || ""} ${row.teamLabel || ""} ${row.rawLine || ""} ${row.classificationReason || ""}`;
    const matchedAlias = aliasMatches(haystack, aliases);
    const matchedNameAlias = aliasMatches(row.playerName || "", aliases);
    const matchedPlayer = playerAliasMatches(row.playerName, teamConfig);
    const dcPrefix = /\b(?:dc|dca)\s*[\]|]/i.test(row.playerName || "") || /^\s*(?:\[?dc\]?|\|?dc\|)/i.test(row.playerName || "");
    const normalizedType = normalizeTeamType(row.teamType);
    const rowColor = normalizeRowColor(row.rowColor || row.teamColor || row.backgroundColor || row.color);
    const rowColorType = teamTypeFromRowColor(rowColor);
    const rowColorConfidence = Number(row.rowColorConfidence);
    const strongSampledColor = row.rowColorSource === "image-sampler" &&
        rowColorType &&
        Number.isFinite(rowColorConfidence) &&
        rowColorConfidence >= ROW_COLOR_STRONG_CONFIDENCE;
    const sampledColorOverridesGemini = Boolean(strongSampledColor);
    const own = sampledColorOverridesGemini
        ? rowColorType === "own"
        : Boolean(matchedNameAlias || matchedPlayer || dcPrefix || rowColorType === "own" || normalizedType === "own");
    const opponent = sampledColorOverridesGemini
        ? rowColorType === "opponent"
        : normalizedType === "opponent" || rowColorType === "opponent" || !own;
    const classificationSource = sampledColorOverridesGemini
        ? `image-row-color:${rowColor}`
        : matchedAlias
            ? `alias:${matchedAlias}`
            : matchedPlayer
                ? `known-player:${matchedPlayer}`
                : dcPrefix
                    ? "name-prefix"
                    : rowColorType
                        ? `${row.rowColorSource || "row-color"}:${rowColor}`
                        : normalizedType === "own"
                            ? "gemini-own"
                            : opponent
                                ? "gemini-opponent"
                                : "default-opponent";

    return {
        ...row,
        teamType: own ? "own" : "opponent",
        teamLabel: own
            ? (teamConfig.name || row.teamLabel || "Own team")
            : (row.teamLabel && !matchedAlias ? row.teamLabel : "Opponent"),
        teamColor: own ? "yellow" : "blue",
        rowColor: rowColor || (own ? "yellow" : "blue"),
        classificationSource
    };
}

function rowSourceImage(row, ocrResults) {
    const index = Number(row.sourceImageIndex || row.imageIndex || row.sourceImage || 0);
    const result = Number.isInteger(index) && index > 0 ? ocrResults[index - 1] : null;
    return result?.imagePath ? path.basename(result.imagePath) : "";
}

function rowSourceImageIndex(row, fallback = 1) {
    const index = Number(row.sourceImageIndex || row.imageIndex || row.sourceImage || fallback);
    return Number.isInteger(index) && index > 0 ? index : fallback;
}

function rowRankValue(row) {
    return firstNumber(row.rank, row.placement, row.position, row.place);
}

function cleanRowColorHint(hint) {
    const color = normalizeRowColor(hint?.color || hint?.rowColor || hint?.teamColor);
    const confidence = Number(hint?.confidence);
    if (!color || !Number.isFinite(confidence) || confidence < ROW_COLOR_MIN_CONFIDENCE) return null;
    return {
        color,
        confidence,
        rowIndex: Number(hint.rowIndex) || null,
        yStart: Number.isFinite(Number(hint.yStart)) ? Number(hint.yStart) : null,
        yEnd: Number.isFinite(Number(hint.yEnd)) ? Number(hint.yEnd) : null,
        sampleRegion: hint.sampleRegion || null
    };
}

function annotateRowWithColorHint(rows, item, hint) {
    if (!hint) return;
    rows[item.index] = {
        ...rows[item.index],
        rowColor: hint.color,
        rowColorConfidence: hint.confidence,
        rowColorSource: "image-sampler",
        rowColorBand: {
            rowIndex: hint.rowIndex,
            yStart: hint.yStart,
            yEnd: hint.yEnd,
            sampleRegion: hint.sampleRegion
        }
    };
}

function rowHasImageSamplerHint(row) {
    return row?.rowColorSource === "image-sampler" && normalizeRowColor(row.rowColor);
}

function flattenedRowColorHints(ocrResults = []) {
    return ocrResults.flatMap((result, imageIndex) =>
        (result?.rowColorHints || [])
            .map(cleanRowColorHint)
            .filter(Boolean)
            .map(hint => ({
                ...hint,
                imageIndex: imageIndex + 1
            }))
    );
}

function rankedRowItems(rows) {
    return rows
        .map((row, index) => ({
            index,
            order: index,
            rank: rowRankValue(row)
        }))
        .filter(item => Number.isInteger(item.rank))
        .sort((a, b) => a.rank - b.rank || a.order - b.order);
}

function applyGlobalRowColorHints(rows, ocrResults = []) {
    const hints = flattenedRowColorHints(ocrResults);
    if (!hints.length) return rows;

    const ranked = rankedRowItems(rows);
    if (!ranked.length || ranked.every(item => rowHasImageSamplerHint(rows[item.index]))) return rows;

    const uniqueRanks = new Set(ranked.map(item => item.rank));
    const minRank = Math.min(...uniqueRanks);
    const maxRank = Math.max(...uniqueRanks);
    const rankSpan = maxRank - minRank + 1;

    if (uniqueRanks.size === ranked.length && rankSpan === hints.length) {
        for (const item of ranked) {
            annotateRowWithColorHint(rows, item, hints[item.rank - minRank]);
        }
        return rows;
    }

    if (ranked.length === hints.length) {
        ranked.forEach((item, index) => annotateRowWithColorHint(rows, item, hints[index]));
    }

    return rows;
}

function applyRowColorHints(rows, ocrResults = []) {
    const annotated = rows.map(row => ({ ...row }));
    const groups = new Map();

    annotated.forEach((row, index) => {
        const imageIndex = rowSourceImageIndex(row, 1);
        if (!groups.has(imageIndex)) groups.set(imageIndex, []);
        groups.get(imageIndex).push({
            index,
            order: index,
            rank: rowRankValue(row)
        });
    });

    for (const [imageIndex, items] of groups.entries()) {
        const hints = flattenedRowColorHints([ocrResults[imageIndex - 1]]);
        if (!hints.length) continue;

        const ranked = items
            .filter(item => Number.isInteger(item.rank))
            .sort((a, b) => a.rank - b.rank || a.order - b.order);
        if (!ranked.length) continue;

        const uniqueRanks = new Set(ranked.map(item => item.rank));
        const minRank = Math.min(...uniqueRanks);
        const maxRank = Math.max(...uniqueRanks);
        const rankSpan = maxRank - minRank + 1;

        if (uniqueRanks.size === ranked.length && rankSpan === hints.length) {
            for (const item of ranked) {
                annotateRowWithColorHint(annotated, item, hints[item.rank - minRank]);
            }
            continue;
        }

        if (ranked.length === hints.length) {
            ranked.forEach((item, index) => annotateRowWithColorHint(annotated, item, hints[index]));
        }
    }

    return applyGlobalRowColorHints(annotated, ocrResults);
}

function normalizeGeminiRow(row, ocrResults, teamConfig = {}) {
    const rank = firstNumber(row.rank, row.placement, row.position, row.place);
    const playerName = escapeNameArtifacts(row.playerName || row.name || row.player || row.ign || row.username);
    if (!rank || rank < 1 || rank > 100 || !playerName) return null;

    const extractedPoints = firstNumber(
        row.points,
        row.eventPoints,
        row.cupPoints,
        row.teamEventPoints,
        row.tePoints
    );
    const points = normalizeEventPointsForRank(rank, extractedPoints);
    const score = firstNumber(
        row.score,
        row.totalScore,
        row.seasonScore,
        row.finalScore,
        row.value
    );

    return classifyPlayer({
        rank,
        placement: rank,
        playerName: playerName.slice(0, 80),
        teamLabel: cleanText(row.teamLabel || row.team || row.teamName || row.side || "", 80),
        teamType: row.teamType || row.classification || row.sideType || "",
        rowColor: normalizeRowColor(row.rowColor || row.row_color || row.teamColor || row.backgroundColor || row.background_color || row.color),
        rowColorConfidence: Number.isFinite(Number(row.rowColorConfidence)) ? Number(row.rowColorConfidence) : null,
        rowColorSource: cleanText(row.rowColorSource || (row.rowColor || row.row_color ? "gemini-row-color" : ""), 60),
        rowColorBand: row.rowColorBand || null,
        points,
        pointsSource: extractedPoints === points ? "gemini" : "rank-table",
        score,
        sourceImage: rowSourceImage(row, ocrResults),
        rawLine: cleanText(row.rawText || row.rawLine || "", 300),
        classificationReason: cleanText(row.classificationReason || row.reason || "", 300),
        confidence: Number.isFinite(Number(row.confidence)) ? Number(row.confidence) : null
    }, teamConfig);
}

function flattenGeminiRows(payload) {
    const rows = [];
    for (const key of ["players", "rows", "rankings", "entries", "standings"]) {
        if (Array.isArray(payload?.[key])) rows.push(...payload[key]);
    }

    if (Array.isArray(payload?.screenshots)) {
        payload.screenshots.forEach((screenshot, screenshotIndex) => {
            for (const key of ["players", "rows", "rankings", "entries", "standings"]) {
                if (!Array.isArray(screenshot?.[key])) continue;
                rows.push(...screenshot[key].map(row => ({
                    ...row,
                    sourceImageIndex: row.sourceImageIndex || screenshot.imageIndex || screenshotIndex + 1
                })));
            }
        });
    }

    return rows;
}

function fallbackRowsFromText(ocrResults, teamConfig = {}) {
    const rows = [];
    for (const result of ocrResults) {
        const lines = normalizeOcrText(result.text).split("\n");
        for (const line of lines) {
            const row = parsePlacementLine(line);
            if (!row) continue;
            rows.push(classifyPlayer({
                ...row,
                sourceImage: path.basename(result.imagePath || ""),
                classificationReason: "fallback raw text parser"
            }, teamConfig));
        }
    }
    return rows;
}

function dedupeRows(rows) {
    const map = new Map();
    for (const row of rows) {
        const key = `${row.rank}:${normalizeForMatch(row.playerName)}`;
        const current = map.get(key);
        if (!current || Number(row.confidence || 0) > Number(current.confidence || 0)) {
            map.set(key, row);
        }
    }
    return [...map.values()].sort((a, b) =>
        Number(a.rank) - Number(b.rank) ||
        String(a.playerName).localeCompare(String(b.playerName))
    );
}

function metadataTeamScores(payload = {}, teamConfig = {}) {
    const metadata = payload.metadata && typeof payload.metadata === "object" ? payload.metadata : {};
    const scoreObject = metadata.teamScores || metadata.team_scores || payload.teamScores || payload.team_scores || {};
    const ownFromMetadata = firstNumber(
        metadata.ownTeamScore,
        metadata.ownTeamPoints,
        metadata.ownTeamEventPoints,
        metadata.own_team_score,
        metadata.own_team_points,
        scoreObject.own,
        scoreObject.ownScore,
        scoreObject.ownTeamScore,
        scoreObject.ownPoints,
        payload.ownTeamScore,
        payload.ownTeamPoints,
        payload.ownTeamEventPoints
    );
    const opponentFromMetadata = firstNumber(
        metadata.opponentTeamScore,
        metadata.opponentTeamPoints,
        metadata.opponentTeamEventPoints,
        metadata.enemyTeamScore,
        metadata.enemyTeamPoints,
        metadata.opponent_team_score,
        metadata.opponent_team_points,
        scoreObject.opponent,
        scoreObject.enemy,
        scoreObject.opponentScore,
        scoreObject.enemyScore,
        scoreObject.opponentTeamScore,
        scoreObject.enemyTeamScore,
        scoreObject.opponentPoints,
        scoreObject.enemyPoints,
        payload.opponentTeamScore,
        payload.opponentTeamPoints,
        payload.opponentTeamEventPoints,
        payload.enemyTeamScore,
        payload.enemyTeamPoints
    );
    if (ownFromMetadata !== null || opponentFromMetadata !== null) {
        return {
            own: ownFromMetadata,
            opponent: opponentFromMetadata,
            rawLine: cleanText(metadata.teamScoreLine || metadata.scoreLine || scoreObject.rawLine || payload.teamScoreLine || "", 200)
        };
    }

    const aliases = teamAliases(teamConfig);
    const teams = Array.isArray(payload.teams) ? payload.teams : [];
    const own = teams.find(team => normalizeTeamType(team.teamType) === "own" || aliasMatches(team.label || team.name, aliases));
    const opponent = teams.find(team => team !== own && normalizeTeamType(team.teamType) === "opponent");

    if (!own && !opponent) return null;
    return {
        own: firstNumber(own?.score, own?.points, own?.teamScore, own?.teamPoints, own?.teamEventPoints, own?.eventPoints),
        opponent: firstNumber(opponent?.score, opponent?.points, opponent?.teamScore, opponent?.teamPoints, opponent?.teamEventPoints, opponent?.eventPoints),
        rawLine: teams.map(team => `${team.label || team.name || "Team"} ${team.score ?? team.points ?? ""}`.trim()).join(" vs ")
    };
}

function scoreNumbersFromLine(line) {
    return (String(line || "").match(/\d[\d\s,.]{0,8}/g) || [])
        .map(parseNumber)
        .filter(number => Number.isFinite(number) && number >= 0 && number <= 10000);
}

function textHasTeamAlias(text, aliases) {
    const normalized = normalizeForMatch(text);
    return aliases.some(alias => {
        const aliasNorm = normalizeForMatch(alias);
        return aliasNorm && normalized.includes(aliasNorm);
    });
}

function teamScoreCandidateAt(lines, index) {
    const line = lines[index] || "";
    if (!/[A-Za-z]/.test(line) || parsePlacementLine(line)) return null;
    if (/winner|touch|continue|rank|reward|garage|season|cup points/i.test(line)) return null;

    const sameLineNumbers = scoreNumbersFromLine(line);
    if (sameLineNumbers.length) {
        return {
            label: cleanText(line.replace(/\d[\d\s,.]{0,8}/g, ""), 80),
            score: sameLineNumbers[0],
            rawLine: line
        };
    }

    for (let offset = 1; offset <= 2 && index + offset < lines.length; offset += 1) {
        const next = lines[index + offset] || "";
        if (/[A-Za-z]/.test(next)) break;
        const numbers = scoreNumbersFromLine(next);
        if (numbers.length) {
            return {
                label: cleanText(line, 80),
                score: numbers[0],
                rawLine: `${line} ${next}`.trim()
            };
        }
    }

    return null;
}

function teamScoresFromText(text, teamConfig = {}) {
    const lines = normalizeOcrText(text).split("\n").map(line => line.trim()).filter(Boolean);
    if (!lines.length) return null;

    const aliases = teamAliases(teamConfig);
    for (const line of lines) {
        if (!textHasTeamAlias(line, aliases)) continue;
        const numbers = scoreNumbersFromLine(line);
        if (numbers.length < 2) continue;
        return {
            own: numbers[0],
            opponent: numbers[1],
            rawLine: cleanText(line, 200)
        };
    }

    const candidates = [];
    for (let index = 0; index < lines.length; index += 1) {
        const candidate = teamScoreCandidateAt(lines, index);
        if (candidate) candidates.push(candidate);
    }

    const own = candidates.find(candidate => textHasTeamAlias(candidate.label, aliases));
    const opponent = candidates.find(candidate => candidate !== own && !textHasTeamAlias(candidate.label, aliases));
    if (!own || !opponent) return null;

    return {
        own: own.score,
        opponent: opponent.score,
        rawLine: cleanText(`${own.rawLine} vs ${opponent.rawLine}`, 200)
    };
}

function parseMetadata(ocrResults, teamConfig = {}) {
    const firstText = ocrResults[0]?.text || "";
    const allText = ocrResults.map(result => result.text || "").filter(Boolean).join("\n");
    const firstLines = firstText.split("\n").map(line => line.trim()).filter(Boolean);
    const title = firstLines.find(line =>
        /[A-Za-z]/.test(line) &&
        !/winner|touch|continue|cup points|season points|garage power/i.test(line) &&
        !parsePlacementLine(line)
    ) || "Team Event";
    const scoreLine = firstLines.find(line => {
        const numbers = line.match(/\d[\d\s,.]{1,8}/g) || [];
        return numbers.length >= 2 && /vs|discord|power|academy|team/i.test(line);
    }) || "";
    const numbers = (scoreLine.match(/\d[\d\s,.]{1,8}/g) || [])
        .map(parseNumber)
        .filter(number => Number.isFinite(number));

    return {
        title: title.slice(0, 120),
        eventName: title.slice(0, 120),
        ownTeamName: teamConfig.name || "",
        extractionModel: ocrResults[0]?.model || "",
        teamScores: teamScoresFromText(allText, teamConfig) || (numbers.length >= 2 ? {
            own: numbers[0],
            opponent: numbers[1],
            rawLine: scoreLine
        } : null)
    };
}

function normalizeGeminiEvent(payload, ocrResults, teamConfig = {}) {
    const flattenedRows = applyRowColorHints(flattenGeminiRows(payload), ocrResults);
    const rows = dedupeRows(
        flattenedRows
            .map(row => normalizeGeminiRow(row, ocrResults, teamConfig))
            .filter(Boolean)
    );
    const fallbackRows = rows.length ? [] : fallbackRowsFromText(ocrResults, teamConfig);
    const players = rows.length ? rows : dedupeRows(fallbackRows);
    const fallback = parseMetadata(ocrResults, teamConfig);
    const metadataObject = payload?.metadata && typeof payload.metadata === "object" ? payload.metadata : {};
    const eventName = cleanText(
        payload?.eventName ||
        payload?.event_name ||
        metadataObject.eventName ||
        metadataObject.title ||
        payload?.title ||
        fallback.title,
        120
    );

    return {
        metadata: {
            ...fallback,
            title: eventName || fallback.title,
            eventName: eventName || fallback.eventName,
            game: cleanText(payload?.game || metadataObject.game || "Hill Climb Racing 2", 80),
            ownTeamName: teamConfig.name || fallback.ownTeamName || "",
            screenshotTypes: Array.isArray(payload?.screenshotTypes) ? payload.screenshotTypes.map(item => cleanText(item, 40)).filter(Boolean) : [],
            teams: Array.isArray(payload?.teams) ? payload.teams : [],
            teamScores: metadataTeamScores(payload, teamConfig) || fallback.teamScores,
            winner: cleanText(metadataObject.winner || payload?.winner || "", 120),
            notes: cleanText(metadataObject.notes || payload?.notes || "", 500),
            extractionModel: ocrResults[0]?.model || fallback.extractionModel || "",
            extractionType: "gemini-flash"
        },
        players,
        stats: summarize(players),
        rawText: [
            `--- Gemini Flash Structured Response (${ocrResults[0]?.model || DEFAULT_GEMINI_MODEL}) ---`,
            ocrResults[0]?.rawGeminiText || "",
            "",
            "--- Visible Text By Image ---",
            ...ocrResults.map((result, index) => `--- Image ${index + 1} ---\n${result.text || ""}`)
        ].join("\n").trim()
    };
}

function summarize(players) {
    const sorted = players.slice().sort((a, b) =>
        Number(a.rank) - Number(b.rank) ||
        String(a.playerName).localeCompare(String(b.playerName))
    );
    const own = sorted.filter(player => player.teamType === "own");
    const opponents = sorted.filter(player => player.teamType !== "own");
    const sum = (items, key) => items.reduce((total, item) => total + (Number(item[key]) || 0), 0);
    const average = (items, key) => items.length
        ? Math.round((sum(items, key) / items.length) * 100) / 100
        : null;
    const opponentRanks = opponents
        .map(player => Number(player.rank))
        .filter(rank => Number.isFinite(rank));
    const topOpponentRank = opponentRanks.length ? Math.min(...opponentRanks) : null;
    const kabPlayers = topOpponentRank === null
        ? []
        : own.filter(player => Number(player.rank) < topOpponentRank);
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
        topOpponentRank,
        kabPlayers: kabPlayers.map(player => player.playerName),
        kabCount: kabPlayers.length,
        buckets,
        opponentsBelowByPlayer: own.map(player => ({
            playerName: player.playerName,
            rank: player.rank,
            opponentsBelow: opponents.filter(opponent => Number(opponent.rank) > Number(player.rank)).length
        }))
    };
}

function parseRaceScreenshots(ocrResults, teamConfig = {}) {
    const structured = ocrResults.find(result => result.structured)?.structured;
    if (structured && typeof structured === "object") {
        return normalizeGeminiEvent(structured, ocrResults, teamConfig);
    }

    const players = dedupeRows(fallbackRowsFromText(ocrResults, teamConfig));
    const metadata = parseMetadata(ocrResults, teamConfig);
    return {
        metadata,
        players,
        stats: summarize(players),
        rawText: ocrResults.map((result, index) => `--- Image ${index + 1} / Gemini visible text ---\n${result.text}`).join("\n\n")
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
    detectRaceRowColors,
    extractRecruitmentApplication,
    extractTeamEventSession,
    normalizeOcrText,
    ocrImage,
    ocrImages,
    parseRaceScreenshots,
    runCommand,
    summarizePlayers: summarize
};
