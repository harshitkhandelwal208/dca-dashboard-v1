const { SlashCommandBuilder } = require("discord.js");
const {
    attachmentFor,
    buildSummaryEmbed,
    buildSummaryText,
    correctSpreadsheetSession,
    listSpreadsheetSessions,
    processSpreadsheetSession,
    rebuildSpreadsheetSession,
    resolveSessionForTeam,
    resolveTeamForInteraction,
    sendSessionOutput
} = require("../../utils/spreadsheetManager");
const {
    buildPeriodReportEmbed,
    generatePeriodReport,
    reportAttachment
} = require("../../utils/spreadsheetReports");

function teamOption(option) {
    return option
        .setName("team")
        .setDescription("Configured spreadsheet team name or ID")
        .setMaxLength(80)
        .setRequired(true);
}

function sessionOption(option, required = false) {
    return option
        .setName("session_id")
        .setDescription("Spreadsheet session ID. Defaults to the latest matching session.")
        .setMaxLength(80)
        .setRequired(required);
}

function correctionFieldChoices(option) {
    return option
        .addChoices(
            { name: "Player name", value: "player_name" },
            { name: "Team name", value: "team_name" },
            { name: "Placement", value: "placement" },
            { name: "Points", value: "points" },
            { name: "Score", value: "score" },
            { name: "Team type", value: "team_type" }
        );
}

function anchorDateOption(option) {
    return option
        .setName("anchor_date")
        .setDescription("Optional YYYY-MM-DD date inside the week or month to report. Defaults to latest processed session.")
        .setMaxLength(10)
        .setRequired(false);
}

function filesForSession(session, includeChart = true) {
    return [
        attachmentFor(session.outputs?.spreadsheetPath),
        includeChart ? attachmentFor(session.outputs?.chartPath) : null
    ].filter(Boolean);
}

function dateForSession(session) {
    const value = session?.processedAt || session?.updatedAt || session?.createdAt || "";
    const date = new Date(value);
    return Number.isNaN(date.getTime()) ? null : date;
}

function parseAnchorDate(value) {
    if (!value) return null;
    if (!/^\d{4}-\d{2}-\d{2}$/.test(value)) {
        throw new Error("anchor_date must use YYYY-MM-DD format.");
    }
    const date = new Date(`${value}T12:00:00.000Z`);
    if (Number.isNaN(date.getTime())) {
        throw new Error("anchor_date is not a valid date.");
    }
    return date;
}

async function resolveReportAnchor(interaction, teamConfig) {
    const explicit = parseAnchorDate(interaction.options.getString("anchor_date", false));
    if (explicit) return explicit;

    const latest = (await listSpreadsheetSessions({ teamId: teamConfig.id }))
        .find(session => session.status === "processed");
    return dateForSession(latest) || new Date();
}

async function replyWithPeriodReport(interaction, teamConfig, period) {
    const anchorDate = await resolveReportAnchor(interaction, teamConfig);
    const report = await generatePeriodReport(teamConfig, period, { anchorDate });
    if (!report) {
        await interaction.editReply(`No processed spreadsheet sessions were found for this ${period === "weekly" ? "week" : "month"}.`);
        return;
    }

    const attachment = reportAttachment(report);
    await interaction.editReply({
        content: `${period === "weekly" ? "Weekly" : "Monthly"} report for **${teamConfig.name}**.`,
        embeds: [buildPeriodReportEmbed(report)],
        files: attachment ? [attachment] : []
    });
}

