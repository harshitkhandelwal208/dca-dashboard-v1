const fs = require("fs");
const path = require("path");
const {
    AttachmentBuilder,
    EmbedBuilder,
    PermissionFlagsBits
} = require("discord.js");
const { generateSpreadsheetArtifacts } = require("./calcSpreadsheet");
const { loadDashboardConfig } = require("./dashboardConfig");
const { logAction } = require("./logStore");
const { ocrImages, parseRaceScreenshots, summarizePlayers } = require("./raceOcr");
const {
    generatePeriodReport,
    reportAttachments
} = require("./spreadsheetReports");
const {
    cleanupSpreadsheetRawData,
    getReportEmission,
    getSpreadsheetSession,
    latestSpreadsheetSession,
    listSpreadsheetSessions,
    markReportEmitted,
    saveSpreadsheetSession,
    updateSpreadsheetSession
} = require("./spreadsheetStore");

const IMAGE_EXT_RE = /\.(png|jpe?g|webp|gif)$/i;
const timers = new Map();

function displayTag(user) {
    return user?.tag || user?.username || user?.id || "unknown";
}

function normalize(value) {
    return String(value || "")
        .toLowerCase()
        .replace(/[^a-z0-9]+/g, " ")
        .trim();
}

function isImageAttachment(attachment) {
    const contentType = String(attachment.contentType || "");
    return contentType.startsWith("image/") || IMAGE_EXT_RE.test(attachment.name || attachment.url || "");
}

function extractImageAttachments(message) {
    return [...message.attachments.values()]
        .filter(isImageAttachment)
        .slice(0, 20)
        .map(attachment => ({
            id: attachment.id,
            messageId: message.id,
            name: attachment.name || "screenshot",
            url: attachment.url,
            proxyUrl: attachment.proxyURL || "",
            contentType: attachment.contentType || "",
            size: attachment.size || 0,
            createdAt: message.createdAt.toISOString()
        }));
}

function enabledSpreadsheetTeams(config) {
    if (!config?.spreadsheets?.enabled) return [];
    return (config.spreadsheets.teams || []).filter(team => team.enabled && team.monitoredChannelId);
}

function findSpreadsheetTeam(config, value) {
    const text = normalize(value);
    return (config?.spreadsheets?.teams || []).find(team =>
        team.id === value ||
        normalize(team.id) === text ||
        normalize(team.name) === text ||
        (team.ownTeamAliases || []).some(alias => normalize(alias) === text)
    ) || null;
}

function teamForMessage(config, message) {
    return enabledSpreadsheetTeams(config).find(team => team.monitoredChannelId === message.channelId) || null;
}

function isAdmin(member) {
    return Boolean(member?.permissions?.has(PermissionFlagsBits.Administrator));
}

function canAccessSpreadsheetTeam(member, teamConfig) {
    if (!member) return false;
    if (isAdmin(member)) return true;
    return Boolean(teamConfig?.accessRoleId && member.roles?.cache?.has(teamConfig.accessRoleId));
}

async function requireSpreadsheetAccess(interaction, teamConfig) {
    const member = interaction.member?.roles?.cache
        ? interaction.member
        : await interaction.guild?.members.fetch(interaction.user.id).catch(() => null);
    if (canAccessSpreadsheetTeam(member, teamConfig)) return true;

    await interaction.editReply("You do not have access to this team's spreadsheet data.");
    return false;
}

function sessionWindowMs(config) {
    return Math.max(1, Number(config?.spreadsheets?.sessionWindowMinutes || 1)) * 60 * 1000;
}

function sessionTimerKey(session) {
    return `${session.teamId}:${session.channelId}:${session.authorId}`;
}

