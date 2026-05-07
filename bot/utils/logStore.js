const { EmbedBuilder } = require("discord.js");
const { readState, writeState } = require("./stateStore");
const { loadDashboardConfig } = require("./dashboardConfig");

const LOG_SCOPE = "botLogs";
const MAX_LOGS = 1000;

function emptyLogState() {
    return { logs: [] };
}

function normalizeLogState(raw) {
    if (raw && typeof raw === "object" && Array.isArray(raw.logs)) return { logs: raw.logs };
    return emptyLogState();
}

function clone(value) {
    return JSON.parse(JSON.stringify(value));
}

async function appendLog(entry) {
    const state = normalizeLogState(await readState(LOG_SCOPE, emptyLogState()));
    const log = {
        id: entry.id || `${Date.now()}-${Math.random().toString(16).slice(2)}`,
        type: entry.type || "system",
        title: entry.title || "Log Entry",
        message: entry.message || "",
        guildId: entry.guildId || "",
        actorId: entry.actorId || "",
        actorTag: entry.actorTag || "",
        targetId: entry.targetId || "",
        targetTag: entry.targetTag || "",
        metadata: entry.metadata || {},
        createdAt: entry.createdAt || new Date().toISOString()
    };

    state.logs.unshift(log);
    state.logs = state.logs.slice(0, MAX_LOGS);
    await writeState(LOG_SCOPE, state);
    return clone(log);
}

async function listLogs(limit = 100, type = "") {
    const state = normalizeLogState(await readState(LOG_SCOPE, emptyLogState()));
    return state.logs
        .filter(log => !type || log.type === type)
        .slice(0, Math.max(1, Math.min(500, Number(limit) || 100)))
        .map(clone);
}

function logEventEnabled(config, type) {
    if (!config.logging?.enabled) return false;
    const map = {
        ticket: "tickets",
        memberCount: "memberCounts",
        youtube: "youtube",
        reactionRole: "reactionRoles",
        system: "system"
    };
    const key = map[type] || type;
    return config.logging.events?.[key] !== false;
}

async function sendLogToDiscord(client, log, config) {
    if (!client?.channels?.fetch || !config.logging?.channelId || !logEventEnabled(config, log.type)) return;

    const channel = await client.channels.fetch(config.logging.channelId).catch(() => null);
    if (!channel?.isTextBased?.()) return;

    const embed = new EmbedBuilder()
        .setTitle(log.title)
        .setDescription(log.message || "No details provided.")
        .setColor(0x0f766e)
        .setTimestamp(new Date(log.createdAt));

    if (log.actorId) embed.addFields({ name: "Actor", value: `<@${log.actorId}>`, inline: true });
    if (log.targetId) embed.addFields({ name: "Target", value: `<@${log.targetId}>`, inline: true });
    if (log.metadata?.threadId) embed.addFields({ name: "Thread", value: `<#${log.metadata.threadId}>`, inline: true });

    await channel.send({ embeds: [embed] });
}

async function logAction(client, entry) {
    const log = await appendLog(entry);

    try {
        const config = await loadDashboardConfig();
        await sendLogToDiscord(client, log, config);
    } catch (error) {
        console.error("Failed to send log entry to Discord:", error.message);
    }

    return log;
}

module.exports = {
    appendLog,
    listLogs,
    logAction
};
