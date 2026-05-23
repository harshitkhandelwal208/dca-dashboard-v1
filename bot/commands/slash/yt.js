const { PermissionFlagsBits, SlashCommandBuilder } = require("discord.js");
const { loadDashboardConfig } = require("../../utils/dashboardConfig");
const { checkYouTubeFeeds } = require("../../utils/youtubeManager");

module.exports = {
    data: new SlashCommandBuilder()
        .setName("yt")
        .setDescription("Inspect dashboard-managed YouTube notifications")
        .setDefaultMemberPermissions(PermissionFlagsBits.ManageGuild)
        .addSubcommand(subcommand =>
            subcommand
                .setName("list")
                .setDescription("Show configured YouTube feeds")
        )
        .addSubcommand(subcommand =>
            subcommand
                .setName("check")
                .setDescription("Run one YouTube feed check now")
        ),

    async execute(interaction) {
        const subcommand = interaction.options.getSubcommand();
        await interaction.deferReply({ ephemeral: true });

        if (subcommand === "list") {
            const { youtube } = await loadDashboardConfig();
            const rows = youtube.feeds.map(feed => {
                const status = feed.enabled ? "enabled" : "disabled";
                const channel = feed.channelId || youtube.defaultChannelId;
                return `**${feed.name}** - ${status} - ${channel ? `<#${channel}>` : "no Discord channel"}`;
            });

            await interaction.editReply(rows.join("\n") || "No YouTube feeds are configured.");
            return;
        }

        if (subcommand === "check") {
            const result = await checkYouTubeFeeds(interaction.client);
            if (result.skipped) {
                await interaction.editReply(`Skipped: ${result.reason}`);
                return;
            }

            await interaction.editReply(result.results
                .map(item => {
                    if (item.error) return `**${item.name}** - error: ${item.error}`;
                    if (item.posted) return `**${item.name}** - posted ${item.videoId}`;
                    if (item.initialized) return `**${item.name}** - initialized ${item.videoId}`;
                    if (item.unchanged) return `**${item.name}** - unchanged`;
                    return `**${item.name}** - skipped: ${item.reason || "no change"}`;
                })
                .join("\n") || "No feeds checked.");
        }
    }
};
