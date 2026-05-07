const { SlashCommandBuilder } = require("discord.js");
const { loadDashboardConfig } = require("../../utils/dashboardConfig");
const { getDashboardUrl } = require("../../utils/serverConfig");

module.exports = {
    data: new SlashCommandBuilder()
        .setName("dashboard")
        .setDescription("Get a quick link to the bot dashboard."),

    async execute(interaction) {
        const config = await loadDashboardConfig();
        const url = getDashboardUrl(config);

        await interaction.reply({
            content: url ? `Dashboard: ${url}` : "Dashboard URL is not configured yet.",
            ephemeral: true
        });
    }
};
