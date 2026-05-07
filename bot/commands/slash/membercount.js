const { PermissionFlagsBits, SlashCommandBuilder } = require("discord.js");
const { loadDashboardConfig, saveDashboardConfig } = require("../../utils/dashboardConfig");
const { syncMemberCountMessage } = require("../../utils/memberCountManager");
const { logAction } = require("../../utils/logStore");

module.exports = {
    data: new SlashCommandBuilder()
        .setName("membercount")
        .setDescription("Manage the team member count message")
        .setDefaultMemberPermissions(PermissionFlagsBits.ManageGuild)
        .addSubcommand(subcommand =>
            subcommand
                .setName("sync")
                .setDescription("Create or update the member count message from dashboard settings")
        )
        .addSubcommand(subcommand =>
            subcommand
                .setName("set")
                .setDescription("Set a team's player count")
                .addStringOption(option =>
                    option.setName("team").setDescription("Team name").setRequired(true)
                )
                .addIntegerOption(option =>
                    option.setName("players").setDescription("Player count").setRequired(true).setMinValue(0).setMaxValue(999)
                )
                .addStringOption(option =>
                    option.setName("status").setDescription("Recruitment status").setRequired(false)
                )
        )
        .addSubcommand(subcommand =>
            subcommand
                .setName("list")
                .setDescription("Show configured member counts")
        ),

    async execute(interaction) {
        const subcommand = interaction.options.getSubcommand();
        await interaction.deferReply({ ephemeral: true });

        if (subcommand === "sync") {
            const sync = await syncMemberCountMessage(interaction.client);
            await interaction.editReply(sync.skipped
                ? `Skipped: ${sync.reason}`
                : `Member count message ${sync.created ? "created" : "updated"} in <#${sync.channelId}>.`
            );
            return;
        }

        if (subcommand === "set") {
            const teamName = interaction.options.getString("team", true);
            const players = interaction.options.getInteger("players", true);
            const status = interaction.options.getString("status", false);
            const config = await loadDashboardConfig();
            const target = teamName.toLowerCase().replace(/[^a-z0-9]+/g, "");
            let found = false;
            let foundTeam = null;

            const teams = config.memberCounts.teams.map(team => {
                const names = [team.name, ...(team.aliases || [])].map(name => String(name).toLowerCase().replace(/[^a-z0-9]+/g, ""));
                if (!names.includes(target)) return team;
                found = true;
                foundTeam = team;
                return {
                    ...team,
                    players,
                    recruitmentStatus: status || team.recruitmentStatus
                };
            });

            if (!found) {
                await interaction.editReply(`Team **${teamName}** is not configured. Add it from the dashboard first.`);
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
                message: `<@${interaction.user.id}> set **${foundTeam.name}** to **${players}** players${status ? `, ${status}` : ""}.`,
                guildId: interaction.guildId,
                actorId: interaction.user.id,
                actorTag: interaction.user.tag || interaction.user.username,
                metadata: { teamId: foundTeam.id, players, status: status || "" }
            });

            const sync = await syncMemberCountMessage(interaction.client);
            await interaction.editReply(sync.skipped
                ? `Saved, but sync skipped: ${sync.reason}`
                : `Saved and updated <#${sync.channelId}>.`
            );
            return;
        }

        if (subcommand === "list") {
            const { memberCounts } = await loadDashboardConfig();
            await interaction.editReply(memberCounts.teams
                .map(team => `**${team.name}** - ${team.players} players, ${team.recruitmentStatus}`)
                .join("\n") || "No teams are configured."
            );
        }
    }
};
