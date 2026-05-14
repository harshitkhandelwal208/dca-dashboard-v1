const fs = require("fs");
const path = require("path");
const { spawn } = require("child_process");

const DEFAULT_TIMEOUT_MS = 300000;
const DEFAULT_GEMINI_MODEL = "gemini-2.5-flash";
const DEFAULT_MAX_RETRIES = 4;

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
        "- For scores with spaces, output the numeric value without spaces, for example '52 723' becomes 52723.",
        "- Classify each row as own, opponent, or unknown by using team labels, aliases, colors, side of the versus screen, highlighted rows, prefixes like DC, and surrounding layout context.",
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
        "    \"points\": 0,",
        "    \"score\": 0,",
        "    \"sourceImageIndex\": 1,",
        "    \"rawText\": \"string\",",
        "    \"classificationReason\": \"string\",",
        "    \"confidence\": 0.0",
        "  }],",
        "  \"podium\": [{\"rank\":1, \"playerName\":\"string\", \"teamType\":\"own|opponent|unknown\"}],",
        "  \"metadata\": {\"winner\":\"string\", \"ownTeamScore\":0, \"opponentTeamScore\":0, \"notes\":\"string\"},",
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

    const response = await callGemini(parts, settings);
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
    if (["own", "our", "team", "teammate", "ally", "allied"].includes(text)) return "own";
    if (["opponent", "enemy", "opposing", "rival", "other"].includes(text)) return "opponent";
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
    const matchedPlayer = playerAliasMatches(row.playerName, teamConfig);
    const dcPrefix = /\b(?:dc|dca)\s*[\]|]/i.test(row.playerName || "") || /^\s*(?:\[?dc\]?|\|?dc\|)/i.test(row.playerName || "");
    const normalizedType = normalizeTeamType(row.teamType);
    const own = Boolean(matchedAlias || matchedPlayer || dcPrefix || normalizedType === "own");
    const opponent = normalizedType === "opponent" || !own;

    return {
        ...row,
        teamType: own ? "own" : "opponent",
        teamLabel: own
            ? (teamConfig.name || row.teamLabel || "Own team")
            : (row.teamLabel && !matchedAlias ? row.teamLabel : "Opponent"),
        teamColor: own ? "yellow" : "blue",
        classificationSource: matchedAlias
            ? `alias:${matchedAlias}`
            : matchedPlayer
                ? `known-player:${matchedPlayer}`
                : dcPrefix
                    ? "name-prefix"
                    : normalizedType === "own"
                        ? "gemini-own"
                        : opponent
                            ? "gemini-opponent"
                            : "default-opponent"
    };
}

function rowSourceImage(row, ocrResults) {
    const index = Number(row.sourceImageIndex || row.imageIndex || row.sourceImage || 0);
    const result = Number.isInteger(index) && index > 0 ? ocrResults[index - 1] : null;
    return result?.imagePath ? path.basename(result.imagePath) : "";
}

function normalizeGeminiRow(row, ocrResults, teamConfig = {}) {
    const rank = firstNumber(row.rank, row.placement, row.position, row.place);
    const playerName = escapeNameArtifacts(row.playerName || row.name || row.player || row.ign || row.username);
    if (!rank || rank < 1 || rank > 100 || !playerName) return null;

    const points = firstNumber(
        row.points,
        row.eventPoints,
        row.cupPoints,
        row.teamEventPoints,
        row.tePoints
    );
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
        points,
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
    const ownFromMetadata = firstNumber(metadata.ownTeamScore, payload.ownTeamScore);
    const opponentFromMetadata = firstNumber(metadata.opponentTeamScore, payload.opponentTeamScore);
    if (ownFromMetadata !== null || opponentFromMetadata !== null) {
        return {
            own: ownFromMetadata,
            opponent: opponentFromMetadata,
            rawLine: cleanText(metadata.teamScoreLine || payload.teamScoreLine || "", 200)
        };
    }

    const aliases = teamAliases(teamConfig);
    const teams = Array.isArray(payload.teams) ? payload.teams : [];
    const own = teams.find(team => normalizeTeamType(team.teamType) === "own" || aliasMatches(team.label || team.name, aliases));
    const opponent = teams.find(team => team !== own && normalizeTeamType(team.teamType) === "opponent");

    if (!own && !opponent) return null;
    return {
        own: firstNumber(own?.score, own?.points),
        opponent: firstNumber(opponent?.score, opponent?.points),
        rawLine: teams.map(team => `${team.label || team.name || "Team"} ${team.score ?? team.points ?? ""}`.trim()).join(" vs ")
    };
}

function parseMetadata(ocrResults, teamConfig = {}) {
    const firstText = ocrResults[0]?.text || "";
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
        teamScores: numbers.length >= 2 ? {
            own: numbers[0],
            opponent: numbers[1],
            rawLine: scoreLine
        } : null
    };
}

function normalizeGeminiEvent(payload, ocrResults, teamConfig = {}) {
    const rows = dedupeRows(
        flattenGeminiRows(payload)
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
    extractRecruitmentApplication,
    extractTeamEventSession,
    normalizeOcrText,
    ocrImage,
    ocrImages,
    parseRaceScreenshots,
    runCommand,
    summarizePlayers: summarize
};
