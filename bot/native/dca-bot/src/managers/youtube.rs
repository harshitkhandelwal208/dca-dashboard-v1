//! YouTube upload notifier (`youtubeManager.js`).

use crate::app::App;
use crate::logs::{log_action, LogEntry};
use crate::util::*;
use dca_state::config::{update_config, YoutubeConfig, YoutubeFeed};
use serde_json::json;
use serenity::all::*;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

const VIDEO_HISTORY_LIMIT: usize = 50;
const MAX_BACKLOG_POSTS_PER_FEED: usize = 5;

static IN_FLIGHT: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Debug)]
pub struct Video {
    pub video_id: String,
    pub title: String,
    pub url: String,
    pub published_at: String,
}

#[derive(Default, Clone, Debug)]
pub struct FeedResult {
    pub id: String,
    pub name: String,
    pub skipped: bool,
    pub reason: String,
    pub initialized: bool,
    pub unchanged: bool,
    pub posted: bool,
    pub posted_count: usize,
    pub video_id: String,
    pub error: String,
}

pub enum CheckOutcome {
    Skipped(String),
    Results(Vec<FeedResult>),
}

fn render_template(template: &str, name: &str, channel: &str, video_id: &str, url: &str) -> String {
    template.replace("{name}", name).replace("{url}", url).replace("{channelId}", channel).replace("{videoId}", video_id)
}

fn unique_ids(ids: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut seen = HashSet::new();
    ids.into_iter().map(|i| i.trim().to_string()).filter(|i| !i.is_empty() && seen.insert(i.clone())).take(VIDEO_HISTORY_LIMIT).collect()
}

fn timestamp_of(value: &str) -> i64 {
    chrono::DateTime::parse_from_rfc3339(value.trim()).map(|d| d.timestamp_millis()).unwrap_or(0)
}

async fn fetch_recent_videos(app: &App, channel_id: &str) -> Result<Vec<Video>, String> {
    // (`DCA_YOUTUBE_FEED_BASE` lets tests serve a feed locally.)
    let base = std::env::var("DCA_YOUTUBE_FEED_BASE").unwrap_or_else(|_| "https://www.youtube.com".into());
    let url = format!("{}/feeds/videos.xml?channel_id={channel_id}", base.trim_end_matches('/'));
    let body = app.web.get(&url).send().await.map_err(|e| e.to_string())?;
    if !body.status().is_success() {
        return Err(format!("Status code {}", body.status().as_u16()));
    }
    let text = body.text().await.map_err(|e| e.to_string())?;
    let doc = roxmltree::Document::parse(&text).map_err(|e| e.to_string())?;
    let mut videos = Vec::new();
    for entry in doc.descendants().filter(|n| n.has_tag_name("entry")) {
        let child_text = |tag: &str| entry.children().find(|c| c.has_tag_name(tag)).and_then(|c| c.text()).unwrap_or("").trim().to_string();
        let raw_id = child_text("id");
        let mut video_id = raw_id.rsplit(':').next().unwrap_or("").to_string();
        if video_id.is_empty() {
            video_id = entry.children().find(|c| c.tag_name().name() == "videoId").and_then(|c| c.text()).unwrap_or("").to_string();
        }
        let link = entry
            .children()
            .filter(|c| c.has_tag_name("link"))
            .find(|c| c.attribute("rel").map_or(true, |r| r == "alternate"))
            .and_then(|c| c.attribute("href"))
            .unwrap_or("")
            .to_string();
        if video_id.is_empty() {
            if let Some(v) = link.split("v=").nth(1) {
                video_id = v.split('&').next().unwrap_or("").to_string();
            }
        }
        if video_id.is_empty() {
            continue;
        }
        videos.push(Video {
            title: { let t = child_text("title"); if t.is_empty() { "New video".to_string() } else { t } },
            url: if link.is_empty() { format!("https://www.youtube.com/watch?v={video_id}") } else { link },
            published_at: child_text("published"),
            video_id,
        });
    }
    Ok(videos)
}

fn video_state(feed: &YoutubeFeed) -> Vec<String> {
    unique_ids(feed.sent_video_ids.iter().cloned().chain(std::iter::once(feed.last_video_id.clone())))
}

fn checkpoint_timestamp(feed: &YoutubeFeed, recent: &[Video]) -> i64 {
    let stored = timestamp_of(&feed.last_published_at);
    if stored > 0 {
        return stored;
    }
    let seen: HashSet<String> = video_state(feed).into_iter().collect();
    recent.iter().filter(|v| seen.contains(&v.video_id)).map(|v| timestamp_of(&v.published_at)).filter(|t| *t > 0).max().unwrap_or(0)
}

fn videos_to_announce(feed: &YoutubeFeed, recent: &[Video], yt: &YoutubeConfig) -> Vec<Video> {
    if recent.is_empty() {
        return Vec::new();
    }
    let seen_ids = video_state(feed);
    if seen_ids.is_empty() {
        return Vec::new();
    }
    let seen: HashSet<String> = seen_ids.into_iter().collect();
    let checkpoint = checkpoint_timestamp(feed, recent);
    if checkpoint == 0 {
        return Vec::new();
    }
    let hours = (yt.max_announcement_age_hours as i64).clamp(1, 720);
    let min_published = chrono::Utc::now().timestamp_millis() - hours * 3_600_000;
    let mut candidates: Vec<Video> = recent
        .iter()
        .filter(|v| !seen.contains(&v.video_id))
        .filter(|v| {
            let p = timestamp_of(&v.published_at);
            p > checkpoint && p >= min_published
        })
        .cloned()
        .collect();
    candidates.truncate(MAX_BACKLOG_POSTS_PER_FEED);
    candidates.reverse();
    candidates
}

