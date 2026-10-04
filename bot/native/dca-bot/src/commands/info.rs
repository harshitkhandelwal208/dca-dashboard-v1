//! Information and small utility commands: whois, help, dashboard, invite, yt, team list, announce, bam,
//! pingmessage, temperature.

use crate::app::App;
use crate::managers::recruitment::send_invite_to_applicant;
use crate::managers::youtube::{check_feeds, CheckOutcome};
use crate::opts;
use crate::responder::{ReplyData, Responder};
use crate::util::*;
use dca_state::config::DashboardConfig;
use serenity::all::*;

pub fn dashboard_url(config: &DashboardConfig) -> String {
    let base = if config.bot.dashboard_url.is_empty() { std::env::var("DASHBOARD_BASE_URL").unwrap_or_default() } else { config.bot.dashboard_url.clone() };
    if base.is_empty() {
        String::new()
    } else {
        format!("{}/dashboard", base.trim_end_matches('/'))
    }
}

// ================================================================================================ whois

fn created_ms(id: UserId) -> i64 {
    id.created_at().unix_timestamp() * 1000
}

async fn whois_embed(app: &App, guild: Option<GuildId>, target: &User, full: bool) -> CreateEmbed {
    let member = match guild {
        Some(g) => app.http.get_member(g, target.id).await.ok(),
        None => None,
    };
    let avatar = target.face();
    let created = created_ms(target.id);
    let joined = member.as_ref().and_then(|m| m.joined_at).map(|t| t.unix_timestamp() * 1000);
    let mut embed = CreateEmbed::new().title(&target.name).thumbnail(avatar.clone());
    let mut color = Colour::new(0x3498db);

    let mut fields: Vec<(String, String, bool)> = vec![("ID".into(), target.id.to_string(), true), ("Avatar".into(), format!("[Link]({avatar})"), true)];
    if full {
        let user = app.http.get_user(target.id).await.unwrap_or_else(|_| target.clone());
        let banner = user.banner_url();
        fields.push(("Banner".into(), banner.clone().map(|b| format!("[Link]({b})")).unwrap_or_else(|| "No banner".into()), true));
        fields.push(("Accent Color".into(), user.accent_colour.map(|c| format!("#{:06X}", c.0)).unwrap_or_else(|| "None".into()), true));
        if let Some(c) = user.accent_colour {
            color = c;
        }
        if let (Some(g), Some(m)) = (guild, member.as_ref()) {
            if let Ok((_, roles)) = app.guild_roles(g).await {
                let top = m.roles.iter().filter_map(|r| roles.get(r)).filter(|r| r.colour.0 != 0).max_by_key(|r| r.position);
                if let Some(r) = top {
                    color = r.colour;
                }
            }
        }
        if let Some(b) = banner {
            embed = embed.image(b);
        }
    }
    fields.push(("Account Created".into(), discord_timestamp(created, 'f'), true));
    fields.push(("Account Age".into(), discord_timestamp(created, 'R'), true));
    fields.push(("Joined Server At".into(), joined.map(|j| discord_timestamp(j, 'f')).unwrap_or_else(|| "Couldn't find out".into()), true));
    fields.push(("Join Server Age".into(), joined.map(|j| discord_timestamp(j, 'R')).unwrap_or_else(|| "Couldn't find out".into()), true));
    // The bot does not use the presence intent, so (like before) no custom status is ever visible.
    fields.push(("Status".into(), "No custom status".into(), false));
    for (n, v, i) in fields {
        embed = embed.field(n, v, i);
    }
    embed.colour(color)
}

pub async fn slash_whois(app: &App, r: &Responder, cmd: &CommandInteraction) -> BotResult<()> {
    let options = cmd.data.options();
    let target = opts::user(&options, "target").cloned().unwrap_or_else(|| cmd.user.clone());
    let embed = whois_embed(app, cmd.guild_id, &target, false).await;
    r.reply(ReplyData::default().embed(embed)).await
}

pub async fn text_whois(app: &App, msg: &Message) -> BotResult<()> {
    let target = msg.mentions.first().cloned().unwrap_or_else(|| msg.author.clone());
    let embed = whois_embed(app, msg.guild_id, &target, true).await;
    msg.channel_id.send_message(&app.http, CreateMessage::new().embed(embed).reference_message(msg)).await?;
    Ok(())
}

// ================================================================================================= help

