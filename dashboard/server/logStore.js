const { readState, writeState } = require("./stateStore");

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

module.exports = {
    appendLog,
    listLogs
};
