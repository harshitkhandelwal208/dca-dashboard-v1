const { SlashCommandBuilder } = require("discord.js");
const {
    attachmentFor,
    buildSummaryEmbed,
    buildSummaryText,
    correctSpreadsheetSession,
    listSpreadsheetSessions,
    previewSpreadsheetSession,
    processSpreadsheetSession,
    rebuildSpreadsheetSession,
    resolveSessionForTeam,
    resolveTeamForInteraction,
    sendPeriodReport,
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
            { name: "Team type", value: "team_type" },
            { name: "Event name", value: "event_name" }
        );
}

function rowOption(option) {
    return option
        .setName("row")
        .setDescription("Placement/rank row to correct")
        .setMinValue(1)
        .setMaxValue(100)
        .setRequired(true);
}

function valueOption(option, description = "Replacement value") {
    return option
        .setName("value")
        .setDescription(description)
        .setMaxLength(120)
        .setRequired(true);
}

function teamTypeOption(option) {
    return option
        .setName("team_type")
        .setDescription("Corrected team classification")
        .addChoices(
            { name: "Own team", value: "own" },
            { name: "Opponent", value: "opponent" }
        )
        .setRequired(true);
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
        includeChart ? attachmentFor(session.outputs?.spreadsheetImagePath) : null
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

function previewText(session) {
    const rows = (session.players || []).slice(0, 20).map(player =>
        `#${player.rank} ${player.playerName} - ${player.teamType} - pts ${player.points ?? ""} - score ${player.score ?? ""}`
    );
    return [
        `Session: \`${session.id}\``,
        `Event: **${session.metadata?.title || session.teamEventName || "Team Event"}**`,
        `Players: **${session.players?.length || 0}** (${session.stats?.ownPlayers || 0} own, ${session.stats?.opponents || 0} opponents)`,
        `Missing own players scored 0: **${session.attendance?.missingPlayers?.length || 0}**`,
        "",
        rows.join("\n") || "No rows parsed."
    ].join("\n").slice(0, 1900);
}

async function postReportAndReply(interaction, teamConfig, period, reason) {
    const anchorDate = await resolveReportAnchor(interaction, teamConfig);
    const result = await sendPeriodReport(interaction.client, teamConfig, period, {
        anchorDate,
        force: true,
        reason
    });
    if (result.skipped) {
        await interaction.editReply(`No ${period} report was posted: ${result.reason}.`);
        return;
    }
    await interaction.editReply(`${period === "weekly" ? "Weekly" : "Monthly"} report regenerated and posted to the configured output channel.`);
}

module.exports = {
    data: new SlashCommandBuilder()
        .setName("spreadsheets")
        .setDescription("Manage Gemini team-event screenshot spreadsheet sessions")
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
                        .setName("rerun_gemini")
                        .setDescription("Download images and run Gemini Flash extraction again")
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
                .setDescription("Correct a parsed field and rebuild the spreadsheet")
                .addStringOption(teamOption)
                .addStringOption(option => sessionOption(option, true))
                .addIntegerOption(rowOption)
                .addStringOption(option =>
                    correctionFieldChoices(option
                        .setName("field")
                        .setDescription("Field to correct")
                        .setRequired(true)
                    )
                )
                .addStringOption(valueOption)
        )
        .addSubcommand(subcommand =>
            subcommand
                .setName("correct-name")
                .setDescription("Staff: correct a player name and rebuild")
                .addStringOption(teamOption)
                .addStringOption(option => sessionOption(option, true))
                .addIntegerOption(rowOption)
                .addStringOption(option => valueOption(option, "Correct player name"))
        )
        .addSubcommand(subcommand =>
            subcommand
                .setName("correct-team")
                .setDescription("Staff: correct a player team assignment and rebuild")
                .addStringOption(teamOption)
                .addStringOption(option => sessionOption(option, true))
                .addIntegerOption(rowOption)
                .addStringOption(teamTypeOption)
                .addStringOption(option => valueOption(option, "Optional team label").setRequired(false))
        )
        .addSubcommand(subcommand =>
            subcommand
                .setName("correct-placement")
                .setDescription("Staff: correct a placement/rank and rebuild")
                .addStringOption(teamOption)
                .addStringOption(option => sessionOption(option, true))
                .addIntegerOption(rowOption)
                .addIntegerOption(option =>
                    option
                        .setName("placement")
                        .setDescription("Correct placement/rank")
                        .setMinValue(1)
                        .setMaxValue(100)
                        .setRequired(true)
                )
        )
        .addSubcommand(subcommand =>
            subcommand
                .setName("correct-points")
                .setDescription("Staff: correct event points or score and rebuild")
                .addStringOption(teamOption)
                .addStringOption(option => sessionOption(option, true))
                .addIntegerOption(rowOption)
                .addStringOption(option =>
                    option
                        .setName("field")
                        .setDescription("Which numeric field to correct")
                        .addChoices(
                            { name: "Event points", value: "points" },
                            { name: "Score", value: "score" }
                        )
                        .setRequired(true)
                )
                .addStringOption(valueOption)
        )
        .addSubcommand(subcommand =>
            subcommand
                .setName("correct-event-name")
                .setDescription("Staff: correct the event name and rebuild")
                .addStringOption(teamOption)
                .addStringOption(option => sessionOption(option, true))
                .addStringOption(option => valueOption(option, "Correct event name"))
        )
        .addSubcommand(subcommand =>
            subcommand
                .setName("rebuild")
                .setDescription("Rebuild spreadsheet outputs from existing Gemini data and corrections")
                .addStringOption(teamOption)
                .addStringOption(option => sessionOption(option, true))
        )
        .addSubcommand(subcommand =>
            subcommand
                .setName("regenerate-weekly")
                .setDescription("Staff: regenerate and post the weekly report")
                .addStringOption(teamOption)
                .addStringOption(anchorDateOption)
        )
        .addSubcommand(subcommand =>
            subcommand
                .setName("regenerate-monthly")
                .setDescription("Staff: regenerate and post the monthly report")
                .addStringOption(teamOption)
                .addStringOption(anchorDateOption)
        )
        .addSubcommand(subcommand =>
            subcommand
                .setName("test-gemini")
                .setDescription("TEMP: test Gemini Flash extraction for a session")
                .addStringOption(teamOption)
                .addStringOption(option => sessionOption(option, false))
        )
        .addSubcommand(subcommand =>
            subcommand
                .setName("test-grouping")
                .setDescription("TEMP: inspect current session grouping state")
                .addStringOption(teamOption)
        )
        .addSubcommand(subcommand =>
            subcommand
                .setName("preview")
                .setDescription("TEMP: preview parsed output without posting final files")
                .addStringOption(teamOption)
                .addStringOption(option => sessionOption(option, false))
        )
        .addSubcommand(subcommand =>
            subcommand
                .setName("rebuild-event")
                .setDescription("TEMP: rebuild one event output")
                .addStringOption(teamOption)
                .addStringOption(option => sessionOption(option, true))
        )
        .addSubcommand(subcommand =>
            subcommand
                .setName("force-weekly")
                .setDescription("TEMP: force weekly report generation")
                .addStringOption(teamOption)
                .addStringOption(anchorDateOption)
        )
        .addSubcommand(subcommand =>
            subcommand
                .setName("force-monthly")
                .setDescription("TEMP: force monthly report generation")
                .addStringOption(teamOption)
                .addStringOption(anchorDateOption)
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
                `Gemini model: **${config.spreadsheets.geminiModel || process.env.GEMINI_FLASH_MODEL || "gemini-2.5-flash"}**`,
                `Gemini timeout: **${Math.round((config.spreadsheets.geminiTimeoutMs || Number(process.env.GEMINI_TIMEOUT_MS) || 300000) / 1000)} second(s)**`,
                `Gemini retries: **${config.spreadsheets.geminiMaxRetries ?? process.env.GEMINI_MAX_RETRIES ?? 4}**`,
                `Raw data retention: **${config.spreadsheets.rawDataRetentionDays || 31} day(s)**`,
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
                rerunOcr: interaction.options.getBoolean("rerun_gemini") === true
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

        if (subcommand === "correct-name") {
            const sessionId = interaction.options.getString("session_id", true);
            const session = await resolveSessionForTeam(teamConfig.id, sessionId, ["processed"]);
            if (!session) {
                await interaction.editReply(`No processed session \`${sessionId}\` exists for **${teamConfig.name}**.`);
                return;
            }
            const corrected = await correctSpreadsheetSession(interaction.client, session.id, {
                row: interaction.options.getInteger("row", true),
                field: "player_name",
                value: interaction.options.getString("value", true),
                actorId: interaction.user.id,
                actorTag: interaction.user.tag || interaction.user.username
            });
            await interaction.editReply({
                content: `Player name correction applied to \`${corrected.id}\`.`,
                embeds: [buildSummaryEmbed(corrected)],
                files: filesForSession(corrected)
            });
            return;
        }

        if (subcommand === "correct-team") {
            const sessionId = interaction.options.getString("session_id", true);
            const session = await resolveSessionForTeam(teamConfig.id, sessionId, ["processed"]);
            if (!session) {
                await interaction.editReply(`No processed session \`${sessionId}\` exists for **${teamConfig.name}**.`);
                return;
            }
            const row = interaction.options.getInteger("row", true);
            const actor = {
                actorId: interaction.user.id,
                actorTag: interaction.user.tag || interaction.user.username
            };
            let corrected = await correctSpreadsheetSession(interaction.client, session.id, {
                row,
                field: "team_type",
                value: interaction.options.getString("team_type", true),
                ...actor
            });
            const label = interaction.options.getString("value", false);
            if (label) {
                corrected = await correctSpreadsheetSession(interaction.client, session.id, {
                    row,
                    field: "team_name",
                    value: label,
                    ...actor
                });
            }
            await interaction.editReply({
                content: `Team correction applied to \`${corrected.id}\`.`,
                embeds: [buildSummaryEmbed(corrected)],
                files: filesForSession(corrected)
            });
            return;
        }

        if (subcommand === "correct-placement") {
            const sessionId = interaction.options.getString("session_id", true);
            const session = await resolveSessionForTeam(teamConfig.id, sessionId, ["processed"]);
            if (!session) {
                await interaction.editReply(`No processed session \`${sessionId}\` exists for **${teamConfig.name}**.`);
                return;
            }
            const corrected = await correctSpreadsheetSession(interaction.client, session.id, {
                row: interaction.options.getInteger("row", true),
                field: "placement",
                value: String(interaction.options.getInteger("placement", true)),
                actorId: interaction.user.id,
                actorTag: interaction.user.tag || interaction.user.username
            });
            await interaction.editReply({
                content: `Placement correction applied to \`${corrected.id}\`.`,
                embeds: [buildSummaryEmbed(corrected)],
                files: filesForSession(corrected)
            });
            return;
        }

        if (subcommand === "correct-points") {
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
                content: `Points correction applied to \`${corrected.id}\`.`,
                embeds: [buildSummaryEmbed(corrected)],
                files: filesForSession(corrected)
            });
            return;
        }

        if (subcommand === "correct-event-name") {
            const sessionId = interaction.options.getString("session_id", true);
            const session = await resolveSessionForTeam(teamConfig.id, sessionId, ["processed"]);
            if (!session) {
                await interaction.editReply(`No processed session \`${sessionId}\` exists for **${teamConfig.name}**.`);
                return;
            }
            const corrected = await correctSpreadsheetSession(interaction.client, session.id, {
                row: 1,
                field: "event_name",
                value: interaction.options.getString("value", true),
                actorId: interaction.user.id,
                actorTag: interaction.user.tag || interaction.user.username
            });
            await interaction.editReply({
                content: `Event name correction applied to \`${corrected.id}\`.`,
                embeds: [buildSummaryEmbed(corrected)],
                files: filesForSession(corrected)
            });
            return;
        }

        if (subcommand === "regenerate-weekly") {
            await postReportAndReply(interaction, teamConfig, "weekly", "staff-regenerate-weekly");
            return;
        }

        if (subcommand === "regenerate-monthly") {
            await postReportAndReply(interaction, teamConfig, "monthly", "staff-regenerate-monthly");
            return;
        }

        if (subcommand === "test-gemini") {
            const session = await resolveSessionForTeam(
                teamConfig.id,
                interaction.options.getString("session_id", false),
                ["pending", "processed", "failed"]
            );
            if (!session) {
                await interaction.editReply(`No spreadsheet session found for **${teamConfig.name}**.`);
                return;
            }
            const preview = await previewSpreadsheetSession(session.id, { rerunGemini: true });
            await interaction.editReply(`TEMP Gemini test complete.\n${previewText(preview)}`);
            return;
        }

        if (subcommand === "test-grouping") {
            const sessions = await listSpreadsheetSessions({ teamId: teamConfig.id });
            const pending = sessions.filter(session => session.status === "pending").slice(0, 10);
            await interaction.editReply([
                "TEMP grouping inspection.",
                `Grouping window: **${config.spreadsheets.sessionWindowMinutes || 5} minute(s)**`,
                `Pending sessions: **${pending.length}**`,
                pending.map(session =>
                    `\`${session.id}\` - ${session.images?.length || 0} image(s), author ${session.authorTag || session.authorId}, last image ${new Date(session.lastImageAt || session.updatedAt || session.createdAt).toLocaleString()}`
                ).join("\n") || "No pending sessions."
            ].join("\n"));
            return;
        }

        if (subcommand === "preview") {
            const session = await resolveSessionForTeam(
                teamConfig.id,
                interaction.options.getString("session_id", false),
                ["pending", "processed", "failed"]
            );
            if (!session) {
                await interaction.editReply(`No spreadsheet session found for **${teamConfig.name}**.`);
                return;
            }
            const preview = await previewSpreadsheetSession(session.id, { rerunGemini: false });
            await interaction.editReply(`TEMP parsed preview.\n${previewText(preview)}`);
            return;
        }

        if (subcommand === "force-weekly") {
            await postReportAndReply(interaction, teamConfig, "weekly", "temp-force-weekly");
            return;
        }

        if (subcommand === "force-monthly") {
            await postReportAndReply(interaction, teamConfig, "monthly", "temp-force-monthly");
            return;
        }

        if (subcommand === "rebuild" || subcommand === "rebuild-event") {
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
