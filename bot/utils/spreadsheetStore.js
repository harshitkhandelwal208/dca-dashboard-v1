const { readState, writeState } = require("./stateStore");

const SPREADSHEET_SCOPE = "spreadsheetSessions";
const MAX_SESSIONS = 500;

function emptyState() {
    return { sessions: [] };
}

function clone(value) {
    return JSON.parse(JSON.stringify(value));
}

function normalizeState(raw) {
    if (!raw || typeof raw !== "object" || !Array.isArray(raw.sessions)) return emptyState();
    return { sessions: raw.sessions.filter(session => session && typeof session === "object") };
}

function sessionId(prefix = "race") {
    const stamp = new Date().toISOString().replace(/[-:.TZ]/g, "").slice(0, 14);
    return `${prefix}-${stamp}-${Math.random().toString(16).slice(2, 8)}`;
}

async function readSessions() {
    return normalizeState(await readState(SPREADSHEET_SCOPE, emptyState()));
}

async function writeSessions(state) {
    const normalized = normalizeState(state);
    normalized.sessions = normalized.sessions
        .sort((a, b) => Date.parse(b.updatedAt || b.createdAt || 0) - Date.parse(a.updatedAt || a.createdAt || 0))
        .slice(0, MAX_SESSIONS);
    await writeState(SPREADSHEET_SCOPE, normalized);
    return normalized;
}

async function listSpreadsheetSessions(filter = {}) {
    const state = await readSessions();
    return state.sessions
        .filter(session => !filter.teamId || session.teamId === filter.teamId)
        .filter(session => !filter.status || session.status === filter.status)
        .map(clone);
}

async function getSpreadsheetSession(id) {
    const state = await readSessions();
    const session = state.sessions.find(item => item.id === id);
    return session ? clone(session) : null;
}

async function saveSpreadsheetSession(session) {
    const state = await readSessions();
    const now = new Date().toISOString();
    const next = {
        ...session,
        id: session.id || sessionId(),
        createdAt: session.createdAt || now,
        updatedAt: now
    };
    const index = state.sessions.findIndex(item => item.id === next.id);
    if (index === -1) {
        state.sessions.unshift(next);
    } else {
        state.sessions[index] = { ...state.sessions[index], ...next };
    }
    await writeSessions(state);
    return clone(next);
}

async function updateSpreadsheetSession(id, patch) {
    const session = await getSpreadsheetSession(id);
    if (!session) return null;
    return saveSpreadsheetSession({ ...session, ...patch, id });
}

async function latestSpreadsheetSession(teamId, statuses = []) {
    const sessions = await listSpreadsheetSessions({ teamId });
    const allowed = new Set(statuses.filter(Boolean));
    return sessions.find(session => !allowed.size || allowed.has(session.status)) || null;
}

module.exports = {
    getSpreadsheetSession,
    latestSpreadsheetSession,
    listSpreadsheetSessions,
    saveSpreadsheetSession,
    sessionId,
    updateSpreadsheetSession
};
