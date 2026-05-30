const Parser = require("rss-parser");
const { loadDashboardConfig, saveDashboardConfig } = require("./dashboardConfig");
const { logAction } = require("./logStore");

const parser = new Parser();
let intervalHandle = null;
let checkInFlight = null;
const VIDEO_HISTORY_LIMIT = 50;
const MAX_BACKLOG_POSTS_PER_FEED = 5;
const DEFAULT_MAX_ANNOUNCEMENT_AGE_HOURS = 72;

function renderTemplate(template, values) {
    return String(template || "")
        .replaceAll("{name}", values.name || "")
        .replaceAll("{url}", values.url || "")
        .replaceAll("{channelId}", values.channelId || "")
        .replaceAll("{videoId}", values.videoId || "");
}

function uniqueVideoIds(ids) {
    const seen = new Set();
    return ids
        .map(id => String(id || "").trim())
        .filter(id => id && !seen.has(id) && seen.add(id))
        .slice(0, VIDEO_HISTORY_LIMIT);
}

function extractVideoId(item) {
    const rawId = String(item?.id || item?.guid || "").trim();
    if (rawId) {
        const candidate = rawId.split(":").pop();
        if (candidate) return candidate;
    }

    try {
        const url = new URL(item?.link || "");
        return url.searchParams.get("v") || "";
    } catch {
        return "";
    }
}

function normalizeVideo(item) {
    const videoId = extractVideoId(item);
    if (!videoId) return null;

    return {
        videoId,
        title: item.title || "New video",
        url: item.link || `https://www.youtube.com/watch?v=${videoId}`,
        publishedAt: item.isoDate || item.pubDate || ""
    };
}

async function fetchRecentVideos(feedId) {
    const feed = await parser.parseURL(`https://www.youtube.com/feeds/videos.xml?channel_id=${encodeURIComponent(feedId)}`);
    return (feed?.items || [])
        .map(normalizeVideo)
        .filter(Boolean);
}

function videoStateForFeed(feed) {
    return uniqueVideoIds([
        ...(Array.isArray(feed.sentVideoIds) ? feed.sentVideoIds : []),
        ...(Array.isArray(feed.postedVideoIds) ? feed.postedVideoIds : []),
        ...(Array.isArray(feed.seenVideoIds) ? feed.seenVideoIds : []),
        feed.lastVideoId
    ]);
}

function timestampFor(value) {
    const timestamp = Date.parse(value || "");
    return Number.isFinite(timestamp) ? timestamp : 0;
}

function videoTimestamp(video) {
    return timestampFor(video?.publishedAt);
}

function maxAnnouncementAgeMs(youtube) {
    const hours = Number(youtube?.maxAnnouncementAgeHours || DEFAULT_MAX_ANNOUNCEMENT_AGE_HOURS);
    const bounded = Math.min(720, Math.max(1, Number.isFinite(hours) ? hours : DEFAULT_MAX_ANNOUNCEMENT_AGE_HOURS));
    return bounded * 60 * 60 * 1000;
}

function checkpointTimestamp(feed, recentVideos) {
    const storedTimestamp = timestampFor(feed.lastPublishedAt);
    if (storedTimestamp) return storedTimestamp;

    const seen = new Set(videoStateForFeed(feed));
    const seenTimestamps = recentVideos
        .filter(video => seen.has(video.videoId))
        .map(videoTimestamp)
        .filter(Boolean);

    return seenTimestamps.length ? Math.max(...seenTimestamps) : 0;
}

function videosToAnnounce(feed, recentVideos, youtube) {
    if (!recentVideos.length) return [];

    const seenIds = videoStateForFeed(feed);
    if (!seenIds.length) return [];

    const seen = new Set(seenIds);
    const checkpoint = checkpointTimestamp(feed, recentVideos);
    if (!checkpoint) return [];

    const minPublishedAt = Date.now() - maxAnnouncementAgeMs(youtube);
    const candidates = recentVideos.filter(video => {
        if (seen.has(video.videoId)) return false;

        const publishedAt = videoTimestamp(video);
        return publishedAt > checkpoint && publishedAt >= minPublishedAt;
    });

    return candidates
        .slice(0, MAX_BACKLOG_POSTS_PER_FEED)
        .reverse();
}

