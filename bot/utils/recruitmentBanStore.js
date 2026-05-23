const { readState, writeState } = require("./stateStore");

const BAN_SCOPE = "recruitmentBans";

function clone(value) {
    return JSON.parse(JSON.stringify(value));
}

function emptyState() {
    return { bans: {} };
}

function normalizeState(raw) {
    return raw && typeof raw === "object" && raw.bans && typeof raw.bans === "object"
        ? raw
        : emptyState();
}

async function listRecruitmentBans() {
    const state = normalizeState(await readState(BAN_SCOPE, emptyState()));
    return Object.values(state.bans)
        .map(clone)
        .sort((a, b) => String(b.updatedAt || b.createdAt).localeCompare(String(a.updatedAt || a.createdAt)));
}

async function addRecruitmentBan(entry) {
    if (!entry?.userId) throw new Error("Ban entry is missing userId.");

    const state = normalizeState(await readState(BAN_SCOPE, emptyState()));
    const now = new Date().toISOString();
    state.bans[entry.userId] = {
        ...state.bans[entry.userId],
        userId: entry.userId,
        userTag: entry.userTag || state.bans[entry.userId]?.userTag || "",
        reason: entry.reason || "No reason provided.",
        bannedById: entry.bannedById || "",
        bannedByTag: entry.bannedByTag || "",
        guildId: entry.guildId || "",
        createdAt: state.bans[entry.userId]?.createdAt || now,
        updatedAt: now
    };

    await writeState(BAN_SCOPE, state);
    return clone(state.bans[entry.userId]);
}

module.exports = {
    addRecruitmentBan,
    listRecruitmentBans
};
