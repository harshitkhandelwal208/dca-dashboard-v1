const { loadDashboardConfig, saveDashboardConfig } = require("./dashboardConfig");
const { appendLog } = require("./logStore");
const { listRecruitmentBans } = require("./recruitmentBanStore");

const DISCORD_API = "https://discord.com/api/v10";
const CHANNEL_TYPES = {
    GUILD_TEXT: 0,
    GUILD_CATEGORY: 4,
    GUILD_ANNOUNCEMENT: 5
};

function getGuildId() {
    return process.env.DISCORD_GUILD_ID || "";
}

async function getConfiguredGuildId() {
    const config = await loadDashboardConfig().catch(() => null);
    return process.env.COMMUNITY_GUILD_ID || process.env.DISCORD_GUILD_ID || config?.bot?.communityGuildId || config?.bot?.guildId || "";
}

function getRecruitmentGuildId(config) {
    return config?.bot?.recruitmentGuildId || process.env.RECRUITMENT_GUILD_ID || config?.bot?.guildId || process.env.DISCORD_GUILD_ID || "";
}

function getBotToken() {
    return process.env.DISCORD_TOKEN || "";
}

function colorToNumber(color) {
    return Number.parseInt(String(color || "#0f766e").replace("#", ""), 16);
}

async function discordBotRequest(path, options = {}) {
    const token = getBotToken();
    if (!token) throw new Error("DISCORD_TOKEN is required for dashboard Discord API calls.");
    const isFormData = typeof FormData !== "undefined" && options.body instanceof FormData;

    const headers = {
        Authorization: `Bot ${token}`,
        ...(options.body && !isFormData ? { "Content-Type": "application/json" } : {}),
        ...(options.headers || {})
    };

    for (const [key, value] of Object.entries(headers)) {
        if (value === undefined || value === null) delete headers[key];
    }

    const response = await fetch(`${DISCORD_API}${path}`, {
        ...options,
        headers
    });

    const text = await response.text();
    let data = {};
    if (text) {
        try {
            data = JSON.parse(text);
        } catch {
            data = { message: text };
        }
    }

    if (!response.ok) {
        const error = new Error(data.message || data.error || `Discord API returned ${response.status}`);
        error.status = response.status;
        error.body = data;
        throw error;
    }

    return data;
}

async function getBotUser() {
    return discordBotRequest("/users/@me");
}

async function getGuildMember(userId) {
    const guildId = await getConfiguredGuildId();
    if (!guildId) throw new Error("DISCORD_GUILD_ID is required.");
    return discordBotRequest(`/guilds/${guildId}/members/${userId}`);
}

async function getSingleGuildLookups(guildId) {
    if (!guildId) {
        return { ready: false, guild: null, channels: [], roles: [] };
    }

    const [guild, channels, roles] = await Promise.all([
        discordBotRequest(`/guilds/${guildId}`),
        discordBotRequest(`/guilds/${guildId}/channels`),
        discordBotRequest(`/guilds/${guildId}/roles`)
    ]);

    const categories = new Map(
        channels
            .filter(channel => channel.type === CHANNEL_TYPES.GUILD_CATEGORY)
            .map(channel => [channel.id, channel.name])
    );

    return {
        ready: true,
        guild: {
            id: guild.id,
            name: guild.name,
            icon: guild.icon
                ? `https://cdn.discordapp.com/icons/${guild.id}/${guild.icon}.png?size=80`
                : ""
        },
        channels: channels
            .filter(channel => channel.type === CHANNEL_TYPES.GUILD_TEXT || channel.type === CHANNEL_TYPES.GUILD_ANNOUNCEMENT)
            .sort((a, b) => (a.position || 0) - (b.position || 0) || a.name.localeCompare(b.name))
            .map(channel => ({
                id: channel.id,
                name: channel.parent_id && categories.has(channel.parent_id)
                    ? `${categories.get(channel.parent_id)} / #${channel.name}`
                    : `#${channel.name}`,
                type: channel.type
            })),
        roles: roles
            .filter(role => role.id !== guildId && !role.managed)
            .sort((a, b) => (b.position || 0) - (a.position || 0) || a.name.localeCompare(b.name))
            .map(role => ({
                id: role.id,
                name: role.name,
                color: role.color ? `#${role.color.toString(16).padStart(6, "0")}` : "",
                position: role.position || 0
            }))
    };
}