#[derive(Clone)]
struct Patch {
    last_video_id: String,
    last_published_at: String,
    last_checked_at: String,
    sent: Vec<String>,
}

fn feed_patch(feed: &YoutubeFeed, recent: &[Video], announced: &[Video]) -> Patch {
    let latest = recent.first();
    let ids = unique_ids(
        recent.iter().map(|v| v.video_id.clone()).chain(announced.iter().map(|v| v.video_id.clone())).chain(video_state(feed)),
    );
    Patch {
        last_video_id: latest.map(|v| v.video_id.clone()).unwrap_or_else(|| feed.last_video_id.clone()),
        last_published_at: latest.map(|v| v.published_at.clone()).unwrap_or_else(|| feed.last_published_at.clone()),
        last_checked_at: dca_state::util::now_iso(),
        sent: ids,
    }
}

async fn run_check(app: &App) -> CheckOutcome {
    let config = app.config().await;
    let yt = &config.youtube;
    if !yt.enabled {
        return CheckOutcome::Skipped("YouTube notifications are disabled.".into());
    }
    let mut results = Vec::new();
    let mut patches: HashMap<String, Patch> = HashMap::new();

    for feed in &yt.feeds {
        let base = FeedResult { id: feed.id.clone(), name: feed.name.clone(), ..Default::default() };
        if !feed.enabled {
            results.push(FeedResult { skipped: true, reason: "disabled".into(), ..base });
            continue;
        }
        let recent = match fetch_recent_videos(app, &feed.id).await {
            Ok(v) => v,
            Err(error) => {
                results.push(FeedResult { error, ..base });
                continue;
            }
        };
        if recent.is_empty() {
            results.push(FeedResult { skipped: true, reason: "no videos found".into(), ..base });
            continue;
        }
        if video_state(feed).is_empty() {
            patches.insert(feed.id.clone(), feed_patch(feed, &recent, &[]));
            results.push(FeedResult { initialized: true, video_id: recent[0].video_id.clone(), ..base });
            continue;
        }
        let announcements = videos_to_announce(feed, &recent, yt);
        if announcements.is_empty() {
            patches.insert(feed.id.clone(), feed_patch(feed, &recent, &[]));
            results.push(FeedResult { unchanged: true, video_id: recent[0].video_id.clone(), ..base });
            continue;
        }

        let target = if feed.channel_id.is_empty() { yt.default_channel_id.clone() } else { feed.channel_id.clone() };
        let channel = channel_id(&target);
        let channel_ok = match channel {
            Some(c) => c.to_channel(&app.http).await.ok().and_then(|c| c.guild()).is_some(),
            None => false,
        };
        let (mut posted, mut send_error) = (0usize, String::new());
        for video in &announcements {
            if let (true, Some(ch)) = (channel_ok, channel) {
                match ch.say(&app.http, render_template(&yt.announcement_template, &feed.name, &feed.id, &video.video_id, &video.url)).await {
                    Ok(_) => {
                        posted += 1;
                        log_action(
                            app,
                            LogEntry::new("youtube", "YouTube Video Posted", format!("{} posted {}", feed.name, video.url))
                                .guild(&config.bot.guild_id)
                                .meta(json!({ "feedId": feed.id, "videoId": video.video_id, "channelId": target })),
                        )
                        .await;
                    }
                    Err(error) => {
                        send_error = error.to_string();
                        tracing::warn!("Failed to announce YouTube video {}: {error}", video.video_id);
                    }
                }
            }
        }
        patches.insert(feed.id.clone(), feed_patch(feed, &recent, &announcements));
        results.push(FeedResult {
            posted: posted > 0,
            posted_count: posted,
            skipped: posted == 0,
            reason: if !send_error.is_empty() { send_error } else if channel_ok { String::new() } else { "announcement channel is not configured or is not text based".into() },
            video_id: announcements.last().map(|v| v.video_id.clone()).unwrap_or_else(|| recent[0].video_id.clone()),
            ..base
        });
    }

    if !patches.is_empty() {
        let _ = update_config(&app.store, |c| {
            for feed in c.youtube.feeds.iter_mut() {
                if let Some(p) = patches.get(&feed.id) {
                    feed.last_video_id = p.last_video_id.clone();
                    feed.last_published_at = p.last_published_at.clone();
                    feed.last_checked_at = p.last_checked_at.clone();
                    feed.sent_video_ids = p.sent.clone();
                }
            }
        })
        .await;
    }
    CheckOutcome::Results(results)
}

/// `/yt check` and the scheduler: only one check at a time.
pub async fn check_feeds(app: &App) -> CheckOutcome {
    if IN_FLIGHT.swap(true, Ordering::SeqCst) {
        return CheckOutcome::Skipped("A YouTube feed check is already running.".into());
    }
    let outcome = run_check(app).await;
    IN_FLIGHT.store(false, Ordering::SeqCst);
    outcome
}

pub fn start_notifier(app: Arc<App>) {
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(10)).await;
        loop {
            let config = app.config().await;
            if config.youtube.enabled {
                let _ = check_feeds(&app).await;
            }
            // The interval is re-read every cycle, so a dashboard change applies without a restart.
            let minutes = config.youtube.check_interval_minutes.max(1) as u64;
            tokio::time::sleep(Duration::from_secs(minutes * 60)).await;
        }
    });
}
