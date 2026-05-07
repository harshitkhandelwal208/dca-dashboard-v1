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
                    name: "Spreadsheets",
                    value: "`/spreadsheets generate`, `/spreadsheets summary`, `/spreadsheets weekly`, `/spreadsheets monthly`, `/spreadsheets correct`, `/spreadsheets rebuild`, `/spreadsheets file`"
                },
                {
                    name: "Auto Reports",
                    value: "Processed team events post rebuilt weekly and monthly XLSX reports to the configured output channel. Missing event scores become `0`; `#KAB` means ranked above every opponent."
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
