function getCommunityGuildId(config) {
    return config?.bot?.communityGuildId ||
        config?.bot?.guildId ||
        process.env.COMMUNITY_GUILD_ID ||
        process.env.DISCORD_GUILD_ID ||
        "";
}

function getRecruitmentGuildId(config) {
    return config?.bot?.recruitmentGuildId ||
        process.env.RECRUITMENT_GUILD_ID ||
        config?.bot?.guildId ||
        process.env.DISCORD_GUILD_ID ||
        "";
}

function getDashboardUrl(config) {
    const base = config?.bot?.dashboardUrl || process.env.DASHBOARD_BASE_URL || "";
    return base ? `${String(base).replace(/\/$/, "")}/dashboard` : "";
}

module.exports = {
    getCommunityGuildId,
    getDashboardUrl,
    getRecruitmentGuildId
};