function safeFileName(value, fallback = "image") {
    const clean = String(value || fallback)
        .replace(/[/\\?%*:|"<>]/g, "-")
        .replace(/\s+/g, "-")
        .replace(/^-+|-+$/g, "")
        .slice(0, 90);
    return clean || fallback;
}

function outputBaseDir(teamId, sessionId) {
    return path.join(__dirname, "..", "data", "spreadsheets", safeFileName(teamId, "team"), safeFileName(sessionId, "session"));
}

function extensionForAttachment(attachment, index) {
    const fromName = path.extname(attachment.name || "").toLowerCase();
    if (/^\.(png|jpe?g|webp|gif)$/i.test(fromName)) return fromName;
    const type = String(attachment.contentType || "").toLowerCase();
    if (type.includes("png")) return ".png";
    if (type.includes("webp")) return ".webp";
    if (type.includes("gif")) return ".gif";
    return index === 0 ? ".jpg" : ".png";
}

async function downloadImages(session, outputDir) {
    const sourceDir = path.join(outputDir, "source");
    await fs.promises.mkdir(sourceDir, { recursive: true });
    const files = [];

    for (let index = 0; index < (session.images || []).length; index += 1) {
        const image = session.images[index];
        const response = await fetch(image.proxyUrl || image.url);
        if (!response.ok) throw new Error(`Could not download ${image.name || "image"} (${response.status}).`);
        const buffer = Buffer.from(await response.arrayBuffer());
        const filePath = path.join(sourceDir, `${String(index + 1).padStart(2, "0")}-${safeFileName(image.name, "screenshot")}${extensionForAttachment(image, index)}`);
        await fs.promises.writeFile(filePath, buffer);
        files.push(filePath);
    }

    return files;
}

function applyCorrections(parsed, corrections = []) {
    const metadata = { ...(parsed.metadata || {}) };
    const players = (parsed.players || []).map(player => ({ ...player }));

    for (const correction of corrections) {
        if (correction.field === "event_name") {
            const value = String(correction.value || "").slice(0, 120);
            metadata.title = value || metadata.title || "Team Event";
            metadata.eventName = metadata.title;
            continue;
        }

        const row = Number(correction.row);
        const player = players.find(item => item.rank === row) || players[row - 1];
        if (!player) continue;

        const value = correction.value;
        if (correction.field === "player_name") player.playerName = String(value || "").slice(0, 80);
        if (correction.field === "team_name") player.teamLabel = String(value || "").slice(0, 80);
        if (correction.field === "placement") {
            const next = Number.parseInt(value, 10);
            if (Number.isInteger(next) && next > 0 && next <= 80) {
                player.rank = next;
                player.placement = next;
            }
        }
        if (correction.field === "points") {
            const next = parseScoreValue(value);
            player.points = Number.isFinite(next) ? next : null;
        }
        if (correction.field === "score") {
            const next = parseScoreValue(value);
            player.score = Number.isFinite(next) ? next : null;
        }
        if (correction.field === "team_type") {
            const own = /^(own|yellow|team)$/i.test(String(value || ""));
            player.teamType = own ? "own" : "opponent";
            player.teamColor = own ? "yellow" : "blue";
            if (!own) player.teamLabel = "Opponent";
            player.classificationSource = "staff-correction";
        }
    }

    players.sort((a, b) => a.rank - b.rank || a.playerName.localeCompare(b.playerName));
    return {
        ...parsed,
        metadata,
        players,
        stats: summarizePlayers(players)
    };
}

function parseScoreValue(value) {
    const text = String(value || "").trim().replace(/\s+/g, "");
    if (!text) return NaN;
    if (/^\d+[,.]\d+$/.test(text)) return Number.parseFloat(text.replace(",", "."));
    return Number.parseInt(text.replace(/\D/g, ""), 10);
}

function playerKey(name) {
    return String(name || "")
        .toLowerCase()
        .replace(/[^a-z0-9]+/g, " ")
        .trim();
}

function processedDate(session) {
    const date = new Date(session.processedAt || session.updatedAt || session.createdAt || Date.now());
    return Number.isNaN(date.getTime()) ? new Date() : date;
}

function eventNameForSession(session) {
    return String(
        session.metadata?.eventName ||
        session.metadata?.title ||
        session.teamEventName ||
        session.eventName ||
        "Team Event"
    ).replace(/\s+/g, " ").trim();
}

function uniqueNames(values) {
    const map = new Map();
    for (const value of values || []) {
        const text = String(value || "").replace(/\s+/g, " ").trim();
        const key = playerKey(text);
        if (key && !map.has(key)) map.set(key, text);
    }
    return [...map.values()];
}

async function ownPlayerRosterForTeam(teamId, excludeSessionId = "") {
    const sessions = await listSpreadsheetSessions({ teamId });
    const names = [];

    for (const historical of sessions) {
        if (historical.id === excludeSessionId || historical.status !== "processed") continue;
        names.push(...(historical.attendance?.roster || []));
        for (const player of historical.players || []) {
            if (player.teamType === "own") names.push(player.playerName);
        }
    }

    return uniqueNames(names);
}

async function teamConfigWithKnownRoster(teamConfig, currentSessionId = "") {
    const learnedRoster = await ownPlayerRosterForTeam(teamConfig.id, currentSessionId);
    return {
        ...teamConfig,
        ownPlayerAliases: uniqueNames([
            ...(teamConfig.ownPlayerAliases || []),
            ...learnedRoster
        ])
    };
}

async function attachAttendanceSnapshot(session) {
    const sessions = await listSpreadsheetSessions({ teamId: session.teamId });
    const roster = new Map();

    for (const historical of sessions) {
        if (historical.status !== "processed" || historical.id === session.id) continue;
        for (const player of historical.players || []) {
            if (player.teamType !== "own") continue;
            const key = playerKey(player.playerName);
            if (key && !roster.has(key)) roster.set(key, player.playerName);
        }
    }

    for (const player of session.players || []) {
        if (player.teamType !== "own") continue;
        const key = playerKey(player.playerName);
        if (key && !roster.has(key)) roster.set(key, player.playerName);
    }

    const currentOwn = new Set((session.players || [])
        .filter(player => player.teamType === "own")
        .map(player => playerKey(player.playerName))
        .filter(Boolean));
    const missingPlayers = [...roster.entries()]
        .filter(([key]) => !currentOwn.has(key))
        .map(([, name]) => name)
        .sort((a, b) => a.localeCompare(b));

    return {
        ...session,
        attendance: {
            roster: [...roster.values()].sort((a, b) => a.localeCompare(b)),
            attendedPlayers: (session.players || [])
                .filter(player => player.teamType === "own")
                .map(player => player.playerName),
            missingPlayers,
            zeroScorePolicy: "Players missing this event are recorded as 0 in event summaries and period rollups."
        }
    };
}

async function rebuildArtifacts(session, config, teamConfig) {
    const outputDir = outputBaseDir(session.teamId, session.id);
    const artifacts = await generateSpreadsheetArtifacts({
        ...session,
        teamName: teamConfig.name
    }, outputDir, {
        outputFormat: config.spreadsheets.outputFormat,
        libreOfficePath: config.spreadsheets.libreOfficePath
    });

    return {
        ...session.outputs,
        ...artifacts
    };
}

async function processSpreadsheetSession(client, sessionId, options = {}) {
    const config = await loadDashboardConfig();
    let session = await getSpreadsheetSession(sessionId);
    if (!session) throw new Error("Spreadsheet session not found.");
    const teamConfig = findSpreadsheetTeam(config, session.teamId);
    if (!teamConfig) throw new Error("Spreadsheet team config was not found.");
    if (session.status === "processing") return session;
    const extractionTeamConfig = await teamConfigWithKnownRoster(teamConfig, session.id);

    session = await updateSpreadsheetSession(session.id, {
        status: "processing",
        error: ""
    });

    try {
        let parsed;
        let ocrResults = session.ocrResults || [];
        let imagePaths = [];
        if (!ocrResults.length || options.rerunOcr) {
            const outputDir = outputBaseDir(session.teamId, session.id);
            imagePaths = await downloadImages(session, outputDir);
            ocrResults = await ocrImages(imagePaths, {
                ...config.spreadsheets,
                teamConfig: extractionTeamConfig
            });
            parsed = parseRaceScreenshots(ocrResults, extractionTeamConfig);
        } else {
            parsed = parseRaceScreenshots(ocrResults, extractionTeamConfig);
        }

        parsed = applyCorrections(parsed, session.corrections || []);
        session = {
            ...session,
            status: "processed",
            processedAt: new Date().toISOString(),
            ocrResults,
            metadata: parsed.metadata,
            teamEventName: parsed.metadata?.eventName || parsed.metadata?.title || "",
            players: parsed.players,
            stats: parsed.stats,
            rawOcrText: parsed.rawText,
            rawGeminiText: ocrResults[0]?.rawGeminiText || "",
            rawGeminiJson: ocrResults[0]?.structured || null,
            outputs: {}
        };
        session = await attachAttendanceSnapshot(session);
        session.outputs = await rebuildArtifacts(session, config, teamConfig);
        const saved = await saveSpreadsheetSession(session);

        await logAction(client, {
            type: "system",
            title: "Race Spreadsheet Generated",
            message: `Generated spreadsheet session **${saved.id}** for **${teamConfig.name}** with ${saved.players.length} parsed players.`,
            guildId: saved.guildId,
            actorId: saved.authorId,
            actorTag: saved.authorTag,
            metadata: {
                teamId: saved.teamId,
                sessionId: saved.id,
                players: saved.players.length
            }
        }).catch(() => null);

        return saved;
    } catch (error) {
        await removeSourceImages(session).catch(() => null);
        await updateSpreadsheetSession(session.id, {
            status: "failed",
            error: error.message
        });
        throw error;
    }
}

async function rebuildSpreadsheetSession(client, sessionId) {
    const config = await loadDashboardConfig();
    const session = await getSpreadsheetSession(sessionId);
    if (!session) throw new Error("Spreadsheet session not found.");
    const teamConfig = findSpreadsheetTeam(config, session.teamId);
    if (!teamConfig) throw new Error("Spreadsheet team config was not found.");
    const extractionTeamConfig = await teamConfigWithKnownRoster(teamConfig, session.id);
    const parsed = applyCorrections(
        session.ocrResults?.length
            ? parseRaceScreenshots(session.ocrResults, extractionTeamConfig)
            : {
                metadata: session.metadata || {},
                players: session.players || [],
                stats: session.stats || {},
                rawText: session.rawOcrText || ""
            },
        session.corrections || []
    );
    const next = {
        ...session,
        ...parsed,
        status: "processed",
        teamEventName: parsed.metadata?.eventName || parsed.metadata?.title || session.teamEventName || "",
        outputs: {}
    };
    const withAttendance = await attachAttendanceSnapshot(next);
    withAttendance.outputs = await rebuildArtifacts(withAttendance, config, teamConfig);
    const saved = await saveSpreadsheetSession(withAttendance);

    await logAction(client, {
        type: "system",
        title: "Race Spreadsheet Rebuilt",
        message: `Rebuilt spreadsheet session **${saved.id}** for **${teamConfig.name}**.`,
        guildId: saved.guildId,
        actorId: saved.authorId,
        actorTag: saved.authorTag,
        metadata: { teamId: saved.teamId, sessionId: saved.id }
    }).catch(() => null);

    return saved;
}

async function correctSpreadsheetSession(client, sessionId, correction) {
    const session = await getSpreadsheetSession(sessionId);
    if (!session) throw new Error("Spreadsheet session not found.");
    const corrections = [
        ...(session.corrections || []),
        {
            ...correction,
            createdAt: new Date().toISOString()
        }
    ];
    await updateSpreadsheetSession(session.id, { corrections });
    return rebuildSpreadsheetSession(client, session.id);
}

function buildSummaryText(session) {
    const stats = session.stats || {};
    const metadata = session.metadata || {};
    const kabPlayers = sessionKabPlayers(session);
    const bestOwn = (session.players || [])
        .filter(player => player.teamType === "own")
        .sort((a, b) => (Number(a.rank) || 999) - (Number(b.rank) || 999))
        .slice(0, 8);
    const lines = [
        `**${metadata.title || "Race Session"}**`,
        `Session: \`${session.id}\``,
        `Team: **${metadata.ownTeamName || session.teamName || session.teamId}**`,
        `Players parsed: **${stats.totalPlayers || 0}** (${stats.ownPlayers || 0} own, ${stats.opponents || 0} opponents)`,
        `Points: own **${stats.ownPoints || 0}** vs opponents **${stats.opponentPoints || 0}**`,
        `Scores: own **${stats.ownScore || 0}** vs opponents **${stats.opponentScore || 0}**`,
        `Top 10: own **${stats.ownTop10 || 0}** vs opponents **${stats.opponentTop10 || 0}**`,
        `#KAB this event: **${kabPlayers.length}** (${kabPlayers.map(player => player.playerName).join(", ") || "none"})`
    ];

    if (bestOwn.length) {
        lines.push("");
        lines.push("Best own players:");
        lines.push(bestOwn.map(player =>
            `#${player.rank} ${player.playerName} - ${player.points ?? player.score ?? 0} point(s), ${player.score ?? "no score"} score`
        ).join("\n"));
    }

    if (stats.podium?.length) {
        lines.push(`Podium: ${stats.podium.map(player => `#${player.rank} ${player.playerName}`).join(", ")}`);
    }
    lines.push("");
    lines.push("Weekly and monthly reports are rebuilt from processed sessions. Missing players in a period receive 0 for that event, and #KAB counts events where a player ranked above every opponent.");
    if (session.outputs?.conversionError) {
        lines.push(`LibreOffice conversion fallback: ${session.outputs.conversionError}`);
    }

    return lines.join("\n");
}

function sessionKabPlayers(session) {
    const players = session.players || [];
    const own = players.filter(player => player.teamType === "own");
    const opponents = players.filter(player => player.teamType !== "own");
    if (!own.length) return [];

    const opponentRanks = opponents
        .map(player => Number(player.rank))
        .filter(rank => Number.isFinite(rank));
    const topOpponentRank = opponentRanks.length ? Math.min(...opponentRanks) : null;

    return own.filter(player => {
        const rank = Number(player.rank);
        if (Number.isFinite(rank) && topOpponentRank !== null) return rank < topOpponentRank;
        return false;
    });
}

function buildSummaryEmbed(session) {
    return new EmbedBuilder()
        .setTitle("Race Spreadsheet Summary")
        .setDescription(buildSummaryText(session).slice(0, 4000))
        .setColor(0x0f766e)
        .setTimestamp(new Date(session.processedAt || session.updatedAt || Date.now()));
}

function existingFile(filePath) {
    return filePath && fs.existsSync(filePath) ? filePath : "";
}

function attachmentFor(filePath, name = "") {
    if (!existingFile(filePath)) return null;
    return new AttachmentBuilder(filePath, { name: name || path.basename(filePath) });
}

function chunkFiles(files, size = 10) {
    const chunks = [];
    for (let index = 0; index < files.length; index += size) {
        chunks.push(files.slice(index, index + size));
    }
    return chunks;
}

async function sendFileChunks(channel, content, files) {
    const chunks = chunkFiles(files.filter(Boolean), 10);
    if (!chunks.length) {
        return [await channel.send({ content, allowedMentions: { parse: [] } })];
    }

    const messages = [];
    for (let index = 0; index < chunks.length; index += 1) {
        messages.push(await channel.send({
            content: index === 0 ? content : `Additional generated files for the same event output.`,
            files: chunks[index],
            allowedMentions: { parse: [] }
        }));
    }
    return messages;
}

async function removeSourceImages(session) {
    const outputDir = outputBaseDir(session.teamId, session.id);
    await fs.promises.rm(path.join(outputDir, "source"), { recursive: true, force: true }).catch(() => null);
}

async function sendPeriodReport(client, teamConfig, period, options = {}) {
    const report = await generatePeriodReport(teamConfig, period, {
        anchorDate: options.anchorDate || new Date()
    });
    if (!report) return { skipped: true, reason: "no-sessions" };

    if (!options.force) {
        const existing = await getReportEmission(teamConfig.id, period, report.periodKey);
        if (existing) return { skipped: true, reason: "already-emitted", report };
    }

    const channelId = teamConfig.outputChannelId;
    if (!channelId) return { skipped: true, reason: "missing-output-channel", report };

    const channel = await client.channels.fetch(channelId).catch(() => null);
    if (!channel?.isTextBased?.()) return { skipped: true, reason: "output-channel-unavailable", report };

    const attachments = reportAttachments(report);
    const message = await channel.send({
        content: period === "weekly"
            ? `Weekly report for **${teamConfig.name}** - **${eventNameForSession({ metadata: { eventName: report.eventName } })}** (${report.periodLabel}).`
            : `Monthly report for **${teamConfig.name}** (${report.periodLabel}).`,
        files: attachments,
        allowedMentions: { parse: [] }
    });
    await markReportEmitted(report, {
        reason: options.reason || "",
        channelId,
        messageId: message.id
    });

    return { report, message };
}

async function previousProcessedSession(teamConfig, session) {
    const currentDate = processedDate(session);
    const sessions = (await listSpreadsheetSessions({ teamId: teamConfig.id }))
        .filter(item => item.status === "processed" && item.id !== session.id)
        .sort((a, b) => processedDate(b) - processedDate(a));
    return sessions.find(item => processedDate(item) <= currentDate) || sessions[0] || null;
}

async function weeklyReportAnchorForEventChange(teamConfig, session) {
    const currentName = normalize(eventNameForSession(session));
    if (!currentName) return false;

    const previous = await previousProcessedSession(teamConfig, session);
    if (!previous) return false;

    return normalize(eventNameForSession(previous)) !== currentName ? previous : null;
}

async function postAutomaticReports(client, teamConfig, session) {
    if (!teamConfig?.outputChannelId) return [];
    const posted = [];

    const reportAnchor = await weeklyReportAnchorForEventChange(teamConfig, session);
    if (reportAnchor) {
        const result = await sendPeriodReport(client, teamConfig, "weekly", {
            anchorDate: processedDate(reportAnchor),
            force: false,
            reason: `event-name-change:${eventNameForSession(reportAnchor)}->${eventNameForSession(session)}`
        }).catch(error => {
            console.error("Automatic weekly report failed:", error.message);
            return null;
        });
        if (result?.message) posted.push(result);
    }

    return posted;
}

async function sendSessionOutput(client, session) {
    const config = await loadDashboardConfig();
    const teamConfig = findSpreadsheetTeam(config, session.teamId);
    try {
        const channelId = teamConfig?.outputChannelId || session.channelId;
        const channel = await client.channels.fetch(channelId).catch(() => null);
        if (!channel?.isTextBased?.()) return null;

        const spreadsheet = attachmentFor(session.outputs?.spreadsheetPath);
        const spreadsheetImage = attachmentFor(session.outputs?.spreadsheetImagePath);
        const chartImage = attachmentFor(session.outputs?.chartPath);
        const files = [
            spreadsheet,
            spreadsheetImage,
            chartImage
        ].filter(Boolean);
        const content = teamConfig?.outputChannelId
            ? `Final team-event output for **${eventNameForSession(session)}** (\`${session.id}\`).`
            : `Spreadsheet session \`${session.id}\` was processed. Configure an output channel to receive automatic final files and reports.`;

        const messages = await sendFileChunks(channel, content, files);
        if (teamConfig) {
            await postAutomaticReports(client, teamConfig, session);
        }

        return messages[0] || null;
    } finally {
        await removeSourceImages(session);
    }
}

async function previewSpreadsheetSession(sessionId, options = {}) {
    const config = await loadDashboardConfig();
    const session = await getSpreadsheetSession(sessionId);
    if (!session) throw new Error("Spreadsheet session not found.");
    const teamConfig = findSpreadsheetTeam(config, session.teamId);
    if (!teamConfig) throw new Error("Spreadsheet team config was not found.");
    const extractionTeamConfig = await teamConfigWithKnownRoster(teamConfig, session.id);

    let ocrResults = session.ocrResults || [];
    let imagePaths = [];
    let downloadedImages = false;
    try {
        if (!ocrResults.length || options.rerunGemini) {
            const outputDir = outputBaseDir(session.teamId, session.id);
            imagePaths = await downloadImages(session, outputDir);
            downloadedImages = true;
            ocrResults = await ocrImages(imagePaths, {
                ...config.spreadsheets,
                teamConfig: extractionTeamConfig
            });
        }

        const parsed = applyCorrections(parseRaceScreenshots(ocrResults, extractionTeamConfig), session.corrections || []);
        return attachAttendanceSnapshot({
            ...session,
            status: "preview",
            metadata: parsed.metadata,
            teamEventName: parsed.metadata?.eventName || parsed.metadata?.title || session.teamEventName || "",
            players: parsed.players,
            stats: parsed.stats,
            rawOcrText: parsed.rawText,
            rawGeminiText: ocrResults[0]?.rawGeminiText || "",
            rawGeminiJson: ocrResults[0]?.structured || null
        });
    } finally {
        if (downloadedImages) await removeSourceImages(session);
    }
}

function previousMonthAnchor(now = new Date()) {
    return new Date(Date.UTC(now.getUTCFullYear(), now.getUTCMonth(), 0, 12, 0, 0));
}

async function runSpreadsheetMaintenance(client, options = {}) {
    const config = await loadDashboardConfig();
    await cleanupSpreadsheetRawData({
        retentionDays: config.spreadsheets.rawDataRetentionDays || 31
    }).catch(error => {
        console.error("Spreadsheet raw data cleanup failed:", error.message);
    });

    const now = new Date();
    if (!options.forceMonthly && now.getUTCDate() > 3) return [];

    const teams = enabledSpreadsheetTeams(config).filter(team => team.outputChannelId);
    const posted = [];
    for (const teamConfig of teams) {
        const result = await sendPeriodReport(client, teamConfig, "monthly", {
            anchorDate: previousMonthAnchor(now),
            force: Boolean(options.forceMonthly),
            reason: options.forceMonthly ? "forced-monthly" : "month-end"
        }).catch(error => {
            console.error(`Automatic monthly report failed for ${teamConfig.id}:`, error.message);
            return null;
        });
        if (result?.message) posted.push(result);
    }

    return posted;
}

let maintenanceStarted = false;

function startSpreadsheetReportScheduler(client) {
    if (maintenanceStarted) return;
    maintenanceStarted = true;

    setTimeout(() => {
        runSpreadsheetMaintenance(client).catch(error => {
            console.error("Spreadsheet maintenance failed:", error.message);
        });
    }, 60 * 1000);

    setInterval(() => {
        runSpreadsheetMaintenance(client).catch(error => {
            console.error("Spreadsheet maintenance failed:", error.message);
        });
    }, 6 * 60 * 60 * 1000);
}

function scheduleProcessing(client, session, config, teamConfig) {
    const key = sessionTimerKey(session);
    const existing = timers.get(key);
    if (existing) clearTimeout(existing);
    if (!teamConfig.autoProcess) return;

    const timeout = setTimeout(async () => {
        timers.delete(key);
        try {
            const processed = await processSpreadsheetSession(client, session.id);
            await sendSessionOutput(client, processed);
        } catch (error) {
            const channel = await client.channels.fetch(session.channelId).catch(() => null);
            if (channel?.isTextBased?.()) {
                await channel.send({
                    content: `Spreadsheet Gemini extraction failed for session \`${session.id}\`: ${error.message}`,
                    allowedMentions: { parse: [] }
                }).catch(() => null);
            }
        }
    }, sessionWindowMs(config));

    timers.set(key, timeout);
}

async function handleSpreadsheetMessage(message) {
    if (!message.guildId || message.author?.bot) return false;
    const config = await loadDashboardConfig();
    const teamConfig = teamForMessage(config, message);
    if (!teamConfig) return false;
    const attachments = extractImageAttachments(message);
    if (!attachments.length) return false;

    const now = new Date().toISOString();
    const windowMs = sessionWindowMs(config);
    const pendingSessions = await listSpreadsheetSessions({ teamId: teamConfig.id, status: "pending" });
    const latest = pendingSessions.find(item =>
        item.channelId === message.channelId &&
        item.authorId === message.author.id &&
        Date.now() - Date.parse(item.lastImageAt || item.createdAt || 0) <= windowMs
    );
    const canAppend = Boolean(latest);
    const session = canAppend ? latest : {
        teamId: teamConfig.id,
        teamName: teamConfig.name,
        guildId: message.guildId,
        channelId: message.channelId,
        authorId: message.author.id,
        authorTag: displayTag(message.author),
        messageIds: [],
        images: [],
        corrections: [],
        status: "pending",
        createdAt: now
    };

    const saved = await saveSpreadsheetSession({
        ...session,
        teamName: teamConfig.name,
        messageIds: [...new Set([...(session.messageIds || []), message.id])],
        images: [...(session.images || []), ...attachments],
        lastImageAt: now,
        status: "pending"
    });

    scheduleProcessing(message.client, saved, config, teamConfig);
    return true;
}

async function resolveTeamForInteraction(interaction, teamValue) {
    const config = await loadDashboardConfig();
    const teamConfig = findSpreadsheetTeam(config, teamValue);
    if (!teamConfig) {
        await interaction.editReply(`Spreadsheet team **${teamValue}** is not configured.`);
        return { config, teamConfig: null };
    }
    if (!(await requireSpreadsheetAccess(interaction, teamConfig))) {
        return { config, teamConfig: null };
    }
    return { config, teamConfig };
}

async function resolveSessionForTeam(teamId, sessionId, statuses = []) {
    if (sessionId) {
        const session = await getSpreadsheetSession(sessionId);
        if (!session || session.teamId !== teamId) return null;
        return statuses.length && !statuses.includes(session.status) ? null : session;
    }
    return latestSpreadsheetSession(teamId, statuses);
}

module.exports = {
    attachmentFor,
    buildSummaryEmbed,
    buildSummaryText,
    canAccessSpreadsheetTeam,
    correctSpreadsheetSession,
    findSpreadsheetTeam,
    handleSpreadsheetMessage,
    listSpreadsheetSessions,
    previewSpreadsheetSession,
    processSpreadsheetSession,
    rebuildSpreadsheetSession,
    requireSpreadsheetAccess,
    runSpreadsheetMaintenance,
    resolveSessionForTeam,
    resolveTeamForInteraction,
    sendPeriodReport,
    sendSessionOutput,
    startSpreadsheetReportScheduler
};
