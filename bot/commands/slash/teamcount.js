const { PermissionFlagsBits, SlashCommandBuilder } = require("discord.js");
const { loadDashboardConfig, saveDashboardConfig } = require("../../utils/dashboardConfig");
const { logAction } = require("../../utils/logStore");
const { findTeam, syncMemberCountMessage } = require("../../utils/memberCountManager");

module.exports = {
    data: new SlashCommandBuilder()
        .setName("teamcount")
        .setDescription("Quickly change a team member count.")
        .setDefaultMemberPermissions(PermissionFlagsBits.ManageGuild)
        .addStringOption(option =>
            option.setName("team").setDescription("Team name or alias").setRequired(true)
        )
        .addIntegerOption(option =>
            option.setName("players").setDescription("New player count").setMinValue(0).setMaxValue(999).setRequired(true)
        )
        .addStringOption(option =>
            option.setName("status").setDescription("Optional recruitment status").setRequired(false)
        ),

    async execute(interaction) {
        await interaction.deferReply({ ephemeral: true });

        const teamName = interaction.options.getString("team", true);
        const players = interaction.options.getInteger("players", true);
        const status = interaction.options.getString("status", false);
        const config = await loadDashboardConfig();
        const team = findTeam(config.memberCounts, teamName);

        if (!team) {
            await interaction.editReply(`Team **${teamName}** is not configured.`);
            return;
        }

        const teams = config.memberCounts.teams.map(item =>
            item.id === team.id
                ? { ...item, players, recruitmentStatus: status || item.recruitmentStatus }
                : item
        );

        await saveDashboardConfig({
            ...config,
            memberCounts: {
                ...config.memberCounts,
                teams
            }
        });

        await logAction(interaction.client, {
            type: "memberCount",
            title: "Team Count Updated",
            message: `<@${interaction.user.id}> set **${team.name}** to **${players}** players.`,
            guildId: interaction.guildId,
            actorId: interaction.user.id,
            actorTag: interaction.user.tag || interaction.user.username,
            metadata: { teamId: team.id, players, status: status || "" }
        });

        const sync = await syncMemberCountMessage(interaction.client);
        await interaction.editReply(sync.skipped
            ? `Saved, but sync skipped: ${sync.reason}`
            : `Saved and updated <#${sync.channelId}>.`
        );
    }
};
