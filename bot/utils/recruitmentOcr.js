const fs = require("fs");
const os = require("os");
const path = require("path");
const {
    analyzeRecruitLicenseText,
    extractRecruitmentApplication,
    normalizeOcrText
} = require("./raceOcr");

function cleanValue(value, maxLength = 120) {
    if (value === null || value === undefined) return "";
    if (typeof value === "object") {
        if ("value" in value) return cleanValue(value.value, maxLength);
        if ("text" in value) return cleanValue(value.text, maxLength);
        return "";
    }

    return String(value)
        .replace(/\s+/g, " ")
        .trim()
        .slice(0, maxLength);
}

function normalizeForMatch(value) {
    return String(value || "")
        .toLowerCase()
        .replace(/[\u00ae\u2122]/g, "")
        .replace(/[^a-z0-9]+/g, " ")
        .replace(/\s+/g, " ")
        .trim();
}

function knownTeamNames(config = {}) {
    return [
        ...(config?.recruitment?.teams || []),
        ...(config?.memberCounts?.teams || []).flatMap(team => [team.name, ...(team.aliases || [])])
    ].filter(Boolean);
}

function recruitmentGeminiSettings(config = {}) {
    return {
        ...(config?.spreadsheets || {}),
        geminiApiKey: String(
            config?.recruitment?.geminiApiKey ||
            process.env.RECRUITMENT_GEMINI_API_KEY ||
            process.env.GEMINI_API_KEY ||
            process.env.GOOGLE_GEMINI_API_KEY ||
            ""
        ).trim()
    };
}

function extensionForAttachment(attachment) {
    const fromName = path.extname(attachment?.name || "").toLowerCase();
    if (/^\.(png|jpe?g|webp|gif)$/i.test(fromName)) return fromName;
    const type = String(attachment?.contentType || "").toLowerCase();
    if (type.includes("png")) return ".png";
    if (type.includes("webp")) return ".webp";
    if (type.includes("gif")) return ".gif";
    return ".jpg";
}

async function downloadAttachment(attachment, dir, index, prefix) {
    const response = await fetch(attachment.proxyUrl || attachment.proxyURL || attachment.url);
    const label = prefix === "license" ? "license image" : "team event image";
    if (!response.ok) throw new Error(`Could not download ${label} (${response.status}).`);
    const buffer = Buffer.from(await response.arrayBuffer());
    const filePath = path.join(dir, `${prefix}-${index + 1}${extensionForAttachment(attachment)}`);
    await fs.promises.writeFile(filePath, buffer);
    return filePath;
}

function rawVisibleTextFromStructured(structured = {}, fallback = "") {
    const driver = structured.driverLicense || structured.license || structured.profile || {};
    const imageText = Array.isArray(structured.rawVisibleTextByImage)
        ? structured.rawVisibleTextByImage
            .map(item => cleanValue(item.text, 4000))
            .filter(Boolean)
            .join("\n")
        : "";
    const eventText = [
        ...(Array.isArray(structured.teamEvents) ? structured.teamEvents : []),
        ...(Array.isArray(structured.eventScores) ? structured.eventScores : [])
    ]
        .map(item => cleanValue(item.rawVisibleText, 2000))
        .filter(Boolean)
        .join("\n");
    const checkText = (Array.isArray(structured.imageChecks) ? structured.imageChecks : [])
        .map(item => cleanValue(item.rawVisibleText || item.text, 2000))
        .filter(Boolean)
        .join("\n");

    return normalizeOcrText([
        cleanValue(driver.rawVisibleText, 4000),
        imageText,
        eventText,
        checkText,
        fallback
    ].filter(Boolean).join("\n"));
}

function extractGaragePowerFromText(text) {
    const lines = normalizeOcrText(text).split("\n");
    for (let index = 0; index < lines.length; index += 1) {
        const line = lines[index];
        if (!/garage|power|\bgp\b/i.test(line)) continue;

        const combined = `${line} ${lines[index + 1] || ""}`;
        const match = combined.match(/\d[\d\s,.]{2,}/);
        if (match) return cleanValue(match[0].replace(/\s+/g, " "), 40);
    }
    return "";
}

function normalizeRecruitmentImageKind(value) {
    const text = normalizeForMatch(value);
    if (!text) return "unknown";
    if (/wrong|not hcr|other game|invalid/.test(text)) return "wrong-game";
    if (/driver|license|licence|profile|player profile/.test(text)) return "driver-license";
    if (/team event|event score|standing|standings|podium|score summary|spreadsheet|result/.test(text)) return "team-event-score";
    return "unknown";
}