pub async fn slash_help(app: &App, r: &Responder) -> BotResult<()> {
    let config = app.config().await;
    let url = dashboard_url(&config);
    let embed = CreateEmbed::new()
        .title("DCA Bot Help")
        .colour(Colour::new(0x37d6a7))
        .description("Core commands for recruitment, team management, reports, moderation, reminders, and dashboard administration.")
        .field("Recruitment", "`/tickets setup`, `/tickets status`, `/tickets logs`, `/tickets claim`, `/tickets close`, `/tickets add`, `/tickets remove`, `/tickets screenshot-list`, `screenshot-add`, `screenshot-change`, `screenshot-remove`, `/tickets archive`, `/tickets delete`, `/invite`, `/ban`", false)
        .field("Spreadsheets And Team Events", "Submit one or more screenshots in the configured monitored channel. Images from the same user/channel are grouped for the dashboard grouping window, then the local PaddleOCR reader extracts event name, ranks, players, scores, teams and podium data from each screenshot (cropped, odd-sized and even photographed screens are supported).", false)
        .field("Output And Files", "Normal event output posts the final `.xlsx`, a full spreadsheet image, and a chart image to the configured output channel. Event names come from visible screenshot titles. The OCR readings are kept temporarily for audit and cleaned by retention.", false)
        .field("#KAB And Missed Events", "`#KAB` is the count of events where a player ranked above every single opponent. If a known own-team player is absent from an event, that event is scored as `0` in summaries, weekly reports, and monthly reports.", false)
        .field("Reports And Corrections", "Weekly reports post when the team event name changes. Monthly reports post after month end. Staff can use `/spreadsheets correct-name`, `correct-team`, `correct-placement`, `correct-points`, `correct-event-name`, `rebuild`, `regenerate-weekly`, and `regenerate-monthly`.", false)
        .field("Temporary Spreadsheet Tests", "`/spreadsheets test-ocr`, `test-grouping`, `preview`, `rebuild-event`, `force-weekly`, and `force-monthly` are temporary development commands and can be removed later.", false)
        .field("Teams And Counts", "`/membercount set`, `/membercount sync`, `/teamcount`, `/updatecount`, `/top3km`, `/top3te`, `/teameventsummary`", false)
        .field("Moderation", "`/warn`, `/clearwarns`, `/whois`, `/snap`, `/roles`, prefix commands like `-clean`, `-kick`, `-mute`, `-unban`", false)
        .field("Reminders And Feeds", "`/remindMe`, `/reminders`, `/cancelreminder`, `/yt`", false)
        .field("Dashboard", if url.is_empty() { "`/dashboard` or configure a dashboard URL in the Server page.".to_string() } else { format!("[Open dashboard]({url})") }, false)
        .timestamp(Timestamp::now());
    r.reply(ReplyData::default().embed(embed).ephemeral()).await
}

pub async fn text_help(app: &App, msg: &Message) -> BotResult<()> {
    let config = app.config().await;
    let url = dashboard_url(&config);
    let list = truncate(&super::TEXT_COMMANDS.iter().map(|(n, d)| format!("- `-{n}` - {d}")).collect::<Vec<_>>().join("\n"), 1000);
    let embed = CreateEmbed::new()
        .title("DCA Bot Help")
        .description("Use slash commands for dashboard-backed features. Prefix commands remain available for older moderation utilities.")
        .colour(Colour::new(0x00b0f4))
        .field("Team Event Spreadsheets", "Post screenshots in the configured channel. The bot groups images by user/channel and window, reads them with the local PaddleOCR engine to extract event names, ranks, players, scores and teams, and posts the final XLSX plus full spreadsheet and chart images.", false)
        .field("#KAB, Reports, Corrections", "`#KAB` means the player ranked above every opponent. Missing known own-team players score `0`. Weekly reports post when event names change; monthly reports post after month end. Use `/spreadsheets correct-*`, `rebuild`, `regenerate-weekly`, and `regenerate-monthly` for fixes.", false)
        .field("Temporary Spreadsheet Tests", "`/spreadsheets test-ocr`, `test-grouping`, `preview`, `rebuild-event`, `force-weekly`, `force-monthly` are temporary development commands.", false)
        .field("Recruitment", "`/tickets status`, `/tickets claim`, `/tickets close`, `/tickets logs`, `/tickets screenshot-list`, `screenshot-add`, `screenshot-change`, `screenshot-remove`, `/invite`, `/ban`", false)
        .field("Team Counts And Feeds", "`/membercount set`, `/membercount sync`, `/teamcount`, `/updatecount`, `/yt`, `/remindMe`", false)
        .field("Prefix Commands", list, false)
        .field("Dashboard", if url.is_empty() { "Dashboard URL is not configured.".to_string() } else { url }, false)
        .footer(CreateEmbedFooter::new(format!("Prefix commands: {}", super::TEXT_COMMANDS.len())));
    msg.channel_id.send_message(&app.http, CreateMessage::new().embed(embed)).await?;
    Ok(())
}

