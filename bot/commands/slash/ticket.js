const {
    ChannelType,
    PermissionFlagsBits,
    SlashCommandBuilder
} = require("discord.js");
const { loadDashboardConfig, saveDashboardConfig } = require("../../utils/dashboardConfig");
const { syncRecruitmentBanList } = require("../../utils/recruitmentBanPanel");
const {
    RECRUITMENT_OUTCOMES,
    addUserToTicket,
    archiveTicket,
    claimTicket,
    deleteTicket,
    ensureRecruitmentPanel,
    finishClose,
    massAddUsersToTicket,
    removeUserFromTicket,
    renameTicket,
    sendTutorial,
    startClose
} = require("../../utils/recruitmentManager");
const { listRecruitmentLogs, listTickets } = require("../../utils/recruitmentStore");

function optionalText(interaction, name, fallback) {
    const value = interaction.options.getString(name);
    return value === null ? fallback : value;
}

function formatSync(sync) {
    if (sync.skipped) return `Saved, but panel sync was skipped: ${sync.reason}`;
    return `Recruitment panel ${sync.created ? "created" : "synced"} in <#${sync.channelId}>.`;
}

function hasManageGuild(interaction) {
    const permissions = interaction.memberPermissions;
    return Boolean(
        permissions?.has(PermissionFlagsBits.Administrator) ||
        permissions?.has(PermissionFlagsBits.ManageGuild)
    );
}

async function requireManager(interaction) {
    if (hasManageGuild(interaction)) return true;
    await interaction.editReply("You need **Manage Server** to change ticket configuration.");
    return false;
}

function outcomeChoices(option) {
    for (const outcome of RECRUITMENT_OUTCOMES) {
        option.addChoices({ name: outcome.label, value: outcome.id });
    }
    return option;
}

