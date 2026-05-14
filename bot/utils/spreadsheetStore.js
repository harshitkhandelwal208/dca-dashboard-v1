const { readState, writeState } = require("./stateStore");

const SPREADSHEET_SCOPE = "spreadsheetSessions";
const REPORT_EMISSION_SCOPE = "spreadsheetReportEmissions";
const MAX_SESSIONS = 500;

function emptyState() {
    return { sessions: [] };
}

function emptyReportEmissionState() {
    return { reports: [] };
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

async function cleanupSpreadsheetRawData(options = {}) {
    const retentionDays = Math.max(1, Number(options.retentionDays || 31));
    const cutoff = Date.now() - retentionDays * 24 * 60 * 60 * 1000;
    const state = await readSessions();
    let cleaned = 0;

    for (const session of state.sessions) {
        if (session.status !== "processed") continue;
        const date = Date.parse(session.processedAt || session.updatedAt || session.createdAt || 0);
        if (!date || date > cutoff) continue;

        const hasRaw = Boolean(
            session.rawOcrText ||
            session.rawGeminiText ||
            session.rawGeminiJson ||
            (Array.isArray(session.ocrResults) && session.ocrResults.length)
        );
        if (!hasRaw) continue;

        session.ocrResults = [];
        session.rawOcrText = "";
        session.rawGeminiText = "";
        session.rawGeminiJson = null;
        session.rawDataCleanedAt = new Date().toISOString();
        cleaned += 1;
    }

    if (cleaned) await writeSessions(state);
    return cleaned;
}

async function readReportEmissions() {
    const raw = await readState(REPORT_EMISSION_SCOPE, emptyReportEmissionState());
    if (!raw || typeof raw !== "object" || !Array.isArray(raw.reports)) return emptyReportEmissionState();
    return {
        reports: raw.reports.filter(report => report && typeof report === "object")
    };
}

async function writeReportEmissions(state) {
    const next = {
        reports: (state.reports || [])
            .filter(report => report && report.teamId && report.period && report.periodKey)
            .sort((a, b) => Date.parse(b.emittedAt || 0) - Date.parse(a.emittedAt || 0))
            .slice(0, 500)
    };
    await writeState(REPORT_EMISSION_SCOPE, next);
    return next;
}

async function getReportEmission(teamId, period, periodKey) {
    const state = await readReportEmissions();
    return clone(state.reports.find(report =>
        report.teamId === teamId &&
        report.period === period &&
        report.periodKey === periodKey
    ) || null);
}

async function markReportEmitted(report, details = {}) {
    const state = await readReportEmissions();
    const next = {
        teamId: report.teamId,
        period: report.period,
        periodKey: report.periodKey,
        emittedAt: new Date().toISOString(),
        filePath: report.filePath || "",
        eventCount: report.events?.length || 0,
        reason: details.reason || "",
        channelId: details.channelId || "",
        messageId: details.messageId || ""
    };
    const index = state.reports.findIndex(item =>
        item.teamId === next.teamId &&
        item.period === next.period &&
        item.periodKey === next.periodKey
    );
    if (index === -1) state.reports.unshift(next);
    else state.reports[index] = { ...state.reports[index], ...next };
    await writeReportEmissions(state);
    return clone(next);
}

module.exports = {
    cleanupSpreadsheetRawData,
    getReportEmission,
    getSpreadsheetSession,
    latestSpreadsheetSession,
    listSpreadsheetSessions,
    markReportEmitted,
    saveSpreadsheetSession,
    sessionId,
    updateSpreadsheetSession
};