pub async fn slash_dashboard(app: &App, r: &Responder) -> BotResult<()> {
    let url = dashboard_url(&app.config().await);
    r.reply(ReplyData::text(if url.is_empty() { "Dashboard URL is not configured yet.".to_string() } else { format!("Dashboard: {url}") }).ephemeral()).await
}

fn status_line(app: &App, rest_ms: u128) -> String {
    let secs = app.uptime_secs();
    let ocr = if app.reader().is_some() { "ready" } else if app.reader_error.read().unwrap().is_empty() { "loading" } else { "unavailable" };
    let gateway = app.gateway_ms.load(std::sync::atomic::Ordering::Relaxed);
    format!(
        "\u{1f3d3} Pong! Round trip **{rest_ms} ms** \u{b7} gateway **{}** \u{b7} uptime **{}h {}m** \u{b7} OCR **{ocr}**",
        if gateway == 0 { "n/a".to_string() } else { format!("{gateway} ms") },
        secs / 3600,
        (secs % 3600) / 60
    )
}

/// The round trip is measured on the bot's own request (reply time), not from message timestamps, so it does not
/// depend on the host's clock being in step with Discord's.
pub async fn slash_ping(app: &App, r: &Responder, _cmd: &CommandInteraction) -> BotResult<()> {
    let started = std::time::Instant::now();
    r.reply(ReplyData::text("\u{1f3d3} Pinging...")).await?;
    let rest = started.elapsed().as_millis();
    r.edit_reply(ReplyData::text(status_line(app, rest))).await
}

pub async fn text_ping(app: &App, msg: &Message) -> BotResult<()> {
    let started = std::time::Instant::now();
    let mut sent = msg.reply(&app.http, "\u{1f3d3} Pinging...").await?;
    let rest = started.elapsed().as_millis();
    sent.edit(&app.http, EditMessage::new().content(status_line(app, rest))).await?;
    Ok(())
}

pub async fn slash_invite(app: &App, r: &Responder) -> BotResult<()> {
    r.defer(true).await?;
    send_invite_to_applicant(app, r).await;
    Ok(())
}

// ==================================================================================================== yt

pub async fn slash_yt(app: &App, r: &Responder, cmd: &CommandInteraction) -> BotResult<()> {
    if !r.permissions().contains(Permissions::MANAGE_GUILD) && !r.permissions().contains(Permissions::ADMINISTRATOR) {
        return r.reply(ReplyData::text("You don't have permission to use this command.").ephemeral()).await;
    }
    let options = cmd.data.options();
    let Some((sub, _)) = opts::subcommand(&options) else { return Ok(()) };
    r.defer(true).await?;
    if sub == "list" {
        let config = app.config().await;
        let rows: Vec<String> = config
            .youtube
            .feeds
            .iter()
            .map(|f| {
                let ch = if f.channel_id.is_empty() { &config.youtube.default_channel_id } else { &f.channel_id };
                format!("**{}** - {} - {}", f.name, if f.enabled { "enabled" } else { "disabled" }, if ch.is_empty() { "no Discord channel".to_string() } else { format!("<#{ch}>") })
            })
            .collect();
        return r.edit_reply(ReplyData::text(if rows.is_empty() { "No YouTube feeds are configured.".to_string() } else { rows.join("\n") })).await;
    }
    match check_feeds(app).await {
        CheckOutcome::Skipped(reason) => r.edit_reply(ReplyData::text(format!("Skipped: {reason}"))).await,
        CheckOutcome::Results(results) => {
            let lines: Vec<String> = results
                .iter()
                .map(|i| {
                    if !i.error.is_empty() {
                        format!("**{}** - error: {}", i.name, i.error)
                    } else if i.posted {
                        format!("**{}** - posted {} video(s), latest {}", i.name, i.posted_count.max(1), i.video_id)
                    } else if i.initialized {
                        format!("**{}** - initialized {}", i.name, i.video_id)
                    } else if i.unchanged {
                        format!("**{}** - unchanged", i.name)
                    } else {
                        format!("**{}** - skipped: {}", i.name, if i.reason.is_empty() { "no change" } else { &i.reason })
                    }
                })
                .collect();
            r.edit_reply(ReplyData::text(if lines.is_empty() { "No feeds checked.".to_string() } else { truncate(&lines.join("\n"), 2000) })).await
        }
    }
}

