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
                    value: "`/tickets setup`, `/tickets status`, `/tickets logs`, `/tickets claim`, `/tickets close`, `/tickets add`, `/tickets remove`, `/tickets screenshot-list`, `screenshot-add`, `screenshot-change`, `screenshot-remove`, `/tickets archive`, `/tickets delete`, `/invite`, `/ban`"
                },
                {
                    name: "Spreadsheets And Team Events",
                    value: "Submit one or more screenshots in the configured monitored channel. Images from the same user/channel are grouped for the dashboard grouping window, then Gemini Flash extracts event name, rows, scores, teams, podium data, and visible metadata."
                },
                {
                    name: "Gemini Output And Files",
                    value: "Normal event output posts the final `.xlsx`, a full spreadsheet image, and a chart image to the configured output channel. Event names come from visible screenshot titles. Raw Gemini data is kept temporarily for audit and cleaned by retention."
                },
                {
                    name: "#KAB And Missed Events",
                    value: "`#KAB` is the count of events where a player ranked above every single opponent. If a known own-team player is absent from an event, that event is scored as `0` in summaries, weekly reports, and monthly reports."
                },
                {
                    name: "Reports And Corrections",
                    value: "Weekly reports post when the team event name changes. Monthly reports post after month end. Staff can use `/spreadsheets correct-name`, `correct-team`, `correct-placement`, `correct-points`, `correct-event-name`, `rebuild`, `regenerate-weekly`, and `regenerate-monthly`."
                },
                {
                    name: "Temporary Spreadsheet Tests",
                    value: "`/spreadsheets test-gemini`, `test-grouping`, `preview`, `rebuild-event`, `force-weekly`, and `force-monthly` are temporary development commands and can be removed later."
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