function classifyRecruitmentVisibleText(text) {
    const normalized = normalizeForMatch(text);
    if (!normalized) return { kind: "unknown", score: 0, reason: "No readable text." };

    const licenseSignals = [
        /\bgarage power\b/,
        /\bcopy my id\b/,
        /\bcup points\b/,
        /\bseason points\b/,
        /\badventurer rank\b|\brank\b.*\bscout\b|\bvanguard\b/,
        /\bbest season\b/,
        /\bbest win streak\b/,
        /\bachievements\b/
    ].filter(pattern => pattern.test(normalized)).length;
    const eventSignals = [
        /\bfinal standings\b/,
        /\bwin bonus\b/,
        /\brewards\b/,
        /\bwinner\b/,
        /\bnext\b/,
        /\bvs\b/,
        /\bteam event\b/,
        /\b\d{1,2}\s+\d{1,3}\s+[a-z0-9].*\d{4,6}\b/
    ].filter(pattern => pattern.test(normalized)).length;

    if (licenseSignals >= 2 && licenseSignals >= eventSignals) {
        return { kind: "driver-license", score: licenseSignals, reason: "Profile/license labels were visible." };
    }

    if (eventSignals >= 2 && eventSignals > licenseSignals) {
        return { kind: "team-event-score", score: eventSignals, reason: "Team-event result labels were visible." };
    }

    if (/\bhill climb racing 2\b|\bcup points\b|\bseason points\b|\bgarage\b|\bstandings\b/.test(normalized)) {
        return { kind: "unknown", score: Math.max(licenseSignals, eventSignals), reason: "HCR2 text was visible but the screen type was unclear." };
    }

    return { kind: "wrong-game", score: 0, reason: "No HCR2 recruitment screenshot markers were visible." };
}

function eventScoreValue(...values) {
    for (const value of values) {
        const clean = cleanValue(value, 80);
        if (clean) return clean;
    }
    return "";
}

function eventLooksValid(event) {
    const eventName = cleanValue(event.eventName || event.name || event.title || event.teamEventName, 120);
    const rank = eventScoreValue(event.rank, event.placement, event.place);
    const eventPoints = eventScoreValue(event.eventPoints, event.points, event.cupPoints, event.teamEventPoints);
    const score = eventScoreValue(event.score, event.totalScore, event.eventScore, event.value);
    const rawText = cleanValue(event.rawVisibleText || event.rawText, 500);
    const textKind = classifyRecruitmentVisibleText([eventName, rank, eventPoints, score, rawText].join(" "));
    return Boolean(eventName && (rank || eventPoints || score) && textKind.kind !== "driver-license");
}

function normalizeEventScores(structured = {}) {
    const candidates = [];
    for (const key of ["teamEvents", "eventScores", "events", "scores"]) {
        if (Array.isArray(structured[key])) candidates.push(...structured[key]);
    }
    if (structured.teamEvent && typeof structured.teamEvent === "object") {
        candidates.push(structured.teamEvent);
    }

    return candidates
        .filter(eventLooksValid)
        .map((event, index) => ({
            eventName: cleanValue(event.eventName || event.name || event.title || event.teamEventName || `Team Event ${index + 1}`, 120),
            rank: eventScoreValue(event.rank, event.placement, event.place),
            eventPoints: eventScoreValue(event.eventPoints, event.points, event.cupPoints, event.teamEventPoints),
            score: eventScoreValue(event.score, event.totalScore, event.eventScore, event.value),
            rawText: cleanValue(event.rawVisibleText || event.rawText, 500)
        }))
        .filter(event => event.eventName || event.rank || event.eventPoints || event.score);
}

function checkImageIndex(check, fallback) {
    const parsed = Number(check?.imageIndex ?? check?.index ?? check?.image ?? fallback);
    return Number.isInteger(parsed) && parsed > 0 ? parsed : fallback;
}

function textByImage(structured = {}) {
    const map = new Map();
    for (const item of Array.isArray(structured.rawVisibleTextByImage) ? structured.rawVisibleTextByImage : []) {
        const index = checkImageIndex(item, map.size + 1);
        const text = cleanValue(item.text || item.rawVisibleText || item.rawText, 4000);
        if (text) map.set(index, text);
    }

    for (const item of Array.isArray(structured.imageChecks) ? structured.imageChecks : []) {
        const index = checkImageIndex(item, map.size + 1);
        const text = cleanValue(item.rawVisibleText || item.text || item.rawText, 4000);
        if (text && !map.has(index)) map.set(index, text);
    }

    return map;
}

