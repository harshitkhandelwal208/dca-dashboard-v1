const { PermissionFlagsBits, SlashCommandBuilder } = require("discord.js");
const { loadDashboardConfig } = require("../../utils/dashboardConfig");
const { syncRecruitmentBanList } = require("../../utils/recruitmentBanPanel");
const { addRecruitmentBan } = require("../../utils/recruitmentBanStore");
const { getRecruitmentGuildId } = require("../../utils/serverConfig");

async function isRecruitmentBanServer(client, guildId, config) {
    const configuredRecruitmentGuildId = config.bot?.recruitmentGuildId || process.env.RECRUITMENT_GUILD_ID || "";
    if (configuredRecruitmentGuildId) return guildId === configuredRecruitmentGuildId;

    const banListChannelId = config.recruitment?.banListChannelId || "";
    if (!banListChannelId) return guildId === getRecruitmentGuildId(config);

    const channel = await client.channels.fetch(banListChannelId).catch(() => null);
    return channel?.guildId === guildId;
}

module.exports = {
    data: new SlashCommandBuilder()
        .setName("ban")
        .setDescription("Ban a user from the server")
        .addUserOption(option =>
            option
                .setName("user")
                .setDescription("User to ban")
                .setRequired(true)
        )
        .addStringOption(option =>
            option
                .setName("reason")
                .setDescription("Reason for the ban")
                .setRequired(true)
        )
        .setDefaultMemberPermissions(PermissionFlagsBits.BanMembers),

    async execute(interaction) {
        const targetUser = interaction.options.getUser("user", true);
        const targetMember = await interaction.guild.members.fetch(targetUser.id).catch(() => null);
        const reason = interaction.options.getString("reason") || "No reason provided";

        if (targetMember && !targetMember.bannable) {
            await interaction.reply({ content: "I cannot ban this user.", ephemeral: true });
            return;
        }

        const config = await loadDashboardConfig();
        const isRecruitmentServer = await isRecruitmentBanServer(interaction.client, interaction.guildId, config);

        await interaction.guild.bans.create(targetUser.id, { reason });

        if (isRecruitmentServer) {
            await addRecruitmentBan({
                userId: targetUser.id,
                userTag: targetUser.tag || targetUser.username,
                reason,
                bannedById: interaction.user.id,
                bannedByTag: interaction.user.tag || interaction.user.username,
                guildId: interaction.guildId
            });

            await syncRecruitmentBanList(interaction.client).catch(error => {
                console.error("Failed to sync recruitment ban list:", error.message);
            });
        }

        await interaction.reply({
            content: `**${targetUser.tag || targetUser.username}** has been banned. Reason: **${reason}**`,
            ephemeral: false
        });
    }
};
