const { loadDashboardConfig } = require("../utils/dashboardConfig");
const { logAction } = require("../utils/logStore");
const { renderMemberTemplate } = require("../utils/messageTemplates");
const { getCommunityGuildId } = require("../utils/serverConfig");
const { handleCommunityMemberJoin } = require("../utils/teamRoleScheduler");
const { teamButtonsForMember } = require("../utils/welcomeRoleManager");

module.exports = {
    name: "guildMemberAdd",
    async execute(member) {
        const config = await loadDashboardConfig();
        await handleCommunityMemberJoin(member).catch(error => {
            console.error("Error processing pending team roles:", error.message);
        });

        const { welcome } = config;
        const communityGuildId = getCommunityGuildId(config);
        if (communityGuildId && member.guild.id !== communityGuildId) return;
        if (!welcome.enabled || !welcome.channelId) return;

        try {
            const channel = await member.guild.channels.fetch(welcome.channelId);
            if (!channel?.isTextBased?.()) return;

            await channel.send({
                content: renderMemberTemplate(welcome.message, member),
                components: teamButtonsForMember(member.id, config.memberCounts.teams),
                allowedMentions: { users: [member.id], roles: [] }
            });

            await logAction(member.client, {
                type: "system",
                title: "Member Joined",
                message: `<@${member.id}> joined the server.`,
                guildId: member.guild.id,
                targetId: member.id,
                targetTag: member.user?.tag || member.user?.username || member.id,
                metadata: { memberCount: member.guild.memberCount }
            });
        } catch (error) {
            console.error("Error sending welcome message:", error);
        }
    }
};
