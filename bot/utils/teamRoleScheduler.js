const { loadDashboardConfig } = require("./dashboardConfig");
const { logAction } = require("./logStore");
const { findTeam } = require("./memberCountManager");
const { readState, writeState } = require("./stateStore");
const { getCommunityGuildId, getRecruitmentGuildId } = require("./serverConfig");

const ROLE_ASSIGNMENTS_SCOPE = "teamRoleAssignments";
const CHECK_INTERVAL_MS = 60 * 1000;
const RETRY_MS = 10 * 60 * 1000;
const MAX_ATTEMPTS = 2016;
let schedulerStarted = false;

function clone(value) {
    return JSON.parse(JSON.stringify(value));
}

function emptyState() {
    return { assignments: {} };
}

function normalizeState(raw) {
    return raw && typeof raw === "object" && raw.assignments && typeof raw.assignments === "object"
        ? raw
        : emptyState();
}

async function readAssignments() {
    return normalizeState(await readState(ROLE_ASSIGNMENTS_SCOPE, emptyState()));
}

async function writeAssignments(state) {
    await writeState(ROLE_ASSIGNMENTS_SCOPE, normalizeState(state));
}

function assignmentId(type, ticketId, userId, teamId) {
    return `${type}-${ticketId || Date.now()}-${userId}-${teamId}`;
}

function configuredAssignmentsForTeam(config, team) {
    const assignments = [];

    if (team.recruitmentRoleAutoAssignEnabled && team.recruitmentRoleId) {
        assignments.push({
            type: "recruitment",
            guildId: getRecruitmentGuildId(config),
            roleId: team.recruitmentRoleId,
            delayMinutes: Number(team.recruitmentRoleDelayMinutes || 0),
            requiresRoleId: ""
        });
    }

    const communityRoleId = team.communityRoleId || team.roleId || "";
    const communityEnabled = team.communityRoleAutoAssignEnabled || team.autoAssignEnabled;
    if (communityEnabled && communityRoleId) {
        assignments.push({
            type: "community",
            guildId: getCommunityGuildId(config),
            roleId: communityRoleId,
            delayMinutes: Number(team.communityRoleDelayMinutes || team.autoAssignDelayMinutes || 0),
            requiresRoleId: config.recruitment?.communityRulesRoleId || ""
        });
    }

    return assignments.filter(assignment => assignment.guildId && assignment.roleId);
}

async function queueTeamRoleAssignment(client, { userId, teamName, ticketId = "", actor = null } = {}) {
    if (!userId || !teamName) return { skipped: true, reason: "Missing user or team." };

    const config = await loadDashboardConfig();
    const team = findTeam(config.memberCounts, teamName);
    if (!team) return { skipped: true, reason: `Team ${teamName} is not configured.` };
    const configuredAssignments = configuredAssignmentsForTeam(config, team);
    if (!configuredAssignments.length) {
        return { skipped: true, reason: `Auto role assignment is not configured for ${team.name}.` };
    }

    const now = Date.now();
    const state = await readAssignments();
    const queued = [];

    for (const assignment of configuredAssignments) {
        const id = assignmentId(assignment.type, ticketId, userId, team.id);
        const delayMinutes = Math.max(0, Number(assignment.delayMinutes || 0));

        state.assignments[id] = {
            id,
            type: assignment.type,
            status: "pending",
            userId,
            teamId: team.id,
            teamName: team.name,
            roleId: assignment.roleId,
            guildId: assignment.guildId,
            requiresRoleId: assignment.requiresRoleId,
            ticketId,
            attempts: 0,
            createdAt: new Date(now).toISOString(),
            notBeforeAt: new Date(now + delayMinutes * 60 * 1000).toISOString(),
            runAt: new Date(now + delayMinutes * 60 * 1000).toISOString()
        };
        queued.push(clone(state.assignments[id]));
    }

    await writeAssignments(state);

    await logAction(client, {
        type: "ticket",
        title: "Team Role Assignments Queued",
        message: `<@${userId}> has ${queued.length} pending **${team.name}** role assignment(s).`,
        guildId: getRecruitmentGuildId(config),
        actorId: actor?.id || "",
        actorTag: actor?.tag || actor?.username || "",
        targetId: userId,
        metadata: { assignmentIds: queued.map(item => item.id), teamId: team.id }
    }).catch(() => null);

    if (queued.some(assignment => Date.parse(assignment.runAt) <= now)) {
        processDueTeamRoleAssignments(client).catch(error => {
            console.error("Team role assignment failed:", error.message);
        });
    }

    return queued;
}

async function tryAssignment(client, assignment) {
    const guild = await client.guilds.fetch(assignment.guildId).catch(() => null);
    if (!guild) return { done: false, reason: "guild unavailable" };

    const member = await guild.members.fetch(assignment.userId).catch(() => null);
    if (!member) return { done: false, reason: `member not in ${assignment.type || "target"} guild` };

    if (assignment.requiresRoleId && !member.roles.cache.has(assignment.requiresRoleId)) {
        return { done: false, reason: "waiting for configured rules role" };
    }

    if (!member.roles.cache.has(assignment.roleId)) {
        await member.roles.add(assignment.roleId, `Recruitment accepted for ${assignment.teamName}`);
    }

    return { done: true };
}

async function processDueTeamRoleAssignments(client, onlyUserId = "", options = {}) {
    if (!client?.isReady?.()) return;

    const now = Date.now();
    const state = await readAssignments();
    let changed = false;
    const ignoreRunAt = options.ignoreRunAt === true;

    for (const assignment of Object.values(state.assignments)) {
        if (assignment.status !== "pending") continue;
        if (onlyUserId && assignment.userId !== onlyUserId) continue;
        if (Date.parse(assignment.notBeforeAt || assignment.runAt) > now) continue;
        if (!ignoreRunAt && Date.parse(assignment.runAt) > now) continue;

        try {
            const result = await tryAssignment(client, assignment);
            assignment.attempts = Number(assignment.attempts || 0) + 1;
            assignment.lastAttemptAt = new Date().toISOString();

            if (result.done) {
                assignment.status = "complete";
                assignment.completedAt = new Date().toISOString();
            } else if (assignment.attempts >= MAX_ATTEMPTS) {
                assignment.status = "expired";
                assignment.failedReason = result.reason;
            } else {
                assignment.runAt = new Date(now + RETRY_MS).toISOString();
                assignment.lastReason = result.reason;
            }
            changed = true;
        } catch (error) {
            assignment.attempts = Number(assignment.attempts || 0) + 1;
            assignment.lastAttemptAt = new Date().toISOString();
            assignment.lastReason = error.message;
            assignment.runAt = new Date(now + RETRY_MS).toISOString();
            changed = true;
        }
    }

    if (changed) await writeAssignments(state);
}

function startTeamRoleScheduler(client) {
    if (schedulerStarted) return;
    schedulerStarted = true;

    setTimeout(() => processDueTeamRoleAssignments(client).catch(error => {
        console.error("Team role scheduler failed:", error.message);
    }), 5000);

    const timer = setInterval(() => {
        processDueTeamRoleAssignments(client).catch(error => {
            console.error("Team role scheduler failed:", error.message);
        });
    }, CHECK_INTERVAL_MS);

    timer.unref?.();
}

async function handleCommunityMemberJoin(member) {
    const config = await loadDashboardConfig();
    if (member.guild.id !== getCommunityGuildId(config)) return;
    await processDueTeamRoleAssignments(member.client, member.id);
}

module.exports = {
    handleCommunityMemberJoin,
    processDueTeamRoleAssignments,
    queueTeamRoleAssignment,
    startTeamRoleScheduler
};