module.exports = {
    data: new SlashCommandBuilder()
        .setName("tickets")
        .setDescription("Manage recruitment tickets")
        .addSubcommand(subcommand =>
            subcommand
                .setName("setup")
                .setDescription("Configure and post the recruitment Apply panel")
                .addChannelOption(option =>
                    option
                        .setName("channel")
                        .setDescription("Channel where the Apply panel should be posted")
                        .addChannelTypes(ChannelType.GuildText, ChannelType.GuildAnnouncement)
                        .setRequired(true)
                )
                .addChannelOption(option =>
                    option
                        .setName("log_channel")
                        .setDescription("Channel where applicant ticket images should be sent on close")
                        .addChannelTypes(ChannelType.GuildText, ChannelType.GuildAnnouncement)
                        .setRequired(false)
                )
                .addRoleOption(option =>
                    option
                        .setName("recruiter_role")
                        .setDescription("Role that can manage recruitment tickets")
                        .setRequired(false)
                )
                .addStringOption(option =>
                    option
                        .setName("title")
                        .setDescription("Panel embed title")
                        .setMaxLength(120)
                        .setRequired(false)
                )
                .addStringOption(option =>
                    option
                        .setName("description")
                        .setDescription("Panel embed body")
                        .setMaxLength(2000)
                        .setRequired(false)
                )
                .addBooleanOption(option =>
                    option
                        .setName("private_threads")
                        .setDescription("Create private application threads when possible")
                        .setRequired(false)
                )
        )
        .addSubcommand(subcommand =>
            subcommand
                .setName("sync-panel")
                .setDescription("Refresh the recruitment Apply panel from saved settings")
        )
        .addSubcommand(subcommand =>
            subcommand
                .setName("sync-banlist")
                .setDescription("Refresh the recruitment team ban list embeds")
        )
        .addSubcommand(subcommand =>
            subcommand
                .setName("status")
                .setDescription("Show current recruitment ticket settings")
        )
        .addSubcommand(subcommand =>
            subcommand
                .setName("logs")
                .setDescription("Show recent recruitment outcomes")
        )
        .addSubcommand(subcommand =>
            subcommand
                .setName("claim")
                .setDescription("Claim the recruitment ticket thread you are in")
        )
        .addSubcommand(subcommand =>
            subcommand
                .setName("close")
                .setDescription("Close this recruitment ticket and log the final outcome")
                .addStringOption(option =>
                    outcomeChoices(option
                        .setName("outcome")
                        .setDescription("Select a team or reject the application")
                        .setRequired(false)
                    )
                )
        )
        .addSubcommand(subcommand =>
            subcommand
                .setName("add")
                .setDescription("Add a user to this ticket thread")
                .addUserOption(option =>
                    option
                        .setName("user")
                        .setDescription("User to add")
                        .setRequired(true)
                )
        )
        .addSubcommand(subcommand =>
            subcommand
                .setName("massadd")
                .setDescription("Add multiple users to this ticket by mentions or IDs")
                .addStringOption(option =>
                    option
                        .setName("users")
                        .setDescription("Mentions or user IDs separated by spaces")
                        .setMaxLength(1000)
                        .setRequired(true)
                )
        )
        .addSubcommand(subcommand =>
            subcommand
                .setName("remove")
                .setDescription("Remove a user from this ticket thread")
                .addUserOption(option =>
                    option
                        .setName("user")
                        .setDescription("User to remove")
                        .setRequired(true)
                )
        )
        .addSubcommand(subcommand =>
            subcommand
                .setName("rename")
                .setDescription("Rename this recruitment ticket thread")
                .addStringOption(option =>
                    option
                        .setName("name")
                        .setDescription("New thread name")
                        .setMaxLength(90)
                        .setRequired(true)
                )
        )
        .addSubcommand(subcommand =>
            subcommand
                .setName("tutorial")
                .setDescription("Send one configured dashboard tutorial into this ticket")
                .addStringOption(option =>
                    option
                        .setName("tutorial_id")
                        .setDescription("Tutorial ID from the dashboard, e.g. license-screenshot")
                        .setMaxLength(80)
                        .setRequired(true)
                )
        )
        .addSubcommand(subcommand =>
            subcommand
                .setName("archive")
                .setDescription("Lock and archive this ticket thread")
        )
        .addSubcommand(subcommand =>
            subcommand
                .setName("delete")
                .setDescription("Delete this ticket thread")
        ),

    async execute(interaction) {
        const subcommand = interaction.options.getSubcommand();
        await interaction.deferReply({ ephemeral: true });

        if (subcommand === "setup") {
            if (!(await requireManager(interaction))) return;

            const config = await loadDashboardConfig();
            const channel = interaction.options.getChannel("channel", true);
            const logChannel = interaction.options.getChannel("log_channel", false);
            const recruiterRole = interaction.options.getRole("recruiter_role", false);
            const privateThreads = interaction.options.getBoolean("private_threads");

            const nextRecruitment = {
                ...config.recruitment,
                enabled: true,
                panelChannelId: channel.id,
                panelTitle: optionalText(interaction, "title", config.recruitment.panelTitle),
                panelDescription: optionalText(interaction, "description", config.recruitment.panelDescription),
                logChannelId: logChannel?.id || config.recruitment.logChannelId,
                recruiterRoleId: recruiterRole?.id || config.recruitment.recruiterRoleId,
                privateThreads: privateThreads === null ? config.recruitment.privateThreads : privateThreads
            };

            await saveDashboardConfig({
                ...config,
                bot: {
                    ...config.bot,
                    recruiterRoleId: recruiterRole?.id || config.bot.recruiterRoleId
                },
                recruitment: nextRecruitment
            });
            const sync = await ensureRecruitmentPanel(interaction.client);
            await interaction.editReply(formatSync(sync));
            return;
        }

        if (subcommand === "sync-panel") {
            if (!(await requireManager(interaction))) return;

            const sync = await ensureRecruitmentPanel(interaction.client);
            await interaction.editReply(formatSync(sync));
            return;
        }

        if (subcommand === "sync-banlist") {
            if (!(await requireManager(interaction))) return;

            const sync = await syncRecruitmentBanList(interaction.client);
            await interaction.editReply(sync.skipped
                ? `Skipped: ${sync.reason}`
                : `Recruitment ban list synced in <#${sync.channelId}> (${sync.count} users).`
            );
            return;
        }

        if (subcommand === "status") {
            const { recruitment, logging, memberCounts } = await loadDashboardConfig();
            const openTickets = await listTickets({ status: "open" });
            await interaction.editReply([
                `Enabled: **${recruitment.enabled ? "yes" : "no"}**`,
                `Panel Channel: ${recruitment.panelChannelId ? `<#${recruitment.panelChannelId}>` : "not set"}`,
                `Panel Message: ${recruitment.panelMessageId || "not set"}`,
                `Image Log Channel: ${recruitment.logChannelId ? `<#${recruitment.logChannelId}>` : "not set"}`,
                `Combined Log Channel: ${logging.channelId ? `<#${logging.channelId}>` : "not set"}`,
                `Private Threads: **${recruitment.privateThreads ? "yes" : "no"}**`,
                `Transcript On Close: **${recruitment.transcriptOnClose ? "yes" : "no"}**`,
                `Close Behavior: **lock + archive**`,
                `Recruiter Role: ${recruitment.recruiterRoleId ? `<@&${recruitment.recruiterRoleId}>` : "not set"}`,
                `Invite Channel: ${recruitment.inviteChannelId || "not set"}`,
                `Open Tickets: **${openTickets.length}**`,
                `Member Count Auto-Update: **${memberCounts.updateOnRecruitmentClose ? "yes" : "no"}**`
            ].join("\n"));
            return;
        }

        if (subcommand === "logs") {
            const logs = await listRecruitmentLogs(10);
            if (!logs.length) {
                await interaction.editReply("No recruitment logs yet.");
                return;
            }

            await interaction.editReply(logs.map(log => {
                const outcome = log.outcome === "accepted" ? log.team : "Rejected";
                return `**${outcome}** - <@${log.applicantId}> closed by <@${log.closedById}> on ${new Date(log.closedAt).toLocaleString()}`;
            }).join("\n"));
            return;
        }

        if (subcommand === "claim") {
            await claimTicket(interaction);
            return;
        }

        if (subcommand === "close") {
            const outcome = interaction.options.getString("outcome");
            if (outcome) {
                await finishClose(interaction, outcome);
            } else {
                await startClose(interaction);
            }
            return;
        }

        if (subcommand === "add") {
            await addUserToTicket(interaction, interaction.options.getUser("user", true));
            return;
        }

        if (subcommand === "massadd") {
            await massAddUsersToTicket(interaction, interaction.options.getString("users", true));
            return;
        }

        if (subcommand === "remove") {
            await removeUserFromTicket(interaction, interaction.options.getUser("user", true));
            return;
        }

        if (subcommand === "rename") {
            await renameTicket(interaction, interaction.options.getString("name", true));
            return;
        }

        if (subcommand === "tutorial") {
            await sendTutorial(interaction, interaction.options.getString("tutorial_id", true));
            return;
        }

        if (subcommand === "archive") {
            await archiveTicket(interaction);
            return;
        }

        if (subcommand === "delete") {
            await deleteTicket(interaction);
        }
    }
};