function classifyRecruitmentImages(extraction, imageInputs = []) {
    const structured = extraction?.structured && typeof extraction.structured === "object" ? extraction.structured : {};
    const checks = Array.isArray(structured.imageChecks) ? structured.imageChecks : [];
    const textMap = textByImage(structured);

    return imageInputs.map((input, index) => {
        const imageIndex = index + 1;
        const check = checks.find(item => checkImageIndex(item, imageIndex) === imageIndex) || {};
        const text = textMap.get(imageIndex) || cleanValue(check.rawVisibleText || check.text || "", 4000);
        const inferred = classifyRecruitmentVisibleText(text);
        const checkKind = normalizeRecruitmentImageKind(check.kind || check.type || check.imageType || check.classification);
        const kind = checkKind !== "unknown" ? checkKind : inferred.kind;
        const valid = kind === "driver-license" || kind === "team-event-score";
        return {
            imageIndex,
            attachment: input.attachment || null,
            originalField: input.originalField || "",
            kind,
            valid: Boolean(check.isValidForRecruitment ?? check.valid ?? valid) && valid,
            reason: cleanValue(check.reason || inferred.reason, 300),
            rawText: text,
            confidence: Number.isFinite(Number(check.confidence)) ? Number(check.confidence) : inferred.score
        };
    });
}

function analysisLooksLikeDriverLicense(analysis) {
    if (!analysis) return false;
    const textKind = classifyRecruitmentVisibleText(analysis.rawText || "");
    return Boolean(
        textKind.kind === "driver-license" ||
        (analysis.inGameName && analysis.garagePower && textKind.kind !== "team-event-score")
    );
}

async function classifyRecruitmentTicketScreenshots(ticket, config) {
    const licenseAttachments = (ticket.licenseAttachments || []).filter(attachment => attachment?.url);
    const eventAttachments = (ticket.eventAttachments || []).filter(attachment => attachment?.url);
    const allAttachments = [
        ...licenseAttachments.map(attachment => ({ attachment, originalField: "licenseAttachments" })),
        ...eventAttachments.map(attachment => ({ attachment, originalField: "eventAttachments" }))
    ];

    if (!allAttachments.length) {
        return {
            licenseAttachments: [],
            eventAttachments: [],
            invalidAttachments: [],
            classifications: [],
            analysis: null
        };
    }

    const tempDir = await fs.promises.mkdtemp(path.join(os.tmpdir(), "dca-recruitment-classify-"));
    try {
        const imageInputs = [];
        for (let index = 0; index < allAttachments.length; index += 1) {
            const item = allAttachments[index];
            const filePath = await downloadAttachment(item.attachment, tempDir, index, "recruitment");
            imageInputs.push({
                path: filePath,
                kind: "recruitment-upload",
                label: `Recruitment Image ${index + 1}`,
                attachment: item.attachment,
                originalField: item.originalField
            });
        }

        const extraction = await extractRecruitmentApplication(imageInputs, {
            ...recruitmentGeminiSettings(config),
            acceptedTeam: ticket.team || "",
            applicantDiscordId: ticket.applicantId || "",
            knownTeams: knownTeamNames(config)
        });
        const analysis = normalizeRecruitmentExtraction(extraction, ticket, config);
        const classifications = classifyRecruitmentImages(extraction, imageInputs);
        let nextLicense = classifications
            .filter(item => item.valid && item.kind === "driver-license")
            .map(item => item.attachment)
            .filter(Boolean);
        const nextEvents = classifications
            .filter(item => item.valid && item.kind === "team-event-score")
            .map(item => item.attachment)
            .filter(Boolean);

        if (!nextLicense.length && analysisLooksLikeDriverLicense(analysis)) {
            const fallback = classifications.find(item => item.originalField === "licenseAttachments" && item.attachment) ||
                classifications.find(item => item.attachment);
            if (fallback?.attachment) nextLicense = [fallback.attachment];
        }

        return {
            licenseAttachments: nextLicense,
            eventAttachments: nextEvents,
            invalidAttachments: classifications
                .filter(item => !item.valid)
                .map(item => ({
                    ...(item.attachment || {}),
                    reason: item.reason,
                    detectedKind: item.kind
                }))
                .filter(Boolean),
            classifications,
            analysis
        };
    } finally {
        await fs.promises.rm(tempDir, { recursive: true, force: true }).catch(() => null);
    }
}

