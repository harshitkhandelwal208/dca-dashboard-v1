const { PermissionsBitField } = require("discord.js");
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
    name: "ban",
    description: "Bans a user by mention or ID.",

    async execute(message, args) {
        if (!message.member.permissions.has(PermissionsBitField.Flags.BanMembers)) {
            await message.reply("You don't have permission to use this command.");
            return;
        }

        const target = message.mentions.members.first();
        const targetId = target ? target.id : args[0];
        const reason = args.slice(1).join(" ");

        if (!targetId) {
            await message.reply("Please mention a user or provide an ID to ban.");
            return;
        }

        if (!reason) {
            await message.reply("Please provide a reason for the ban.");
            return;
        }

        const user = await message.client.users.fetch(targetId).catch(() => null);

        try {
            await user?.send(`You have been banned from **${message.guild.name}**.\nReason: **${reason}**`);
        } catch {
            await message.channel.send("Couldn't send a DM to the user. Proceeding with the ban.");
        }

        try {
            const config = await loadDashboardConfig();
            const isRecruitmentServer = await isRecruitmentBanServer(message.client, message.guild.id, config);

            await message.guild.bans.create(targetId, { reason });

            if (isRecruitmentServer) {
                await addRecruitmentBan({
                    userId: targetId,
                    userTag: user?.tag || user?.username || "",
                    reason,
                    bannedById: message.author.id,
                    bannedByTag: message.author.tag || message.author.username,
                    guildId: message.guild.id
                });

                await syncRecruitmentBanList(message.client).catch(error => {
                    console.error("Failed to sync recruitment ban list:", error.message);
                });
            }

            await message.channel.send(`**<@${targetId}> has been banned.**\nReason: **${reason}**`);
        } catch (error) {
            console.error(error);
            await message.reply("Failed to ban the user. They may not exist or already be banned.");
        }
    }
};
