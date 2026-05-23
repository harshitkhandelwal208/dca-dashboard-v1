const { readState } = require("./stateStore");

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

module.exports = {
    listRecruitmentBans
};
