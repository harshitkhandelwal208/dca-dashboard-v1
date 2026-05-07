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
    buildPeriodReportEmbed,
    generateStandardPeriodReports,
    reportAttachment
} = require("./spreadsheetReports");
const {
    getSpreadsheetSession,
    latestSpreadsheetSession,
    listSpreadsheetSessions,
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
    return Math.max(1, Number(config?.spreadsheets?.sessionWindowMinutes || 5)) * 60 * 1000;
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
    const players = (parsed.players || []).map(player => ({ ...player }));

    for (const correction of corrections) {
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
        }
    }

    players.sort((a, b) => a.rank - b.rank || a.playerName.localeCompare(b.playerName));
    return {
        ...parsed,
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

    session = await updateSpreadsheetSession(session.id, {
        status: "processing",
        error: ""
    });

    try {
        let parsed;
        let ocrResults = session.ocrResults || [];
        if (!ocrResults.length || options.rerunOcr) {
            const outputDir = outputBaseDir(session.teamId, session.id);
            const imagePaths = await downloadImages(session, outputDir);
            ocrResults = await ocrImages(imagePaths, {
                tesseractPath: config.spreadsheets.tesseractPath,
                imageMagickPath: config.spreadsheets.imageMagickPath,
                tesseractLang: config.spreadsheets.tesseractLang,
                tesseractPsm: config.spreadsheets.tesseractPsm
            });
            if (!config.spreadsheets.keepSourceImages) {
                await fs.promises.rm(path.join(outputDir, "source"), { recursive: true, force: true }).catch(() => null);
            }
            parsed = parseRaceScreenshots(ocrResults, teamConfig);
        } else {
            parsed = parseRaceScreenshots(ocrResults, teamConfig);
        }

        parsed = applyCorrections(parsed, session.corrections || []);
        session = {
            ...session,
            status: "processed",
            processedAt: new Date().toISOString(),
            ocrResults,
            metadata: parsed.metadata,
            players: parsed.players,
            stats: parsed.stats,
            rawOcrText: parsed.rawText,
            outputs: {}
        };
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
    const parsed = applyCorrections(
        session.ocrResults?.length
            ? parseRaceScreenshots(session.ocrResults, teamConfig)
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
        outputs: {}
    };
    next.outputs = await rebuildArtifacts(next, config, teamConfig);
    const saved = await saveSpreadsheetSession(next);

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
    const maxScore = Math.max(0, ...players.map(player => Number(player.points ?? player.score ?? 0) || 0));

    return own.filter(player => {
        const rank = Number(player.rank);
        if (Number.isFinite(rank) && topOpponentRank !== null) return rank < topOpponentRank;
        return (Number(player.points ?? player.score ?? 0) || 0) >= maxScore && maxScore > 0;
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

async function sendSessionOutput(client, session) {
    const config = await loadDashboardConfig();
    const teamConfig = findSpreadsheetTeam(config, session.teamId);
    const channelId = teamConfig?.outputChannelId || session.channelId;
    const channel = await client.channels.fetch(channelId).catch(() => null);
    if (!channel?.isTextBased?.()) return null;

    const reports = teamConfig
        ? await generateStandardPeriodReports(teamConfig, session).catch(error => {
            console.error("Spreadsheet period report generation failed:", error);
            return [];
        })
        : [];
    const files = teamConfig?.outputChannelId ? [
        attachmentFor(session.outputs?.spreadsheetPath),
        attachmentFor(session.outputs?.chartPath),
        ...reports.map(reportAttachment)
    ].filter(Boolean) : [];
    const embeds = [
        buildSummaryEmbed(session),
        ...reports.map(buildPeriodReportEmbed)
    ];

    return channel.send({
        content: teamConfig?.outputChannelId
            ? `Processed spreadsheet session \`${session.id}\`. Weekly and monthly reports were rebuilt with missed events scored as 0.`
            : `Spreadsheet session \`${session.id}\` was processed. Configure an output channel to receive automatic files and weekly/monthly reports.`,
        embeds,
        files,
        allowedMentions: { parse: [] }
    });
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
                    content: `Spreadsheet OCR failed for session \`${session.id}\`: ${error.message}`,
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
    processSpreadsheetSession,
    rebuildSpreadsheetSession,
    requireSpreadsheetAccess,
    resolveSessionForTeam,
    resolveTeamForInteraction,
    sendSessionOutput
};