pub async fn text_yt(app: &App, msg: &Message, args: &[String]) -> BotResult<()> {
    if args.first().map(String::as_str) != Some("list") {
        return Ok(());
    }
    let config = app.config().await;
    let list = config.youtube.feeds.iter().filter(|f| f.enabled).map(|f| format!("**{}** - [YouTube](https://www.youtube.com/channel/{})", f.name, f.id)).collect::<Vec<_>>().join("\n");
    msg.reply(&app.http, if list.is_empty() { "No YouTube channels are being tracked.".to_string() } else { list }).await?;
    Ok(())
}

// ============================================================================================= text utils

pub async fn text_bam(app: &App, msg: &Message) -> BotResult<()> {
    use rand::seq::SliceRandom;
    let Some(target) = msg.mentions.first() else {
        msg.reply(&app.http, "Bruh who am I supposed to BAM? You want **yourself** to get blasted?").await?;
        return Ok(());
    };
    let t = format!("**<@{}>**", target.id);
    let responses = [
        format!("BAM! {t} just got blasted into another dimension!"),
        format!("POW! {t} has been cooked extra crispy!"),
        format!("BOOM! {t} couldn't survive the BAM attack!"),
        format!("WHAM! {t} folded like a school uniform!"),
        format!("KABOOM! {t} got deleted\u{2026} in your imagination"),
    ];
    let pick = responses.choose(&mut rand::thread_rng()).cloned().unwrap_or_default();
    msg.channel_id.say(&app.http, pick).await?;
    Ok(())
}

pub async fn text_pingmessage(app: &App, msg: &Message) -> BotResult<()> {
    let Some(guild) = msg.guild_id else { return Ok(()) };
    if !app.member_has(guild, msg.author.id, Permissions::MANAGE_MESSAGES).await {
        msg.reply(&app.http, "\u{274c} You don't have permission to use this command.").await?;
        return Ok(());
    }
    let target = "Scroll to the top\u{1f446}";
    let recent = msg.channel_id.messages(&app.http, GetMessages::new().limit(10)).await?;
    if recent.iter().any(|m| m.content == target) {
        msg.reply(&app.http, "\u{2705} The message already exists.").await?;
        return Ok(());
    }
    msg.channel_id.say(&app.http, target).await?;
    Ok(())
}

pub async fn text_announce(app: &App, msg: &Message) -> BotResult<()> {
    let Some(guild) = msg.guild_id else { return Ok(()) };
    let developer_role = RoleId::new(1367467038737436712);
    let has = app.http.get_member(guild, msg.author.id).await.map(|m| m.roles.contains(&developer_role)).unwrap_or(false);
    if !has {
        msg.reply(&app.http, "Only developers can use this command.\u{274c} ").await?;
        return Ok(());
    }
    let channel = ChannelId::new(1293490674884149329);
    if channel.to_channel(&app.http).await.is_err() {
        msg.reply(&app.http, "\u{274c} Announcement channel not found.").await?;
        return Ok(());
    }
    let embed = CreateEmbed::new()
        .colour(Colour::new(0xFF6600))
        .title("\u{1f389} 1 Year Anniversary + Bot Update")
        .description("It's been 1 year!\u{1f409}\n\n**Bug Fixed**\nBot is back online with improvements.\n\n**Optimized**\nPerformance enhanced.")
        .footer(CreateEmbedFooter::new("Thank you for your patience and support!"))
        .timestamp(Timestamp::now());
    match channel.send_message(&app.http, CreateMessage::new().embed(embed)).await {
        Ok(_) => msg.reply(&app.http, "\u{2705} Announcement sent!").await?,
        Err(_) => msg.reply(&app.http, "\u{274c} Failed to send announcement.").await?,
    };
    Ok(())
}