async function getGuildLookups() {
    const config = await loadDashboardConfig().catch(() => null);
    const communityGuildId = config?.bot?.communityGuildId || config?.bot?.guildId || process.env.COMMUNITY_GUILD_ID || process.env.DISCORD_GUILD_ID || "";
    const recruitmentGuildId = getRecruitmentGuildId(config);
    const community = await getSingleGuildLookups(communityGuildId).catch(error => ({ ready: false, guild: null, channels: [], roles: [], error: error.message }));
    const recruitment = recruitmentGuildId && recruitmentGuildId !== communityGuildId
        ? await getSingleGuildLookups(recruitmentGuildId).catch(error => ({ ready: false, guild: null, channels: [], roles: [], error: error.message }))
        : community;

    return {
        ...community,
        community,
        recruitment
    };
}

function buildMemberCountContent(memberCounts) {
    const lines = [`# ${memberCounts.title || "Member Count"}`];

    memberCounts.teams.forEach((team, index) => {
        lines.push("");
        lines.push(`## Team ${index + 1} - ${team.name}`);
        if (team.division) lines.push(`(${team.division})`);
        lines.push(`Number of Players - ${team.players}`);
        lines.push(`Recruitment Status - ${team.recruitmentStatus}`);
    });

    return lines.join("\n");
}

function buildMemberCountEmbed(memberCounts) {
    const teams = memberCounts.teams.slice(0, 25);
    const totalPlayers = teams.reduce((sum, team) => sum + Number(team.players || 0), 0);

    return {
        title: memberCounts.title || "Member Count",
        description: [
            `**Tracked teams:** ${teams.length}`,
            `**Total players:** ${totalPlayers}`
        ].join("\n"),
        color: 0x0f766e,
        timestamp: new Date().toISOString(),
        footer: { text: "Use /teamcount or the dashboard to update these numbers." },
        fields: teams.map(team => ({
            name: `${team.name}${team.division ? ` - ${team.division}` : ""}`,
            value: [
                `**Players:** ${team.players}`,
                `**Recruitment:** ${team.recruitmentStatus}`,
                team.aliases?.length ? `**Aliases:** ${team.aliases.join(", ")}` : ""
            ].filter(Boolean).join("\n"),
            inline: true
        }))
    };
}

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
            {
                title: "Team Join Ban List",
                description: "No users are currently blocked from joining teams.",
                color: 0x2f855a,
                timestamp: new Date().toISOString()
            }
        ];
    }

    return chunk(bans, 25).map((group, index, groups) => ({
        title: groups.length > 1 ? `Team Join Ban List (${index + 1}/${groups.length})` : "Team Join Ban List",
        description: `Recruitment bans for ${guildName || "the recruitment server"}.`,
        color: 0xb42318,
        timestamp: new Date().toISOString(),
        fields: group.map(ban => ({
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
        }))
    }));
}

function recruitmentPanelPayload(config) {
    return {
        embeds: [
            {
                title: config.recruitment.panelTitle,
                description: config.recruitment.panelDescription,
                color: colorToNumber(config.recruitment.panelColor),
                timestamp: new Date().toISOString()
            }
        ],
        components: [
            {
                type: 1,
                components: [
                    {
                        type: 2,
                        custom_id: "recruitment:apply",
                        label: "Apply!",
                        style: 1
                    }
                ]
            }
        ]
    };
}

async function fetchMessage(channelId, messageId) {
    if (!channelId || !messageId) return null;

    try {
        return await discordBotRequest(`/channels/${channelId}/messages/${messageId}`);
    } catch {
        return null;
    }
}

async function ensureRecruitmentPanel() {
    const config = await loadDashboardConfig();
    const recruitment = config.recruitment;

    if (!recruitment.enabled) return { skipped: true, reason: "Recruitment is disabled." };
    if (!recruitment.panelChannelId) return { skipped: true, reason: "Recruitment panel channel is not configured." };

    const payload = recruitmentPanelPayload(config);
    let message = await fetchMessage(recruitment.panelChannelId, recruitment.panelMessageId);
    const bot = await getBotUser();
    if (message?.author?.id !== bot.id) message = null;

    const created = !message;
    if (message) {
        message = await discordBotRequest(`/channels/${recruitment.panelChannelId}/messages/${message.id}`, {
            method: "PATCH",
            body: JSON.stringify(payload)
        });
    } else {
        message = await discordBotRequest(`/channels/${recruitment.panelChannelId}/messages`, {
            method: "POST",
            body: JSON.stringify(payload)
        });
    }

    if (message.id !== recruitment.panelMessageId) {
        await saveDashboardConfig({
            ...config,
            recruitment: {
                ...recruitment,
                panelMessageId: message.id
            }
        });
    }

    await pruneRecruitmentPanelChannel(recruitment.panelChannelId, message.id).catch(error => {
        console.error("Failed to clean recruitment panel channel:", error.message);
    });

    return {
        created,
        channelId: recruitment.panelChannelId,
        messageId: message.id
    };
}