module.exports = {
    data: new SlashCommandBuilder()
        .setName("spreadsheets")
        .setDescription("Manage race screenshot OCR spreadsheet sessions")
        .addSubcommand(subcommand =>
            subcommand
                .setName("status")
                .setDescription("Show configured spreadsheet status for a team")
                .addStringOption(teamOption)
        )
        .addSubcommand(subcommand =>
            subcommand
                .setName("sessions")
                .setDescription("List recent spreadsheet sessions for a team")
                .addStringOption(teamOption)
        )
        .addSubcommand(subcommand =>
            subcommand
                .setName("generate")
                .setDescription("Generate or refresh a spreadsheet from a pending screenshot session")
                .addStringOption(teamOption)
                .addStringOption(option => sessionOption(option, false))
                .addBooleanOption(option =>
                    option
                        .setName("rerun_ocr")
                        .setDescription("Download images and run OCR again")
                        .setRequired(false)
                )
        )
        .addSubcommand(subcommand =>
            subcommand
                .setName("summary")
                .setDescription("View the parsed summary for a spreadsheet session")
                .addStringOption(teamOption)
                .addStringOption(option => sessionOption(option, false))
        )
        .addSubcommand(subcommand =>
            subcommand
                .setName("weekly")
                .setDescription("Build the weekly team-event report with missed events scored as zero")
                .addStringOption(teamOption)
                .addStringOption(anchorDateOption)
        )
        .addSubcommand(subcommand =>
            subcommand
                .setName("monthly")
                .setDescription("Build the monthly team-event report with missed events scored as zero")
                .addStringOption(teamOption)
                .addStringOption(anchorDateOption)
        )
        .addSubcommand(subcommand =>
            subcommand
                .setName("file")
                .setDescription("Get the generated Calc spreadsheet file")
                .addStringOption(teamOption)
                .addStringOption(option => sessionOption(option, false))
        )
        .addSubcommand(subcommand =>
            subcommand
                .setName("chart")
                .setDescription("Get the generated summary chart")
                .addStringOption(teamOption)
                .addStringOption(option => sessionOption(option, false))
        )
        .addSubcommand(subcommand =>
            subcommand
                .setName("correct")
                .setDescription("Correct an OCR field and rebuild the spreadsheet")
                .addStringOption(teamOption)
                .addStringOption(option => sessionOption(option, true))
                .addIntegerOption(option =>
                    option
                        .setName("row")
                        .setDescription("Placement/rank row to correct")
                        .setMinValue(1)
                        .setMaxValue(80)
                        .setRequired(true)
                )
                .addStringOption(option =>
                    correctionFieldChoices(option
                        .setName("field")
                        .setDescription("Field to correct")
                        .setRequired(true)
                    )
                )
                .addStringOption(option =>
                    option
                        .setName("value")
                        .setDescription("Replacement value")
                        .setMaxLength(120)
                        .setRequired(true)
                )
        )
        .addSubcommand(subcommand =>
            subcommand
                .setName("rebuild")
                .setDescription("Rebuild spreadsheet outputs from existing OCR and corrections")
                .addStringOption(teamOption)
                .addStringOption(option => sessionOption(option, true))
        ),

    async execute(interaction) {
        await interaction.deferReply({ ephemeral: true });

        const subcommand = interaction.options.getSubcommand();
        const teamValue = interaction.options.getString("team", true);
        const { config, teamConfig } = await resolveTeamForInteraction(interaction, teamValue);
        if (!teamConfig) return;

        if (subcommand === "status") {
            const sessions = await listSpreadsheetSessions({ teamId: teamConfig.id });
            const pending = sessions.filter(session => session.status === "pending").length;
            const processed = sessions.filter(session => session.status === "processed").length;
            await interaction.editReply([
                `Team: **${teamConfig.name}**`,
                `Enabled: **${teamConfig.enabled ? "yes" : "no"}**`,
                `Monitored channel: ${teamConfig.monitoredChannelId ? `<#${teamConfig.monitoredChannelId}>` : "not set"}`,
                `Output channel: ${teamConfig.outputChannelId ? `<#${teamConfig.outputChannelId}>` : "submission channel"}`,
                `Access role: ${teamConfig.accessRoleId ? `<@&${teamConfig.accessRoleId}>` : "admins only"}`,
                `Grouping window: **${config.spreadsheets.sessionWindowMinutes} minute(s)**`,
                `Output format: **${config.spreadsheets.outputFormat}**`,
                `Sessions: **${sessions.length}** total, **${pending}** pending, **${processed}** processed`
            ].join("\n"));
            return;
        }

        if (subcommand === "sessions") {
            const sessions = (await listSpreadsheetSessions({ teamId: teamConfig.id })).slice(0, 10);
            if (!sessions.length) {
                await interaction.editReply(`No spreadsheet sessions have been captured for **${teamConfig.name}** yet.`);
                return;
            }
            await interaction.editReply(sessions.map(session =>
                `\`${session.id}\` - **${session.status}** - ${session.images?.length || 0} image(s) - ${new Date(session.updatedAt || session.createdAt).toLocaleString()}`
            ).join("\n"));
            return;
        }

        if (subcommand === "generate") {
            const session = await resolveSessionForTeam(
                teamConfig.id,
                interaction.options.getString("session_id", false),
                []
            );
            if (!session) {
                await interaction.editReply(`No spreadsheet session found for **${teamConfig.name}**.`);
                return;
            }

            const processed = await processSpreadsheetSession(interaction.client, session.id, {
                rerunOcr: interaction.options.getBoolean("rerun_ocr") === true
            });
            await interaction.editReply({
                embeds: [buildSummaryEmbed(processed)],
                files: filesForSession(processed)
            });
            await sendSessionOutput(interaction.client, processed).catch(error => {
                console.error("Spreadsheet output channel post failed:", error.message);
            });
            return;
        }

        if (subcommand === "summary") {
            const session = await resolveSessionForTeam(
                teamConfig.id,
                interaction.options.getString("session_id", false),
                ["processed", "failed", "pending"]
            );
            if (!session) {
                await interaction.editReply(`No spreadsheet session found for **${teamConfig.name}**.`);
                return;
            }
            await interaction.editReply(buildSummaryText(session));
            return;
        }

        if (subcommand === "weekly") {
            await replyWithPeriodReport(interaction, teamConfig, "weekly");
            return;
        }

        if (subcommand === "monthly") {
            await replyWithPeriodReport(interaction, teamConfig, "monthly");
            return;
        }

        if (subcommand === "file") {
            const session = await resolveSessionForTeam(
                teamConfig.id,
                interaction.options.getString("session_id", false),
                ["processed"]
            );
            if (!session) {
                await interaction.editReply(`No processed spreadsheet session found for **${teamConfig.name}**.`);
                return;
            }
            const files = filesForSession(session, false);
            await interaction.editReply(files.length
                ? { content: `Spreadsheet for \`${session.id}\`.`, files }
                : `No spreadsheet file exists for \`${session.id}\`. Rebuild it first.`
            );
            return;
        }

        if (subcommand === "chart") {
            const session = await resolveSessionForTeam(
                teamConfig.id,
                interaction.options.getString("session_id", false),
                ["processed"]
            );
            if (!session) {
                await interaction.editReply(`No processed spreadsheet session found for **${teamConfig.name}**.`);
                return;
            }
            const chart = attachmentFor(session.outputs?.chartPath);
            await interaction.editReply(chart
                ? { content: `Chart for \`${session.id}\`.`, files: [chart] }
                : `No chart exists for \`${session.id}\`. Rebuild it first.`
            );
            return;
        }

        if (subcommand === "correct") {
            const sessionId = interaction.options.getString("session_id", true);
            const session = await resolveSessionForTeam(teamConfig.id, sessionId, ["processed"]);
            if (!session) {
                await interaction.editReply(`No processed session \`${sessionId}\` exists for **${teamConfig.name}**.`);
                return;
            }

            const corrected = await correctSpreadsheetSession(interaction.client, session.id, {
                row: interaction.options.getInteger("row", true),
                field: interaction.options.getString("field", true),
                value: interaction.options.getString("value", true),
                actorId: interaction.user.id,
                actorTag: interaction.user.tag || interaction.user.username
            });
            await interaction.editReply({
                content: `Correction applied to \`${corrected.id}\` and outputs were rebuilt.`,
                embeds: [buildSummaryEmbed(corrected)],
                files: filesForSession(corrected)
            });
            return;
        }

        if (subcommand === "rebuild") {
            const sessionId = interaction.options.getString("session_id", true);
            const session = await resolveSessionForTeam(teamConfig.id, sessionId, ["processed"]);
            if (!session) {
                await interaction.editReply(`No processed session \`${sessionId}\` exists for **${teamConfig.name}**.`);
                return;
            }

            const rebuilt = await rebuildSpreadsheetSession(interaction.client, session.id);
            await interaction.editReply({
                content: `Rebuilt \`${rebuilt.id}\`.`,
                embeds: [buildSummaryEmbed(rebuilt)],
                files: filesForSession(rebuilt)
            });
        }
    }
};
