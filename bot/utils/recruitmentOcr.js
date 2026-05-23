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

function knownTeamNames(config = {}) {
    return [
        ...(config?.recruitment?.teams || []),
        ...(config?.memberCounts?.teams || []).flatMap(team => [team.name, ...(team.aliases || [])])
    ].filter(Boolean);
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

    return normalizeOcrText([
        cleanValue(driver.rawVisibleText, 4000),
        imageText,
        eventText,
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

function eventScoreValue(...values) {
    for (const value of values) {
        const clean = cleanValue(value, 80);
        if (clean) return clean;
    }
    return "";
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
        .map((event, index) => ({
            eventName: cleanValue(event.eventName || event.name || event.title || event.teamEventName || `Team Event ${index + 1}`, 120),
            rank: eventScoreValue(event.rank, event.placement, event.place),
            eventPoints: eventScoreValue(event.eventPoints, event.points, event.cupPoints, event.teamEventPoints),
            score: eventScoreValue(event.score, event.totalScore, event.eventScore, event.value),
            rawText: cleanValue(event.rawVisibleText || event.rawText, 500)
        }))
        .filter(event => event.eventName || event.rank || event.eventPoints || event.score);
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
            ...config?.spreadsheets,
            acceptedTeam: ticket.team || "",
            applicantDiscordId: ticket.applicantId || "",
            knownTeams: knownTeamNames(config)
        });

        return {
            ...base,
            ...normalizeRecruitmentExtraction(extraction, ticket, config)
        };
    } catch (error) {
        return { ...base, error: error.message };
    } finally {
        await fs.promises.rm(tempDir, { recursive: true, force: true }).catch(() => null);
    }
}

module.exports = {
    analyzeRecruitmentLicense
};