function normalizeRecruitmentExtraction(extraction, ticket, config) {
    const structured = extraction.structured && typeof extraction.structured === "object" ? extraction.structured : {};
    const driver = structured.driverLicense || structured.license || structured.profile || {};
    const rawText = rawVisibleTextFromStructured(structured, extraction.rawResponseText);
    const fallback = analyzeRecruitLicenseText(rawText, config);
    const previousTeam = cleanValue(
        driver.previousTeam ||
        driver.sourceTeam ||
        driver.currentTeam ||
        driver.teamName ||
        structured.previousTeam,
        120
    ) || fallback.sourceTeam || "";

    return {
        discordId: ticket.applicantId || "",
        inGameName: cleanValue(
            driver.playerName ||
            driver.inGameName ||
            driver.ign ||
            structured.playerName,
            80
        ) || fallback.inGameName || "",
        sourceTeam: previousTeam,
        previousTeam,
        acceptedTeam: ticket.team || "",
        garagePower: cleanValue(driver.garagePower || driver.garage_power || structured.garagePower, 60) || extractGaragePowerFromText(rawText),
        eventScores: normalizeEventScores(structured),
        rawText,
        rawGeminiText: extraction.rawResponseText || "",
        rawGeminiJson: structured,
        model: extraction.model || "",
        error: ""
    };
}

async function analyzeRecruitmentLicense(ticket, config) {
    const licenseAttachments = (ticket.licenseAttachments || []).filter(attachment => attachment?.url);
    const eventAttachments = (ticket.eventAttachments || []).filter(attachment => attachment?.url);
    const base = {
        discordId: ticket.applicantId || "",
        inGameName: "",
        sourceTeam: "",
        previousTeam: "",
        acceptedTeam: ticket.team || "",
        garagePower: "",
        eventScores: [],
        rawText: "",
        rawGeminiText: "",
        rawGeminiJson: null,
        model: "",
        error: ""
    };

    if (!licenseAttachments.length) {
        return { ...base, error: "No license image was attached to this ticket." };
    }

    const tempDir = await fs.promises.mkdtemp(path.join(os.tmpdir(), "dca-license-"));
    try {
        const imageInputs = [];
        for (let index = 0; index < licenseAttachments.length; index += 1) {
            const filePath = await downloadAttachment(licenseAttachments[index], tempDir, index, "license");
            imageInputs.push({
                path: filePath,
                kind: "driver-license",
                label: `Driver License ${index + 1}`
            });
        }

        for (let index = 0; index < eventAttachments.length; index += 1) {
            const filePath = await downloadAttachment(eventAttachments[index], tempDir, index, "team-event");
            imageInputs.push({
                path: filePath,
                kind: "team-event-score",
                label: `Team Event Scores ${index + 1}`
            });
        }

        const extraction = await extractRecruitmentApplication(imageInputs, {
            ...recruitmentGeminiSettings(config),
            acceptedTeam: ticket.team || "",
            applicantDiscordId: ticket.applicantId || "",
            knownTeams: knownTeamNames(config)
        });
        const analysis = normalizeRecruitmentExtraction(extraction, ticket, config);
        const classifications = classifyRecruitmentImages(extraction, imageInputs);
        const hasValidLicense = classifications.some(item => item.valid && item.kind === "driver-license") ||
            analysisLooksLikeDriverLicense(analysis);
        const hasValidEvent = classifications.some(item => item.valid && item.kind === "team-event-score") ||
            analysis.eventScores.length > 0;
        const validationErrors = [
            hasValidLicense ? "" : "Driver's license image was not recognized as a valid HCR2 profile/license screenshot.",
            eventAttachments.length && !hasValidEvent ? "Team event score upload was not recognized as a valid HCR2 team-event result/standings screenshot." : ""
        ].filter(Boolean);
        if (validationErrors.length) {
            analysis.error = [analysis.error, ...validationErrors].filter(Boolean).join(" ");
        }

        return {
            ...base,
            ...analysis
        };
    } catch (error) {
        return { ...base, error: error.message };
    } finally {
        await fs.promises.rm(tempDir, { recursive: true, force: true }).catch(() => null);
    }
}

module.exports = {
    analyzeRecruitmentLicense,
    classifyRecruitmentTicketScreenshots,
    classifyRecruitmentVisibleText,
    normalizeRecruitmentImageKind
};