async function pruneRecruitmentPanelChannel(channelId, panelMessageId) {
    let deleted = 0;
    let scanned = 0;
    let before = "";

    while (scanned < 500) {
        const query = new URLSearchParams({ limit: "100" });
        if (before) query.set("before", before);
        const messages = await discordBotRequest(`/channels/${channelId}/messages?${query.toString()}`);
        if (!Array.isArray(messages) || !messages.length) break;

        scanned += messages.length;
        before = messages[messages.length - 1]?.id || "";

        for (const message of messages) {
            if (message.id === panelMessageId) continue;
            await discordBotRequest(`/channels/${channelId}/messages/${message.id}`, {
                method: "DELETE",
                headers: { "Content-Type": undefined }
            }).then(() => {
                deleted += 1;
            }).catch(() => null);
        }

        if (messages.length < 100) break;
    }

    return { deleted, channelId };
}

function normalizeForSearch(value) {
    return String(value || "").replace(/\s+/g, " ").trim();
}

async function findExistingBotMessage(channelId, botId, content) {
    const messages = await discordBotRequest(`/channels/${channelId}/messages?limit=25`);
    const expected = normalizeForSearch(content);
    const firstLine = normalizeForSearch(String(content || "").split("\n")[0]);

    return messages.find(message => (
        message.author?.id === botId &&
        normalizeForSearch(message.content) === expected
    )) || messages.find(message => (
        message.author?.id === botId &&
        firstLine.length >= 16 &&
        normalizeForSearch(message.content).includes(firstLine)
    )) || null;
}

function reactionEmojiPath(emoji) {
    const custom = String(emoji || "").match(/^<a?:([^:]+):(\d+)>$/) || String(emoji || "").match(/^([^:]+):(\d+)$/);
    if (custom) return encodeURIComponent(`${custom[1]}:${custom[2]}`);
    return encodeURIComponent(String(emoji || ""));
}

async function syncReactionRoles() {
    const config = await loadDashboardConfig();
    const bot = await getBotUser();
    const results = [];
    let changed = false;
    const reactionRoles = [];

    for (const group of config.reactionRoles) {
        if (!group.enabled) {
            results.push({ id: group.id, name: group.name, skipped: true, reason: "disabled" });
            reactionRoles.push(group);
            continue;
        }

        if (!group.channelId || !group.options.length) {
            results.push({ id: group.id, name: group.name, skipped: true, reason: "missing channel or options" });
            reactionRoles.push(group);
            continue;
        }

        try {
            let message = await fetchMessage(group.channelId, group.messageId);
            if (message?.author?.id !== bot.id) message = null;
            if (!message) message = await findExistingBotMessage(group.channelId, bot.id, group.message);

            const created = !message;
            if (message) {
                message = await discordBotRequest(`/channels/${group.channelId}/messages/${message.id}`, {
                    method: "PATCH",
                    body: JSON.stringify({ content: group.message })
                });
            } else {
                message = await discordBotRequest(`/channels/${group.channelId}/messages`, {
                    method: "POST",
                    body: JSON.stringify({ content: group.message })
                });
            }

            const reactionErrors = [];
            for (const option of group.options) {
                try {
                    await discordBotRequest(
                        `/channels/${group.channelId}/messages/${message.id}/reactions/${reactionEmojiPath(option.emoji)}/@me`,
                        { method: "PUT", headers: { "Content-Type": undefined } }
                    );
                } catch (error) {
                    reactionErrors.push(`${option.emoji}: ${error.message}`);
                }
            }

            if (message.id !== group.messageId) {
                reactionRoles.push({ ...group, messageId: message.id });
                changed = true;
            } else {
                reactionRoles.push(group);
            }

            results.push({
                id: group.id,
                name: group.name,
                messageId: message.id,
                channelId: group.channelId,
                created,
                reactionErrors
            });
        } catch (error) {
            reactionRoles.push(group);
            results.push({ id: group.id, name: group.name, skipped: true, reason: error.message });
        }
    }

    const nextConfig = changed ? await saveDashboardConfig({ ...config, reactionRoles }) : config;
    await appendLog({
        type: "reactionRole",
        title: "Reaction Roles Synced From Dashboard",
        message: `${results.filter(result => !result.skipped).length} reaction role message(s) were synced.`,
        guildId: nextConfig.bot.guildId,
        metadata: { results }
    });
    return { config: nextConfig, results };
}

