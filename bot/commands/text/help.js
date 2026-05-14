const { EmbedBuilder } = require("discord.js");
const { loadDashboardConfig } = require("../../utils/dashboardConfig");
const { getDashboardUrl } = require("../../utils/serverConfig");

module.exports = {
    name: "help",
    description: "Shows current bot commands and report workflows",

    async execute(message) {
        const commands = message.client.commands;
        const config = await loadDashboardConfig().catch(() => null);
        const dashboardUrl = getDashboardUrl(config);

        const commandList = commands && commands.size
            ? commands
                .map(command => `- \`-${command.name}\` - ${command.description || "No description"}`)
                .join("\n")
                .slice(0, 1000)
            : "No prefix commands found.";

        const embed = new EmbedBuilder()
            .setTitle("DCA Bot Help")
            .setDescription("Use slash commands for dashboard-backed features. Prefix commands remain available for older moderation utilities.")
            .setColor("#00b0f4")
            .addFields(
                {
                    name: "Team Event Spreadsheets",
                    value: "Post screenshots in the configured channel. The bot groups images by user/channel and window, uses Gemini Flash to extract event names, ranks, players, scores, teams, podium data, and posts the final XLSX plus full spreadsheet and chart images."
                },
                {
                    name: "#KAB, Reports, Corrections",
                    value: "`#KAB` means the player ranked above every opponent. Missing known own-team players score `0`. Weekly reports post when event names change; monthly reports post after month end. Use `/spreadsheets correct-*`, `rebuild`, `regenerate-weekly`, and `regenerate-monthly` for fixes."
                },
                {
                    name: "Temporary Spreadsheet Tests",
                    value: "`/spreadsheets test-gemini`, `test-grouping`, `preview`, `rebuild-event`, `force-weekly`, `force-monthly` are temporary development commands."
                },
                {
                    name: "Recruitment",
                    value: "`/tickets status`, `/tickets claim`, `/tickets close`, `/tickets logs`, `/invite`, `/ban`"
                },
                {
                    name: "Team Counts And Feeds",
                    value: "`/membercount set`, `/membercount sync`, `/teamcount`, `/updatecount`, `/yt`, `/remindMe`"
                },
                {
                    name: "Prefix Commands",
                    value: commandList
                },
                {
                    name: "Dashboard",
                    value: dashboardUrl || "Dashboard URL is not configured."
                }
            )
            .setFooter({ text: `Prefix commands: ${commands?.size || 0}` });

        await message.channel.send({ embeds: [embed] });
    }
};
