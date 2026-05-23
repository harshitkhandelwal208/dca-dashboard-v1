const Parser = require("rss-parser");
const { loadDashboardConfig, saveDashboardConfig } = require("./dashboardConfig");
const { logAction } = require("./logStore");

const parser = new Parser();
let intervalHandle = null;

function renderTemplate(template, values) {
    return String(template || "")
        .replaceAll("{name}", values.name || "")
        .replaceAll("{url}", values.url || "")
        .replaceAll("{channelId}", values.channelId || "")
        .replaceAll("{videoId}", values.videoId || "");
}

async function fetchLatestVideo(feedId) {
    const feed = await parser.parseURL(`https://www.youtube.com/feeds/videos.xml?channel_id=${encodeURIComponent(feedId)}`);
    const latest = feed?.items?.[0];
    if (!latest?.id) return null;

    const videoId = String(latest.id).split(":").pop();
    return {
        videoId,
        title: latest.title || "New video",
        url: `https://www.youtube.com/watch?v=${videoId}`
    };
}

async function checkYouTubeFeeds(client) {
    const config = await loadDashboardConfig();
    const youtube = config.youtube;
    if (!youtube.enabled) return { skipped: true, reason: "YouTube notifications are disabled." };

    const feeds = [];
    const results = [];
    let changed = false;

    for (const feed of youtube.feeds) {
        if (!feed.enabled) {
            feeds.push(feed);
            results.push({ id: feed.id, name: feed.name, skipped: true, reason: "disabled" });
            continue;
        }

        try {
            const latest = await fetchLatestVideo(feed.id);
            if (!latest) {
                feeds.push(feed);
                results.push({ id: feed.id, name: feed.name, skipped: true, reason: "no videos found" });
                continue;
            }

            if (!feed.lastVideoId) {
                feeds.push({ ...feed, lastVideoId: latest.videoId });
                changed = true;
                results.push({ id: feed.id, name: feed.name, initialized: true, videoId: latest.videoId });
                continue;
            }

            if (feed.lastVideoId === latest.videoId) {
                feeds.push(feed);
                results.push({ id: feed.id, name: feed.name, unchanged: true, videoId: latest.videoId });
                continue;
            }

            const channelId = feed.channelId || youtube.defaultChannelId;
            const channel = channelId ? await client.channels.fetch(channelId).catch(() => null) : null;
            if (channel?.isTextBased?.()) {
                await channel.send(renderTemplate(youtube.announcementTemplate, {
                    name: feed.name,
                    channelId: feed.id,
                    videoId: latest.videoId,
                    url: latest.url
                }));
            }

            await logAction(client, {
                type: "youtube",
                title: "YouTube Video Posted",
                message: `${feed.name} posted ${latest.url}`,
                guildId: config.bot.guildId,
                metadata: { feedId: feed.id, videoId: latest.videoId, channelId }
            });

            feeds.push({ ...feed, lastVideoId: latest.videoId });
            changed = true;
            results.push({ id: feed.id, name: feed.name, posted: Boolean(channel), videoId: latest.videoId });
        } catch (error) {
            feeds.push(feed);
            results.push({ id: feed.id, name: feed.name, error: error.message });
        }
    }

    if (changed) {
        await saveDashboardConfig({
            ...config,
            youtube: {
                ...youtube,
                feeds
            }
        });
    }

    return { results };
}

function startYouTubeNotifier(client) {
    if (intervalHandle) clearInterval(intervalHandle);

    const run = async () => {
        try {
            const config = await loadDashboardConfig();
            if (!config.youtube.enabled) return;
            await checkYouTubeFeeds(client);
        } catch (error) {
            console.error("YouTube notifier error:", error.message);
        }
    };

    loadDashboardConfig()
        .then(config => {
            const intervalMs = Math.max(1, Number(config.youtube.checkIntervalMinutes || 5)) * 60 * 1000;
            setTimeout(run, 10 * 1000);
            intervalHandle = setInterval(run, intervalMs);
            console.log(`YouTube notifier scheduled every ${Math.round(intervalMs / 60000)} minute(s).`);
        })
        .catch(error => console.error("Failed to start YouTube notifier:", error.message));
}

module.exports = {
    checkYouTubeFeeds,
    startYouTubeNotifier
};
