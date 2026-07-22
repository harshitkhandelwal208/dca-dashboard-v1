const { readState, writeState } = require("./stateStore");

const WARNINGS_SCOPE = "warnings";

function clone(value) {
    return JSON.parse(JSON.stringify(value));
}

function emptyState() {
    return { warnings: [] };
}

function normalizeState(raw) {
    if (raw && typeof raw === "object" && Array.isArray(raw.warnings)) {
        return { warnings: raw.warnings };
    }

    return emptyState();
}

async function addWarning({ userId, guildId, reason }) {
    if (!userId) throw new Error("Warning is missing userId.");
    if (!guildId) throw new Error("Warning is missing guildId.");

    const state = normalizeState(await readState(WARNINGS_SCOPE, emptyState()));
    const warning = {
        id: `${Date.now()}-${Math.random().toString(16).slice(2)}`,
        userId,
        guildId,
        reason: reason || "No reason provided.",
        createdAt: new Date().toISOString()
    };

    state.warnings.push(warning);
    await writeState(WARNINGS_SCOPE, state);
    return clone(warning);
}

async function listWarnings(userId, guildId) {
    const state = normalizeState(await readState(WARNINGS_SCOPE, emptyState()));
    return state.warnings
        .filter(warning => warning.userId === userId && warning.guildId === guildId)
        .sort((a, b) => String(a.createdAt || "").localeCompare(String(b.createdAt || "")))
        .map(clone);
}

async function clearWarnings(userId, guildId) {
    const state = normalizeState(await readState(WARNINGS_SCOPE, emptyState()));
    const before = state.warnings.length;
    state.warnings = state.warnings.filter(warning => warning.userId !== userId || warning.guildId !== guildId);

    const removed = before - state.warnings.length;
    if (removed > 0) await writeState(WARNINGS_SCOPE, state);

    return removed;
}

module.exports = {
    addWarning,
    listWarnings,
    clearWarnings
};