async function syncMemberCountMessage() {
    const config = await loadDashboardConfig();
    const memberCounts = config.memberCounts;

    if (!memberCounts.enabled) return { skipped: true, reason: "Member counts are disabled." };
    if (!memberCounts.channelId) return { skipped: true, reason: "Member count channel is not configured." };

    const bot = await getBotUser();
    let message = await fetchMessage(memberCounts.channelId, memberCounts.messageId);
    if (message?.author?.id !== bot.id) message = null;

    const payload = {
        content: "",
        embeds: [buildMemberCountEmbed(memberCounts)]
    };

    const created = !message;
    if (message) {
        message = await discordBotRequest(`/channels/${memberCounts.channelId}/messages/${message.id}`, {
            method: "PATCH",
            body: JSON.stringify(payload)
        });
    } else {
        message = await discordBotRequest(`/channels/${memberCounts.channelId}/messages`, {
            method: "POST",
            body: JSON.stringify(payload)
        });
    }

    let nextConfig = config;
    if (message.id !== memberCounts.messageId) {
        nextConfig = await saveDashboardConfig({
            ...config,
            memberCounts: {
                ...memberCounts,
                messageId: message.id
            }
        });
    }

    await appendLog({
        type: "memberCount",
        title: created ? "Member Count Message Created From Dashboard" : "Member Count Message Synced From Dashboard",
        message: `${memberCounts.teams.length} teams are listed in <#${memberCounts.channelId}>.`,
        guildId: nextConfig.bot.guildId,
        metadata: { channelId: memberCounts.channelId, messageId: message.id }
    });

    return { config: nextConfig, created, channelId: memberCounts.channelId, messageId: message.id };
}

async function syncRecruitmentBanList() {
    const config = await loadDashboardConfig();
    const channelId = config.recruitment.banListChannelId;
    if (!channelId) return { skipped: true, reason: "Recruitment ban list channel is not configured." };

    const channel = await discordBotRequest(`/channels/${channelId}`);
    const recruitmentGuildId = getRecruitmentGuildId(config);
    const configuredRecruitmentGuildId = config.bot?.recruitmentGuildId || process.env.RECRUITMENT_GUILD_ID || "";
    if (configuredRecruitmentGuildId && channel.guild_id !== configuredRecruitmentGuildId) {
        return { skipped: true, reason: "Ban list channel is not in the configured recruitment server." };
    }

    const guild = channel.guild_id ? await discordBotRequest(`/guilds/${channel.guild_id}`).catch(() => null) : null;
    const bans = await listRecruitmentBans();
    const embeds = buildBanEmbeds(bans, guild?.name || "");
    const messageIds = [...(config.recruitment.banListMessageIds || [])];
    const nextMessageIds = [];
    const bot = await getBotUser();

    for (const embed of embeds) {
        const existingId = messageIds.shift();
        let message = existingId ? await fetchMessage(channelId, existingId) : null;
        if (message?.author?.id !== bot.id) message = null;

        const payload = { embeds: [embed], allowed_mentions: { parse: [] } };
        if (message) {
            message = await discordBotRequest(`/channels/${channelId}/messages/${message.id}`, {
                method: "PATCH",
                body: JSON.stringify(payload)
            });
        } else {
            message = await discordBotRequest(`/channels/${channelId}/messages`, {
                method: "POST",
                body: JSON.stringify(payload)
            });
        }

        nextMessageIds.push(message.id);
    }

    for (const staleId of messageIds) {
        const stale = await fetchMessage(channelId, staleId);
        if (stale?.author?.id === bot.id) {
            await discordBotRequest(`/channels/${channelId}/messages/${staleId}`, {
                method: "DELETE",
                headers: { "Content-Type": undefined }
            }).catch(() => null);
        }
    }

    const saved = await saveDashboardConfig({
        ...config,
        recruitment: {
            ...config.recruitment,
            banListMessageIds: nextMessageIds
        }
    });

    await appendLog({
        type: "system",
        title: "Recruitment Ban List Synced From Dashboard",
        message: `${bans.length} banned user(s) are listed in <#${channelId}>.`,
        guildId: channel.guild_id || recruitmentGuildId,
        metadata: { channelId, messageIds: nextMessageIds }
    });

    return { config: saved, count: bans.length, messages: nextMessageIds.length, channelId };
}

module.exports = {
    discordBotRequest,
    ensureRecruitmentPanel,
    getBotUser,
    getGuildLookups,
    getGuildMember,
    syncMemberCountMessage,
    syncRecruitmentBanList,
    syncReactionRoles
};