pub async fn text_team(app: &App, msg: &Message) -> BotResult<()> {
    let Some(guild) = msg.guild_id else { return Ok(()) };
    let (leader, co_leader, driver) = (RoleId::new(1345425056586661961), RoleId::new(1346297821120299028), RoleId::new(1341452771567599617));
    let mut members: Vec<Member> = Vec::new();
    let mut after: Option<u64> = None;
    loop {
        let batch = app.http.get_guild_members(guild, Some(1000), after).await?;
        if batch.is_empty() {
            break;
        }
        after = batch.last().map(|m| m.user.id.get());
        let n = batch.len();
        members.extend(batch);
        if n < 1000 {
            break;
        }
    }
    let with = |role: RoleId| -> Vec<&Member> { members.iter().filter(|m| m.roles.contains(&role)).collect() };
    let leaders = with(leader);
    let co_leaders: Vec<&Member> = with(co_leader).into_iter().filter(|m| !leaders.iter().any(|l| l.user.id == m.user.id)).collect();
    let drivers: Vec<&Member> = with(driver).into_iter().filter(|m| !leaders.iter().any(|l| l.user.id == m.user.id) && !co_leaders.iter().any(|l| l.user.id == m.user.id)).collect();
    let re = regex::Regex::new(r"^\d+\.\s*").unwrap();
    let format_list = |list: &[&Member]| -> String {
        if list.is_empty() {
            return "No members.".into();
        }
        list.iter()
            .enumerate()
            .map(|(i, m)| {
                let ign = m.nick.clone().unwrap_or_else(|| m.user.name.clone());
                format!("{}. {} - <@{}>", i + 1, re.replace(&ign, ""), m.user.id)
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    let text = format!(
        "**Discord 3\u{2122} (Auto Updated)**\n\n\u{1f7e1} **Leader**\n{}\n\n\u{1f534} **Co-Leaders**\n{}\n\n\u{1f7e2} **Drivers**\n{}",
        format_list(&leaders),
        format_list(&co_leaders),
        format_list(&drivers)
    );
    // The old command failed on lists longer than Discord's 2000 character limit; split instead.
    let mut chunk = String::new();
    for line in text.split('\n') {
        if chunk.len() + line.len() + 1 > 1900 {
            msg.channel_id.send_message(&app.http, CreateMessage::new().content(&chunk).allowed_mentions(CreateAllowedMentions::new())).await?;
            chunk.clear();
        }
        chunk.push_str(line);
        chunk.push('\n');
    }
    if !chunk.trim().is_empty() {
        msg.channel_id.send_message(&app.http, CreateMessage::new().content(chunk).allowed_mentions(CreateAllowedMentions::new())).await?;
    }
    Ok(())
}

pub async fn text_temperature(app: &App, msg: &Message, args: &[String]) -> BotResult<()> {
    let city = args.join(" ");
    if city.is_empty() {
        msg.reply(&app.http, "\u{2757} Please provide a city name.").await?;
        return Ok(());
    }
    let api_key = "e059b3064ced30668da71497d1711908";
    let response = app
        .web
        .get("https://api.openweathermap.org/data/2.5/weather")
        .query(&[("q", city.as_str()), ("appid", api_key), ("units", "metric")])
        .send()
        .await;
    let data: serde_json::Value = match response {
        Ok(r) if r.status().is_success() => r.json().await.unwrap_or_default(),
        _ => {
            msg.reply(&app.http, "\u{26a0}\u{fe0f} Could not fetch the weather. Please check the city name.").await?;
            return Ok(());
        }
    };
    let desc = data["weather"][0]["description"].as_str().unwrap_or("");
    let capitalised = desc.chars().next().map(|c| c.to_uppercase().collect::<String>() + &desc[c.len_utf8()..]).unwrap_or_default();
    let embed = CreateEmbed::new()
        .title(format!("\u{1f324}\u{fe0f} Weather in {}, {}", data["name"].as_str().unwrap_or(""), data["sys"]["country"].as_str().unwrap_or("")))
        .description(capitalised)
        .thumbnail(format!("http://openweathermap.org/img/wn/{}@2x.png", data["weather"][0]["icon"].as_str().unwrap_or("01d")))
        .field("\u{1f321}\u{fe0f} Temperature", format!("{}\u{b0}C", data["main"]["temp"]), true)
        .field("Feels Like", format!("{}\u{b0}C", data["main"]["feels_like"]), true)
        .field("\u{1f4a7} Humidity", format!("{}%", data["main"]["humidity"]), true)
        .field("\u{1f32c}\u{fe0f} Wind Speed", format!("{} m/s", data["wind"]["speed"]), true)
        .colour(Colour::new(0x1e90ff))
        .footer(CreateEmbedFooter::new("Powered by OpenWeatherMap"));
    msg.channel_id.send_message(&app.http, CreateMessage::new().embed(embed)).await?;
    Ok(())
}