function feedStatePatch(feed, recentVideos, announcedVideos = []) {
    const latest = recentVideos[0] || null;
    const ids = uniqueVideoIds([
        ...(latest ? recentVideos.map(video => video.videoId) : []),
        ...announcedVideos.map(video => video.videoId),
        ...videoStateForFeed(feed)
    ]);

    return {
        lastVideoId: latest?.videoId || feed.lastVideoId || "",
        lastPublishedAt: latest?.publishedAt || feed.lastPublishedAt || "",
        lastCheckedAt: new Date().toISOString(),
        sentVideoIds: ids
    };
}

async function persistYoutubeFeedState(config, feedStatePatches) {
    if (!feedStatePatches.size) return;

    const latestConfig = await loadDashboardConfig();
    await saveDashboardConfig({
        ...latestConfig,
        youtube: {
            ...latestConfig.youtube,
            feeds: latestConfig.youtube.feeds.map(feed => {
                const patch = feedStatePatches.get(feed.id);
                return patch ? { ...feed, ...patch } : feed;
            })
        }
    });
}

async function runYouTubeFeedCheck(client) {
    const config = await loadDashboardConfig();
    const youtube = config.youtube;
    if (!youtube.enabled) return { skipped: true, reason: "YouTube notifications are disabled." };

    const results = [];
    const feedStatePatches = new Map();

    for (const feed of youtube.feeds) {
        if (!feed.enabled) {
            results.push({ id: feed.id, name: feed.name, skipped: true, reason: "disabled" });
            continue;
        }

        try {
            const recentVideos = await fetchRecentVideos(feed.id);
            if (!recentVideos.length) {
                results.push({ id: feed.id, name: feed.name, skipped: true, reason: "no videos found" });
                continue;
            }

            if (!videoStateForFeed(feed).length) {
                feedStatePatches.set(feed.id, feedStatePatch(feed, recentVideos));
                results.push({ id: feed.id, name: feed.name, initialized: true, videoId: recentVideos[0].videoId });
                continue;
            }

            const announcements = videosToAnnounce(feed, recentVideos, youtube);
            if (!announcements.length) {
                feedStatePatches.set(feed.id, feedStatePatch(feed, recentVideos));
                results.push({ id: feed.id, name: feed.name, unchanged: true, videoId: recentVideos[0].videoId });
                continue;
            }

            const channelId = feed.channelId || youtube.defaultChannelId;
            const channel = channelId ? await client.channels.fetch(channelId).catch(() => null) : null;
            let postedCount = 0;
            let sendError = "";

            for (const video of announcements) {
                if (channel?.isTextBased?.()) {
                    try {
                        await channel.send(renderTemplate(youtube.announcementTemplate, {
                            name: feed.name,
                            channelId: feed.id,
                            videoId: video.videoId,
                            url: video.url
                        }));
                        postedCount += 1;

                        await logAction(client, {
                            type: "youtube",
                            title: "YouTube Video Posted",
                            message: `${feed.name} posted ${video.url}`,
                            guildId: config.bot.guildId,
                            metadata: { feedId: feed.id, videoId: video.videoId, channelId }
                        });
                    } catch (error) {
                        sendError = error.message;
                        console.error(`Failed to announce YouTube video ${video.videoId}:`, error.message);
                    }
                }
            }

            feedStatePatches.set(feed.id, feedStatePatch(feed, recentVideos, announcements));
            results.push({
                id: feed.id,
                name: feed.name,
                posted: postedCount > 0,
                postedCount,
                skipped: postedCount === 0,
                reason: sendError || (channel?.isTextBased?.() ? "" : "announcement channel is not configured or is not text based"),
                videoId: announcements.at(-1)?.videoId || recentVideos[0].videoId
            });
        } catch (error) {
            results.push({ id: feed.id, name: feed.name, error: error.message });
        }
    }

    await persistYoutubeFeedState(config, feedStatePatches);

    return { results };
}

async function checkYouTubeFeeds(client) {
    if (checkInFlight) {
        return { skipped: true, reason: "A YouTube feed check is already running." };
    }

    checkInFlight = runYouTubeFeedCheck(client);
    try {
        return await checkInFlight;
    } finally {
        checkInFlight = null;
    }
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
