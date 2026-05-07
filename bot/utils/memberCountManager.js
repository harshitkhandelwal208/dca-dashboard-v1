const { EmbedBuilder } = require("discord.js");
const { loadDashboardConfig, saveDashboardConfig } = require("./dashboardConfig");
const { logAction } = require("./logStore");

function normalizeTeamName(value) {
    return String(value || "")
        .normalize("NFKD")
        .replace(/[\u0300-\u036f]/g, "")
        .replace(/[^a-z0-9]+/gi, "")
        .toLowerCase();
}

function findTeam(memberCounts, teamName) {
    const target = normalizeTeamName(teamName);
    return memberCounts.teams.find(team => {
        const names = [team.name, ...(team.aliases || [])];
        return names.some(name => normalizeTeamName(name) === target);
    });
}

function buildMemberCountContent(memberCounts) {
    const lines = [`# ${memberCounts.title || "Member Count"}`];

    memberCounts.teams.forEach((team, index) => {
        lines.push("");
        lines.push(`## Team ${index + 1} - ${team.name}`);
        if (team.division) lines.push(`(${team.division})`);
        lines.push(`Number of Players - ${team.players}`);
        lines.push(`Recruitment Status - ${team.recruitmentStatus}`);
    });

    return lines.join("\n");
}

function buildMemberCountEmbed(memberCounts) {
    const teams = memberCounts.teams.slice(0, 25);
    const totalPlayers = teams.reduce((sum, team) => sum + Number(team.players || 0), 0);
    const embed = new EmbedBuilder()
        .setTitle(memberCounts.title || "Member Count")
        .setDescription([
            `**Tracked teams:** ${teams.length}`,
            `**Total players:** ${totalPlayers}`
        ].join("\n"))
        .setColor(0x0f766e)
        .setFooter({ text: "Use /teamcount or the dashboard to update these numbers." })
        .setTimestamp();

    for (const team of teams) {
        embed.addFields({
            name: `${team.name}${team.division ? ` - ${team.division}` : ""}`,
            value: [
                `**Players:** ${team.players}`,
                `**Recruitment:** ${team.recruitmentStatus}`,
                team.aliases?.length ? `**Aliases:** ${team.aliases.join(", ")}` : ""
            ].filter(Boolean).join("\n"),
            inline: true
        });
    }

    return embed;
}

async function syncMemberCountMessage(client, options = {}) {
    const config = await loadDashboardConfig();
    const memberCounts = config.memberCounts;

    if (!memberCounts.enabled) return { skipped: true, reason: "Member counts are disabled." };
    if (!memberCounts.channelId) return { skipped: true, reason: "Member count channel is not configured." };

    const channel = await client.channels.fetch(memberCounts.channelId).catch(() => null);
    if (!channel?.isTextBased?.() || !channel.messages?.fetch) {
        return { skipped: true, reason: "Member count channel is not text based." };
    }

    let message = null;
    if (memberCounts.messageId) {
        message = await channel.messages.fetch(memberCounts.messageId).catch(() => null);
        if (message?.author?.id !== client.user.id) message = null;
    }

    const payload = {
        content: "",
        embeds: [buildMemberCountEmbed(memberCounts)]
    };

    const created = !message;
    if (message) {
        await message.edit(payload);
    } else {
        message = await channel.send(payload);
        await saveDashboardConfig({
            ...config,
            memberCounts: {
                ...memberCounts,
                messageId: message.id
            }
        });
    }

    if (!options.silent) {
        await logAction(client, {
            type: "memberCount",
            title: created ? "Member Count Message Created" : "Member Count Message Updated",
            message: `${memberCounts.teams.length} teams are listed in <#${channel.id}>.`,
            guildId: channel.guildId,
            metadata: { channelId: channel.id, messageId: message.id }
        });
    }

    return { created, channelId: channel.id, messageId: message.id };
}

async function incrementTeamCount(client, teamName, delta, actor = null) {
    if (!teamName) return { skipped: true, reason: "No team selected." };

    const config = await loadDashboardConfig();
    const memberCounts = config.memberCounts;
    if (!memberCounts.updateOnRecruitmentClose) return { skipped: true, reason: "Auto-update is disabled." };

    const team = findTeam(memberCounts, teamName);
    if (!team) return { skipped: true, reason: `Team ${teamName} is not configured in member counts.` };

    const nextTeams = memberCounts.teams.map(item =>
        item.id === team.id ? { ...item, players: Math.max(0, Number(item.players || 0) + delta) } : item
    );

    await saveDashboardConfig({
        ...config,
        memberCounts: {
            ...memberCounts,
            teams: nextTeams
        }
    });

    await logAction(client, {
        type: "memberCount",
        title: "Member Count Updated",
        message: `${team.name} changed by ${delta > 0 ? `+${delta}` : delta}.`,
        guildId: config.bot.guildId,
        actorId: actor?.id || "",
        actorTag: actor?.tag || actor?.username || "",
        metadata: { teamId: team.id, teamName: team.name, delta }
    });

    if (client?.isReady?.()) {
        await syncMemberCountMessage(client, { silent: true }).catch(error => {
            console.error("Failed to sync member count message:", error.message);
        });
    }

    return { teamId: team.id, teamName: team.name, delta };
}

module.exports = {
    buildMemberCountContent,
    buildMemberCountEmbed,
    findTeam,
    incrementTeamCount,
    syncMemberCountMessage
};
