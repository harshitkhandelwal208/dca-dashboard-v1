function cleanText(value, fallback = "", maxLength = 120) {
    const text = String(value || "")
        .replace(/\s+/g, " ")
        .trim();
    return (text || fallback).slice(0, maxLength);
}

function normalizeKey(value) {
    return String(value || "")
        .toLowerCase()
        .replace(/[\u00ae\u2122]/g, "")
        .replace(/[^a-z0-9]+/g, " ")
        .replace(/\s+/g, " ")
        .trim();
}

function playerKey(name) {
    return normalizeKey(name);
}

function sessionDate(session) {
    const value = session?.processedAt || session?.updatedAt || session?.createdAt || new Date().toISOString();
    const date = new Date(value);
    return Number.isNaN(date.getTime()) ? new Date() : date;
}

function eventNameForSession(session, fallback = "Team Event") {
    return cleanText(
        session?.metadata?.eventName ||
        session?.metadata?.title ||
        session?.teamEventName ||
        session?.eventName ||
        fallback,
        fallback,
        120
    );
}

function normalizeTeamType(value) {
    const text = normalizeKey(value);
    if (["own", "our", "team", "teammate", "ally", "allied"].includes(text)) return "own";
    if (["opponent", "enemy", "opposing", "rival", "other", "blue"].includes(text)) return "opponent";
    return "unknown";
}

function numberValue(value) {
    const number = Number(value);
    return Number.isFinite(number) ? number : 0;
}

function scoreValue(player) {
    return numberValue(player?.score);
}

function pointsValue(player) {
    return numberValue(player?.points);
}

function ownPlayersForSession(session) {
    return (session?.players || []).filter(player => player.teamType === "own");
}

function opponentPlayersForSession(session) {
    return (session?.players || []).filter(player => player.teamType !== "own");
}

function topOpponentRank(session) {
    const ranks = opponentPlayersForSession(session)
        .map(player => Number(player.rank))
        .filter(rank => Number.isFinite(rank));
    return ranks.length ? Math.min(...ranks) : null;
}

function opponentCount(session) {
    return opponentPlayersForSession(session).length;
}

function bluesKilledForRank(session, rank) {
    const ownRank = Number(rank);
    if (!Number.isFinite(ownRank)) return 0;
    return opponentPlayersForSession(session)
        .filter(player => Number(player.rank) > ownRank)
        .length;
}

function percent(value, max) {
    const numerator = Number(value);
    const denominator = Number(max);
    if (!Number.isFinite(numerator) || !Number.isFinite(denominator) || denominator <= 0) return 0;
    return Math.round((numerator / denominator) * 1000) / 10;
}

function blueKillPercent(killed, possible) {
    return percent(killed, possible);
}

function sessionKabMap(session) {
    const threshold = topOpponentRank(session);
    const map = new Map();

    for (const player of ownPlayersForSession(session)) {
        const rank = Number(player.rank);
        const kab = threshold !== null && Number.isFinite(rank) && rank < threshold;
        map.set(player.playerName, kab ? 1 : 0);
    }

    return map;
}

function eventMaxScore(session) {
    return Math.max(0, ...(session?.players || []).map(scoreValue));
}

function eventMaxPoints(session) {
    return Math.max(0, ...(session?.players || []).map(pointsValue));
}

function uniqueNames(names) {
    const seen = new Set();
    const output = [];

    for (const name of names.map(item => cleanText(item, "", 80)).filter(Boolean)) {
        const key = normalizeKey(name);
        if (!key || seen.has(key)) continue;
        seen.add(key);
        output.push(name);
    }

    return output;
}

function namesFromScoreLine(rawLine, ownTeamName) {
    const ownKey = normalizeKey(ownTeamName);
    return String(rawLine || "")
        .split(/\bvs\b|,|;/i)
        .map(part => cleanText(part.replace(/\d[\d\s,.]*$/g, ""), "", 80))
        .filter(name => name && normalizeKey(name) !== ownKey);
}

function sessionOpponentTeams(session) {
    const metadata = session?.metadata || {};
    const ownTeamName = metadata.ownTeamName || session?.teamName || session?.teamId || "";
    const ownKey = normalizeKey(ownTeamName);
    const teams = Array.isArray(metadata.teams) ? metadata.teams : [];
    const explicit = teams
        .map(team => ({
            label: cleanText(team?.label || team?.name || team?.teamName || "", "", 80),
            type: normalizeTeamType(team?.teamType || team?.type || team?.classification || "")
        }))
        .filter(team => team.label)
        .filter(team => team.type === "opponent" || (team.type !== "own" && normalizeKey(team.label) !== ownKey))
        .map(team => team.label);

    const fromPlayers = opponentPlayersForSession(session)
        .map(player => cleanText(player.teamLabel || "", "", 80))
        .filter(label => {
            const key = normalizeKey(label);
            return key && key !== ownKey && !["opponent", "unknown", "blue", "none"].includes(key);
        });

    const fromScoreLine = namesFromScoreLine(metadata.teamScores?.rawLine, ownTeamName);
    const names = uniqueNames([...explicit, ...fromPlayers, ...fromScoreLine]);
    return names.length ? names : ["Opponent"];
}

function teamScoreFromMetadata(session, teamType) {
    const teamScores = session?.metadata?.teamScores || {};
    if (teamType === "own") return numberValue(teamScores.own);
    return numberValue(teamScores.opponent);
}

module.exports = {
    blueKillPercent,
    bluesKilledForRank,
    cleanText,
    eventMaxPoints,
    eventMaxScore,
    eventNameForSession,
    normalizeKey,
    opponentCount,
    opponentPlayersForSession,
    ownPlayersForSession,
    percent,
    playerKey,
    pointsValue,
    scoreValue,
    sessionDate,
    sessionKabMap,
    sessionOpponentTeams,
    teamScoreFromMetadata,
    topOpponentRank
};
