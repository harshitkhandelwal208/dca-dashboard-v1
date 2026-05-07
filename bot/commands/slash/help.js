const { EmbedBuilder, SlashCommandBuilder } = require("discord.js");
const { loadDashboardConfig } = require("../../utils/dashboardConfig");
const { getDashboardUrl } = require("../../utils/serverConfig");

module.exports = {
    data: new SlashCommandBuilder()
        .setName("help")
        .setDescription("Show useful bot commands and dashboard links."),

    async execute(interaction) {
        const config = await loadDashboardConfig();
        const dashboardUrl = getDashboardUrl(config);

        const embed = new EmbedBuilder()
            .setTitle("DCA Bot Help")
            .setColor(0x37d6a7)
            .setDescription("Core commands for recruitment, team management, reports, moderation, reminders, and dashboard administration.")
            .addFields(
                {
                    name: "Recruitment",
                    value: "`/tickets setup`, `/tickets status`, `/tickets logs`, `/tickets claim`, `/tickets close`, `/tickets add`, `/tickets remove`, `/tickets tutorial`, `/tickets archive`, `/tickets delete`, `/invite`, `/ban`"
                },
                {
                    name: "Spreadsheets And Team Events",
                    value: "`/spreadsheets status`, `/spreadsheets sessions`, `/spreadsheets generate`, `/spreadsheets summary`, `/spreadsheets weekly`, `/spreadsheets monthly`, `/spreadsheets correct`, `/spreadsheets rebuild`, `/spreadsheets file`, `/spreadsheets chart`"
                },
                {
                    name: "Spreadsheet Reports",
                    value: "Processed team events automatically rebuild weekly and monthly reports in the configured output channel. Missing event scores are set to `0`; `#KAB` counts events where a player ranked above every opponent."
                },
                {
                    name: "Teams And Counts",
                    value: "`/membercount set`, `/membercount sync`, `/teamcount`, `/updatecount`, `/top3kms`, `/top3TE`, `/TEsummaryFDC`"
                },
                {
                    name: "Moderation",
                    value: "`/warn`, `/clearwarns`, `/whois`, `/snap`, `/roles`, prefix commands like `-clean`, `-kick`, `-mute`, `-unban`"
                },
                {
                    name: "Reminders And Feeds",
                    value: "`/remindMe`, `/reminders`, `/cancelreminder`, `/yt`"
                },
                {
                    name: "Dashboard",
                    value: dashboardUrl ? `[Open dashboard](${dashboardUrl})` : "`/dashboard` or configure a dashboard URL in the Server page."
                }
            )
            .setTimestamp();

        await interaction.reply({ embeds: [embed], ephemeral: true });
    }
};
