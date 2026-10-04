//! `/top3te`, `/top3km`, `/teameventsummary`: announcement helpers.

use crate::app::App;
use crate::opts;
use crate::responder::{ReplyData, Responder};
use crate::util::*;
use serenity::all::*;

/// Turn a mention (or id) into `<@id>` when the member exists; otherwise keep the raw text.
async fn resolve_mention(app: &App, guild: Option<GuildId>, input: &str) -> String {
    let re = regex::Regex::new(r"^<@!?(\d+)>$").unwrap();
    if let (Some(c), Some(g)) = (re.captures(input), guild) {
        if let Ok(id) = c[1].parse::<u64>() {
            if app.http.get_member(g, UserId::new(id)).await.is_ok() {
                return format!("<@{id}>");
            }
        }
    }
    input.to_string()
}

pub async fn slash_top3te(app: &App, r: &Responder, cmd: &CommandInteraction) -> BotResult<()> {
    r.defer(false).await?;
    let options = cmd.data.options();
    let Some(role) = opts::role(&options, "pingrole") else { return err("Missing role.") };
    let Some(shot) = opts::attachment(&options, "screenshot") else { return err("Missing screenshot.") };
    let first = resolve_mention(app, cmd.guild_id, opts::string(&options, "first").unwrap_or("")).await;
    let second = resolve_mention(app, cmd.guild_id, opts::string(&options, "second").unwrap_or("")).await;
    let third = resolve_mention(app, cmd.guild_id, opts::string(&options, "third").unwrap_or("")).await;
    let text = format!(
        "\n<@&{}>\n\n\u{1f3c6} **Top 3 Winners - Team Event** \u{1f3c6}\n\n\u{1f947} **First Position:** {first}  \n\u{1f948} **Second Position:** {second}  \n\u{1f949} **Third Position:** {third}\n\n\u{1f389} Congratulations to all three podium winners! \u{1f38a}  \n**Great win everyone! Good job!**  \nAnd **thanks to the rest of the team** for your contribution.  \n\n**See You Next Match**  \n**Good Luck! \u{1f340}**\n      ",
        role.id
    );
    let embed = CreateEmbed::new()
        .colour(Colour::new(0xFFD700))
        .title("\u{1f3c5} Team Event Podium Recap")
        .description("Here\u{2019}s a snapshot of the winners!")
        .image(&shot.url)
        .footer(CreateEmbedFooter::new("Keep up the great teamwork!"));
    r.edit_reply(ReplyData::text(text).embed(embed)).await
}

pub async fn slash_top3km(app: &App, r: &Responder, cmd: &CommandInteraction) -> BotResult<()> {
    let options = cmd.data.options();
    let Some(role) = opts::role(&options, "pingrole") else { return err("Missing role.") };
    let chest = opts::integer(&options, "chestlevel").unwrap_or(0);
    let Some(shot) = opts::attachment(&options, "screenshot") else { return err("Missing screenshot.") };
    let reward_role = RoleId::new(1137828749753188382);

    async fn resolve(app: &App, guild: Option<GuildId>, input: &str) -> (String, Option<UserId>) {
        let re = regex::Regex::new(r"<@!?(\d+)>|(\d{17,})").unwrap();
        let Some(c) = re.captures(input) else { return (input.to_string(), None) };
        let id = c.get(1).or_else(|| c.get(2)).and_then(|m| m.as_str().parse::<u64>().ok()).map(UserId::new);
        let Some(id) = id else { return (input.to_string(), None) };
        if let Some(g) = guild {
            if let Ok(m) = app.http.get_member(g, id).await {
                return (format!("<@{id}> ({})", m.display_name()), Some(id));
            }
        }
        (format!("<@{id}> (Unknown User)"), Some(id))
    }
    let (first, f_id) = resolve(app, cmd.guild_id, opts::string(&options, "first").unwrap_or("")).await;
    let (second, s_id) = resolve(app, cmd.guild_id, opts::string(&options, "second").unwrap_or("")).await;
    let (third, t_id) = resolve(app, cmd.guild_id, opts::string(&options, "third").unwrap_or("")).await;

    let description = format!(
        "\n<@&{}>\n\nWe got **level {chest} chest!** this time\u{1f525}, Let's aim higher next time!\u{1f4aa}\u{1f3fb}\n\nOur top 3 km drivers for this week are:\n\u{1f947} 1st: {first}\n\u{1f948} 2nd: {second}\n\u{1f949} 3rd: {third}\n\nGood work, top 3 drivers \u{1f389} and they have earned <@&{}> for this week!\nLet's see who will be the next <@&{}>!\n\nGreat work out there guys \u{1f44f}\u{1f3fb}\n\nAlso **thanks to the rest of the members for contributing to the kms**.\n        ",
        role.id, reward_role, reward_role
    );
    let embed = CreateEmbed::new().colour(Colour::new(0x1abc9c)).title("Top 3 KM Drivers of the Week").description(description).image(&shot.url);
    let users: Vec<UserId> = [f_id, s_id, t_id].into_iter().flatten().collect();
    let mentions = CreateAllowedMentions::new().users(users).roles(vec![role.id, reward_role]);
    r.reply(ReplyData { embeds: vec![embed], allowed_mentions: Some(mentions), ..Default::default() }).await
}

pub async fn slash_summary(_app: &App, r: &Responder, cmd: &CommandInteraction) -> BotResult<()> {
    let options = cmd.data.options();
    let get = |n: &str| opts::string(&options, n).unwrap_or("").to_string();
    let mut embed = CreateEmbed::new()
        .colour(Colour::new(0x0099ff))
        .title("Summary of Team Event after Match")
        .field("Won Streak", get("won_streak"), false)
        .field("Lost Streak", get("lost_streak"), false)
        .field("Match Toppers", format!("1. {}\n2. {}\n3. {}", get("topper1"), get("topper2"), get("topper3")), false)
        .field("Total Points of Match", get("total_points_match"), true)
        .field("Total Points", get("total_points"), true)
        .field("Total Points Gained", get("total_points_gained"), true)
        .field("Aura Count", get("aura_count"), true)
        .timestamp(Timestamp::now());
    if let Some(shot) = opts::attachment(&options, "screenshot") {
        embed = embed.image(&shot.url);
    }
    let mut data = ReplyData::default().embed(embed);
    if let Some(role) = opts::role(&options, "ping_role") {
        data.content = Some(format!("<@&{}>", role.id));
    }
    r.reply(data).await
}
