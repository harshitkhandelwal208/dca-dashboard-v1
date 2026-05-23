const { Events } = require('discord.js');
const { loadDashboardConfig } = require("../utils/dashboardConfig");
const { logAction } = require("../utils/logStore");
const { renderMemberTemplate } = require("../utils/messageTemplates");
const { getCommunityGuildId } = require("../utils/serverConfig");

module.exports = {
    name: Events.GuildMemberRemove,
    async execute(member) {
        const config = await loadDashboardConfig();
        const { leave } = config;
        const communityGuildId = getCommunityGuildId(config);
        if (communityGuildId && member.guild.id !== communityGuildId) return;
        if (!leave.enabled || !leave.channelId) return;

        try {
            const channel = await member.guild.channels.fetch(leave.channelId);
            if (!channel?.isTextBased?.()) return;

            await channel.send(renderMemberTemplate(leave.message, member));

            await logAction(member.client, {
                type: "system",
                title: "Member Left",
                message: `**${member.user?.tag || member.id}** left the server.`,
                guildId: member.guild.id,
                targetId: member.id,
                targetTag: member.user?.tag || member.user?.username || member.id,
                metadata: { memberCount: member.guild.memberCount }
            });
        } catch (error) {
            console.error("Error sending leave message:", error);
        }
    },
};
