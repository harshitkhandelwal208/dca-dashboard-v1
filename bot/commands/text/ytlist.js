const { loadDashboardConfig } = require("../../utils/dashboardConfig");

module.exports = {
    name: "yt",
    execute: async (message, args) => {
        if (args[0] !== "list") return;

        const { youtube } = await loadDashboardConfig();
        const channelList = youtube.feeds
            .filter(feed => feed.enabled)
            .map(feed => `**${feed.name}** - [YouTube](https://www.youtube.com/channel/${feed.id})`)
            .join("\n");

        await message.reply(channelList || "No YouTube channels are being tracked.");
    }
};
