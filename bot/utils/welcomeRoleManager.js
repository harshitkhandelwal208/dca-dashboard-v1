const { ActionRowBuilder, ButtonBuilder, ButtonStyle, PermissionFlagsBits } = require("discord.js");
const { loadDashboardConfig } = require("./dashboardConfig");
const { logAction } = require("./logStore");
const { getCommunityGuildId, getRecruitmentGuildId } = require("./serverConfig");

const WELCOME_TEAM_PREFIX = "welcome-team:";

function teamButtonsForMember(memberId, teams) {
    const buttons = teams
        .filter(team => team.communityRoleId || team.roleId)
        .slice(0, 25)
        .map(team =>
            new ButtonBuilder()
                .setCustomId(`${WELCOME_TEAM_PREFIX}${memberId}:${team.id}`)
                .setLabel(team.name.slice(0, 80))
                .setStyle(ButtonStyle.Secondary)
        );

    const rows = [];
    for (let index = 0; index < buttons.length; index += 5) {
        rows.push(new ActionRowBuilder().addComponents(...buttons.slice(index, index + 5)));
    }

    return rows;
}

async function recruiterCanUseButton(client, userId, config) {
    const roleId = config.recruitment.recruiterRoleId || config.bot.recruiterRoleId;
    if (!roleId) return false;

    const recruitmentGuildId = getRecruitmentGuildId(config);
    const guild = await client.guilds.fetch(recruitmentGuildId).catch(() => null);
    if (!guild) return false;

    const member = await guild.members.fetch(userId).catch(() => null);
    if (!member) return false;

    return Boolean(
        member.roles.cache.has(roleId) ||
        member.permissions.has(PermissionFlagsBits.Administrator) ||
        member.permissions.has(PermissionFlagsBits.ManageGuild)
    );
}

async function handleWelcomeTeamButton(interaction) {
    if (!interaction.isButton() || !interaction.customId.startsWith(WELCOME_TEAM_PREFIX)) return false;

    const config = await loadDashboardConfig();
    const communityGuildId = getCommunityGuildId(config);
    if (communityGuildId && interaction.guildId !== communityGuildId) {
        await interaction.reply({ content: "This welcome role button belongs to the configured community server.", ephemeral: true });
        return true;
    }

    const [, memberId, teamId] = interaction.customId.match(/^welcome-team:(\d{10,25}):(.+)$/) || [];
    if (!memberId || !teamId) {
        await interaction.reply({ content: "This welcome button is invalid.", ephemeral: true });
        return true;
    }

    if (!(await recruiterCanUseButton(interaction.client, interaction.user.id, config))) {
        await interaction.reply({ content: "Only recruitment-server recruiters can use these team role buttons.", ephemeral: true });
        return true;
    }

    const team = config.memberCounts.teams.find(item => item.id === teamId);
    const roleId = team?.communityRoleId || team?.roleId || "";
    if (!roleId) {
        await interaction.reply({ content: "That team role is not configured anymore.", ephemeral: true });
        return true;
    }

    const member = await interaction.guild.members.fetch(memberId).catch(() => null);
    if (!member) {
        await interaction.reply({ content: "That member is no longer in this server.", ephemeral: true });
        return true;
    }

    await member.roles.add(roleId, `Welcome team role assigned by ${interaction.user.tag || interaction.user.username}`);

    await logAction(interaction.client, {
        type: "system",
        title: "Welcome Team Role Assigned",
        message: `<@${interaction.user.id}> assigned **${team.name}** to <@${member.id}>.`,
        guildId: interaction.guildId,
        actorId: interaction.user.id,
        actorTag: interaction.user.tag || interaction.user.username,
        targetId: member.id,
        targetTag: member.user?.tag || member.user?.username || member.id,
        metadata: { teamId: team.id, roleId }
    }).catch(() => null);

    await interaction.reply({ content: `Assigned **${team.name}** to ${member}.`, ephemeral: true });
    return true;
}

module.exports = {
    handleWelcomeTeamButton,
    teamButtonsForMember
};
