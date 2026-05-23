const { EmbedBuilder } = require("discord.js");
const { loadDashboardConfig, saveDashboardConfig } = require("./dashboardConfig");
const { listRecruitmentBans } = require("./recruitmentBanStore");

function chunk(items, size) {
    const chunks = [];
    for (let index = 0; index < items.length; index += size) {
        chunks.push(items.slice(index, index + size));
    }
    return chunks;
}

function buildBanEmbeds(bans, guildName) {
    if (!bans.length) {
        return [
            new EmbedBuilder()
                .setTitle("Team Join Ban List")
                .setDescription("No users are currently blocked from joining teams.")
                .setColor(0x2f855a)
                .setTimestamp()
        ];
    }

    return chunk(bans, 25).map((group, index, groups) => {
        const embed = new EmbedBuilder()
            .setTitle(groups.length > 1 ? `Team Join Ban List (${index + 1}/${groups.length})` : "Team Join Ban List")
            .setDescription(`Recruitment bans for ${guildName || "the recruitment server"}.`)
            .setColor(0xb42318)
            .setTimestamp();

        for (const ban of group) {
            embed.addFields({
                name: ban.userTag || "Unknown user",
                value: [
                    `User: <@${ban.userId}>`,
                    `Discord ID: ${ban.userId}`,
                    `Reason: ${ban.reason || "No reason provided."}`,
                    ban.bannedById ? `Banned by: <@${ban.bannedById}> (${ban.bannedByTag || ban.bannedById})` : "",
                    ban.guildId ? `Server ID: ${ban.guildId}` : "",
                    ban.updatedAt ? `Listed: <t:${Math.floor(Date.parse(ban.updatedAt) / 1000)}:R>` : ""
                ].filter(Boolean).join("\n").slice(0, 1024),
                inline: false
            });
        }

        return embed;
    });
}

async function syncRecruitmentBanList(client) {
    const config = await loadDashboardConfig();
    const channelId = config.recruitment.banListChannelId;
    if (!channelId) return { skipped: true, reason: "Recruitment ban list channel is not configured." };

    const channel = await client.channels.fetch(channelId).catch(() => null);
    if (!channel?.isTextBased?.() || !channel.messages?.fetch) {
        return { skipped: true, reason: "Recruitment ban list channel is not text based." };
    }

    const configuredRecruitmentGuildId = config.bot?.recruitmentGuildId || process.env.RECRUITMENT_GUILD_ID || "";
    if (configuredRecruitmentGuildId && channel.guildId !== configuredRecruitmentGuildId) {
        return { skipped: true, reason: "Ban list channel is not in the configured recruitment server." };
    }

    const bans = await listRecruitmentBans();
    const embeds = buildBanEmbeds(bans, channel.guild?.name || "");
    const messageIds = [...(config.recruitment.banListMessageIds || [])];
    const nextMessageIds = [];

    for (const embed of embeds) {
        const existingId = messageIds.shift();
        let message = existingId ? await channel.messages.fetch(existingId).catch(() => null) : null;

        if (message?.author?.id !== client.user.id) message = null;
        if (message) {
            await message.edit({ embeds: [embed], allowedMentions: { parse: [] } });
        } else {
            message = await channel.send({ embeds: [embed], allowedMentions: { parse: [] } });
        }

        nextMessageIds.push(message.id);
    }

    for (const staleId of messageIds) {
        const stale = await channel.messages.fetch(staleId).catch(() => null);
        if (stale?.author?.id === client.user.id) await stale.delete().catch(() => null);
    }

    await saveDashboardConfig({
        ...config,
        recruitment: {
            ...config.recruitment,
            banListMessageIds: nextMessageIds
        }
    });

    return { count: bans.length, messages: nextMessageIds.length, channelId };
}

module.exports = {
    syncRecruitmentBanList
};
