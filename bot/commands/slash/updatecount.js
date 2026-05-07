const { PermissionFlagsBits, SlashCommandBuilder } = require("discord.js");
const { loadDashboardConfig, saveDashboardConfig } = require("../../utils/dashboardConfig");
const { syncMemberCountMessage } = require("../../utils/memberCountManager");
const { logAction } = require("../../utils/logStore");

function normalize(value) {
    return String(value || "").toLowerCase().replace(/[^a-z0-9]+/g, "");
}

module.exports = {
    data: new SlashCommandBuilder()
        .setName("updatecount")
        .setDescription("Update a dashboard-managed team member count")
        .setDefaultMemberPermissions(PermissionFlagsBits.ManageGuild)
        .addStringOption(option =>
            option
                .setName("team")
                .setDescription("Team name")
                .setRequired(true)
        )
        .addStringOption(option =>
            option
                .setName("field")
                .setDescription("Field to update")
                .setRequired(true)
                .addChoices(
                    { name: "Number of Players", value: "players" },
                    { name: "Recruitment Status", value: "recruitment" }
                )
        )
        .addStringOption(option =>
            option
                .setName("value")
                .setDescription("New value")
                .setRequired(true)
        ),

    async execute(interaction) {
        await interaction.deferReply({ ephemeral: true });

        const teamName = interaction.options.getString("team", true);
        const field = interaction.options.getString("field", true);
        const value = interaction.options.getString("value", true);
        const config = await loadDashboardConfig();
        const target = normalize(teamName);
        let foundTeam = null;
        let validationError = "";

        const teams = config.memberCounts.teams.map(team => {
            const names = [team.name, ...(team.aliases || [])].map(normalize);
            if (!names.includes(target)) return team;

            foundTeam = team;
            if (field === "players") {
                const players = Number.parseInt(value, 10);
                if (!Number.isFinite(players) || players < 0) {
                    validationError = "Please provide a valid non-negative player count.";
                    return team;
                }
                return { ...team, players };
            }

            return { ...team, recruitmentStatus: value };
        });

        if (!foundTeam) {
            await interaction.editReply(`Team **${teamName}** is not configured. Add it from the dashboard first.`);
            return;
        }

        if (validationError) {
            await interaction.editReply(validationError);
            return;
        }

        await saveDashboardConfig({
            ...config,
            memberCounts: {
                ...config.memberCounts,
                teams
            }
        });

        await logAction(interaction.client, {
            type: "memberCount",
            title: "Member Count Edited",
            message: `<@${interaction.user.id}> updated **${foundTeam.name}** ${field} to **${value}**.`,
            guildId: interaction.guildId,
            actorId: interaction.user.id,
            actorTag: interaction.user.tag || interaction.user.username,
            metadata: { teamId: foundTeam.id, field, value }
        });

        const sync = await syncMemberCountMessage(interaction.client);
        await interaction.editReply(sync.skipped
            ? `Saved, but sync skipped: ${sync.reason}`
            : `Saved and updated <#${sync.channelId}>.`
        );
    }
};
