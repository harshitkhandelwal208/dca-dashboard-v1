//! Moderation: ban, kick, mute, unban, warn(s), clean, snap/snapban, mass kick. Slash and prefix versions.

use crate::app::App;
use crate::managers::ban_panel::sync_recruitment_ban_list;
use crate::managers::team_roles::recruitment_guild_id;
use crate::opts;
use crate::responder::{ReplyData, Responder};
use crate::util::*;
use dca_state::models::RecruitmentBan;
use dca_state::stores::*;
use serenity::all::*;
use std::time::Duration;

fn discord_code(error: &serenity::Error) -> Option<isize> {
    match error {
        serenity::Error::Http(http) => match http {
            HttpError::UnsuccessfulRequest(resp) => Some(resp.error.code),
            _ => None,
        },
        _ => None,
    }
}

async fn guild_name(app: &App, guild: GuildId) -> String {
    app.http.get_guild(guild).await.map(|g| g.name).unwrap_or_else(|_| "the server".into())
}

async fn dm(app: &App, user: &User, message: CreateMessage) -> bool {
    match user.create_dm_channel(&app.http).await {
        Ok(ch) => ch.send_message(&app.http, message).await.is_ok(),
        Err(_) => false,
    }
}

/// Is this the recruitment server (bans there are mirrored into the team-join ban list)?
async fn is_recruitment_ban_server(app: &App, guild: GuildId) -> bool {
    let config = app.config().await;
    let configured = if config.bot.recruitment_guild_id.is_empty() { std::env::var("RECRUITMENT_GUILD_ID").unwrap_or_default() } else { config.bot.recruitment_guild_id.clone() };
    if !configured.is_empty() {
        return guild.to_string() == configured;
    }
    if let Some(ch) = channel_id(&config.recruitment.ban_list_channel_id) {
        return match ch.to_channel(&app.http).await.ok().and_then(|c| c.guild()) {
            Some(gc) => gc.guild_id == guild,
            None => false,
        };
    }
    guild.to_string() == recruitment_guild_id(&config)
}

async fn record_recruitment_ban(app: &App, guild: GuildId, target: UserId, tag: &str, reason: &str, by: &User) {
    if !is_recruitment_ban_server(app, guild).await {
        return;
    }
    let _ = add_recruitment_ban(
        &app.store,
        RecruitmentBan {
            user_id: target.to_string(),
            user_tag: tag.to_string(),
            reason: reason.to_string(),
            banned_by_id: by.id.to_string(),
            banned_by_tag: display_tag(by),
            guild_id: guild.to_string(),
            ..Default::default()
        },
    )
    .await;
    let _ = sync_recruitment_ban_list(app, false).await;
}

// =============================================================================================== slash

pub async fn slash_ban(app: &App, r: &Responder, cmd: &CommandInteraction) -> BotResult<()> {
    let options = cmd.data.options();
    let Some(guild) = cmd.guild_id else { return err("Use this command in a server.") };
    let Some(target) = opts::user(&options, "user") else { return err("Missing user.") };
    let reason = opts::string(&options, "reason").unwrap_or("No reason provided");
    if !r.permissions().contains(Permissions::BAN_MEMBERS) && !r.permissions().contains(Permissions::ADMINISTRATOR) {
        return r.reply(ReplyData::text("You don't have permission to use this command.").ephemeral()).await;
    }
    match guild.ban_with_reason(&app.http, target.id, 0, reason).await {
        Ok(()) => {}
        Err(e) if discord_code(&e) == Some(50013) => return r.reply(ReplyData::text("I cannot ban this user.").ephemeral()).await,
        Err(e) => return Err(e.into()),
    }
    record_recruitment_ban(app, guild, target.id, &display_tag(target), reason, r.user()).await;
    r.reply(ReplyData::text(format!("**{}** has been banned. Reason: **{reason}**", display_tag(target)))).await
}

pub async fn slash_warn(app: &App, r: &Responder, cmd: &CommandInteraction) -> BotResult<()> {
    let options = cmd.data.options();
    let Some(guild) = cmd.guild_id else { return err("Use this command in a server.") };
    let Some(target) = opts::user(&options, "user") else { return err("Missing user.") };
    let reason = opts::string(&options, "reason").unwrap_or("No reason provided.");
    if !r.permissions().contains(Permissions::MANAGE_MESSAGES) && !r.permissions().contains(Permissions::ADMINISTRATOR) {
        return r.reply(ReplyData::text("You don't have permission to use this command.").ephemeral()).await;
    }
    match add_warning(&app.store, &target.id.to_string(), &guild.to_string(), reason).await {
        Ok(_) => r.reply(ReplyData::text(format!("\u{2705} **{}** has been warned for: **{reason}**", display_tag(target)))).await,
        Err(_) => r.reply(ReplyData::text("\u{274c} An error occurred while adding the warning.").ephemeral()).await,
    }
}

pub async fn slash_clearwarns(app: &App, r: &Responder, cmd: &CommandInteraction) -> BotResult<()> {
    let options = cmd.data.options();
    let Some(guild) = cmd.guild_id else { return err("Use this command in a server.") };
    let Some(target) = opts::user(&options, "user") else { return err("Missing user.") };
    if !r.permissions().contains(Permissions::MANAGE_MESSAGES) && !r.permissions().contains(Permissions::ADMINISTRATOR) {
        return r.reply(ReplyData::text("You don't have permission to use this command.").ephemeral()).await;
    }
    match clear_warnings(&app.store, &target.id.to_string(), &guild.to_string()).await {
        Ok(0) => r.reply(ReplyData::text(format!("\u{274c} **{}** has no warnings to clear.", display_tag(target))).ephemeral()).await,
        Ok(_) => r.reply(ReplyData::text(format!("\u{2705} Cleared all warnings for **{}**.", display_tag(target)))).await,
        Err(_) => r.reply(ReplyData::text("\u{274c} An error occurred while clearing warnings.").ephemeral()).await,
    }
}

const SNAP_QUOTES: [&str; 5] = [
    "You should\u{2019}ve gone for the head.",
    "I am inevitable.",
    "Dread it. Run from it. Destiny arrives all the same.",
    "Perfectly balanced, as all things should be.",
    "Reality is often disappointing.",
];

pub async fn slash_snap(app: &App, r: &Responder, cmd: &CommandInteraction) -> BotResult<()> {
    use rand::seq::SliceRandom;
    let options = cmd.data.options();
    let Some(guild) = cmd.guild_id else { return err("Use this command in a server.") };
    if !r.permissions().contains(Permissions::ADMINISTRATOR) {
        return r.reply(ReplyData::text("You don't have permission to use this command.").ephemeral()).await;
    }
    let Some((sub, sub_opts)) = opts::subcommand(&options) else { return Ok(()) };
    let Some(target) = opts::user(sub_opts, "user") else { return err("Missing user.") };
    let reason = opts::string(sub_opts, "reason").unwrap_or("No reason provided.");
    let quote = SNAP_QUOTES.choose(&mut rand::thread_rng()).copied().unwrap_or(SNAP_QUOTES[0]);
    if app.http.get_member(guild, target.id).await.is_err() {
        return r.reply(ReplyData::text("User not found in this server.").ephemeral()).await;
    }
    let tag = display_tag(target);
    if sub == "ban" {
        match guild.ban_with_reason(&app.http, target.id, 0, reason).await {
            Ok(()) => {}
            Err(e) if discord_code(&e) == Some(50013) => return r.reply(ReplyData::text("\u{26a0}\u{fe0f} I can't snap that user. They're too powerful!").ephemeral()).await,
            Err(e) => return Err(e.into()),
        }
        let embed = CreateEmbed::new()
            .title("\u{1f4a5} **Snap Ban Executed**")
            .description(format!("\u{1f480} *\"{quote}\"*\n\n\u{1f4a8} **{tag} has been banned from the universe!**"))
            .colour(Colour::new(0x800080))
            .image("https://media.tenor.com/1B8uvL_sUwEAAAAC/thanos-snap.gif")
            .footer(CreateEmbedFooter::new(format!("Snapped by {}", display_tag(r.user()))))
            .timestamp(Timestamp::now());
        return r.reply(ReplyData::default().embed(embed)).await;
    }
    match guild.kick_with_reason(&app.http, target.id, reason).await {
        Ok(()) => {}
        Err(e) if discord_code(&e) == Some(50013) => return r.reply(ReplyData::text("\u{26a0}\u{fe0f} I can't snap that user. They're too powerful!").ephemeral()).await,
        Err(e) => return Err(e.into()),
    }
    let embed = CreateEmbed::new()
        .title("\u{1f4a8} **Snap Kick Executed**")
        .description(format!("*\"{quote}\"*\n\n **{tag} has been kicked out of existence!**"))
        .colour(Colour::new(0x990000))
        .image("https://media.tenor.com/G9dQID3iMjUAAAAC/thanos.gif")
        .footer(CreateEmbedFooter::new(format!("Snapped by {}", display_tag(r.user()))))
        .timestamp(Timestamp::now());
    r.reply(ReplyData::default().embed(embed)).await
}

// ================================================================================================ text

async fn need(app: &App, msg: &Message, perm: Permissions, text: &str) -> bool {
    let Some(guild) = msg.guild_id else { return false };
    if app.member_has(guild, msg.author.id, perm).await {
        return true;
    }
    let _ = msg.reply(&app.http, text).await;
    false
}

async fn resolve_user(app: &App, msg: &Message, args: &[String]) -> Option<User> {
    if let Some(u) = msg.mentions.first() {
        return Some(u.clone());
    }
    let id = user_id(args.first()?)?;
    app.http.get_user(id).await.ok()
}

pub async fn text_ban(app: &App, msg: &Message, args: &[String]) -> BotResult<()> {
    let Some(guild) = msg.guild_id else { return Ok(()) };
    if !need(app, msg, Permissions::BAN_MEMBERS, "You don't have permission to use this command.").await {
        return Ok(());
    }
    let target_id = msg.mentions.first().map(|u| u.id.to_string()).or_else(|| args.first().cloned());
    let reason = args.iter().skip(1).cloned().collect::<Vec<_>>().join(" ");
    let Some(target_id) = target_id else {
        msg.reply(&app.http, "Please mention a user or provide an ID to ban.").await?;
        return Ok(());
    };
    if reason.is_empty() {
        msg.reply(&app.http, "Please provide a reason for the ban.").await?;
        return Ok(());
    }
    let Some(target) = user_id(&target_id) else {
        msg.reply(&app.http, "Failed to ban the user. They may not exist or already be banned.").await?;
        return Ok(());
    };
    let user = app.http.get_user(target).await.ok();
    let server = guild_name(app, guild).await;
    if let Some(u) = &user {
        if !dm(app, u, CreateMessage::new().content(format!("You have been banned from **{server}**.\nReason: **{reason}**"))).await {
            msg.channel_id.say(&app.http, "Couldn't send a DM to the user. Proceeding with the ban.").await?;
        }
    }
    match guild.ban_with_reason(&app.http, target, 0, &reason).await {
        Ok(()) => {
            record_recruitment_ban(app, guild, target, &user.as_ref().map(display_tag).unwrap_or_default(), &reason, &msg.author).await;
            msg.channel_id.say(&app.http, format!("**<@{target_id}> has been banned.**\nReason: **{reason}**")).await?;
        }
        Err(e) => {
            tracing::warn!("ban failed: {e}");
            msg.reply(&app.http, "Failed to ban the user. They may not exist or already be banned.").await?;
        }
    }
    Ok(())
}

fn kick_dm_embed(server: &str, reason: &str) -> CreateEmbed {
    CreateEmbed::new()
        .colour(Colour::new(0xFF0000))
        .title("You have been Kicked")
        .description(format!("You have been **Kicked** from **{server}**."))
        .field("Reason", reason, false)
        .field("Need Help?", "If you think this is a mistake, please contact:\n- gorillakurt\n- dc\\_void\\_ \n- b7m5", false)
        .timestamp(Timestamp::now())
}

pub async fn text_kick(app: &App, msg: &Message, args: &[String]) -> BotResult<()> {
    let Some(guild) = msg.guild_id else { return Ok(()) };
    if !need(app, msg, Permissions::KICK_MEMBERS, "You do not have permission to use this command.").await {
        return Ok(());
    }
    let member = match msg.mentions.first() {
        Some(u) => app.http.get_member(guild, u.id).await.ok(),
        None => match args.first().and_then(|a| user_id(a)) {
            Some(id) => app.http.get_member(guild, id).await.ok(),
            None => None,
        },
    };
    let Some(member) = member else {
        msg.reply(&app.http, "Please mention a valid member or provide a valid user ID to kick.").await?;
        return Ok(());
    };
    if args.len() < 2 {
        msg.reply(&app.http, "Please provide a reason for the kick.").await?;
        return Ok(());
    }
    let reason = args[1..].join(" ");
    let server = guild_name(app, guild).await;
    let dm_ok = dm(app, &member.user, CreateMessage::new().embed(kick_dm_embed(&server, &reason))).await;
    if !dm_ok {
        msg.reply(&app.http, format!("\u{26a0}\u{fe0f} Could not send a DM to **{}**, but they will still be kicked.", display_tag(&member.user))).await?;
    }
    match guild.kick_with_reason(&app.http, member.user.id, &reason).await {
        Ok(()) => {
            msg.reply(&app.http, format!("Successfully kicked **{}**. Reason: {reason}", display_tag(&member.user))).await?;
        }
        Err(e) if discord_code(&e) == Some(50013) => {
            msg.reply(&app.http, "I cannot kick this user. They might have a higher role or I lack permissions.").await?;
        }
        Err(e) => {
            tracing::warn!("kick failed: {e}");
            msg.reply(&app.http, "An error occurred while trying to kick the user.").await?;
        }
    }
    Ok(())
}

fn parse_mute_duration(time: &str) -> Option<u64> {
    let re = regex::Regex::new(r"^(\d+)([smhd])$").unwrap();
    let c = re.captures(time)?;
    let v: u64 = c[1].parse().ok()?;
    Some(v * match &c[2] { "s" => 1000, "m" => 60_000, "h" => 3_600_000, _ => 86_400_000 })
}

pub async fn text_mute(app: &App, msg: &Message, args: &[String]) -> BotResult<()> {
    let Some(guild) = msg.guild_id else { return Ok(()) };
    if !need(app, msg, Permissions::MODERATE_MEMBERS, "You do not have permission to use this command.").await {
        return Ok(());
    }
    let target = match msg.mentions.first() {
        Some(u) => Some(u.id),
        None => args.first().and_then(|a| user_id(a)),
    };
    let Some(target) = target else {
        msg.reply(&app.http, "Please mention a valid user or provide their ID.").await?;
        return Ok(());
    };
    let Ok(mut member) = app.http.get_member(guild, target).await else {
        msg.reply(&app.http, "Please mention a valid user or provide their ID.").await?;
        return Ok(());
    };
    let time_arg = args.get(1).cloned().unwrap_or_else(|| "10m".into());
    let Some(duration) = parse_mute_duration(&time_arg) else {
        msg.reply(&app.http, "Invalid duration format. Use `1m`, `10m`, `1h`, etc.").await?;
        return Ok(());
    };
    let until = Timestamp::from_unix_timestamp(chrono::Utc::now().timestamp() + (duration / 1000) as i64).map_err(|e| e.to_string())?;
    match member.disable_communication_until_datetime(&app.http, until).await {
        Ok(()) => {
            let embed = CreateEmbed::new()
                .title("User Muted")
                .colour(Colour::new(0xED4245))
                .thumbnail(member.user.face())
                .field("User", format!("{} ({})", display_tag(&member.user), member.user.id), true)
                .field("Duration", &time_arg, true)
                .field("Moderator", display_tag(&msg.author), true)
                .timestamp(Timestamp::now());
            msg.channel_id.send_message(&app.http, CreateMessage::new().embed(embed).reference_message(msg)).await?;
        }
        Err(e) if discord_code(&e) == Some(50013) => {
            msg.reply(&app.http, "I cannot mute this user. They may have a higher role than me.").await?;
        }
        Err(e) => {
            tracing::warn!("mute failed: {e}");
            msg.reply(&app.http, "An error occurred while trying to mute this user.").await?;
        }
    }
    Ok(())
}

pub async fn text_unban(app: &App, msg: &Message, args: &[String]) -> BotResult<()> {
    let Some(guild) = msg.guild_id else { return Ok(()) };
    if !need(app, msg, Permissions::BAN_MEMBERS, "\u{1f6ab} You do not have permission to use this command.").await {
        return Ok(());
    }
    let Some(input) = args.first() else {
        msg.reply(&app.http, "\u{274c} Please provide a user ID or username.").await?;
        return Ok(());
    };
    let bans = match guild.bans(&app.http, None, None).await {
        Ok(b) => b,
        Err(_) => {
            msg.reply(&app.http, "\u{274c} An error occurred while trying to unban the user.").await?;
            return Ok(());
        }
    };
    let found = bans.iter().find(|b| b.user.id.to_string() == *input).or_else(|| bans.iter().find(|b| b.user.name.to_lowercase() == input.to_lowercase()));
    let Some(ban) = found else {
        msg.reply(&app.http, "\u{26a0}\u{fe0f} This user is not banned or does not exist.").await?;
        return Ok(());
    };
    match guild.unban(&app.http, ban.user.id).await {
        Ok(()) => {
            msg.reply(&app.http, format!("\u{2705} Successfully unbanned **{}**.", display_tag(&ban.user))).await?;
        }
        Err(_) => {
            msg.reply(&app.http, "\u{274c} An error occurred while trying to unban the user.").await?;
        }
    }
    Ok(())
}

pub async fn text_warn(app: &App, msg: &Message, args: &[String]) -> BotResult<()> {
    let Some(guild) = msg.guild_id else { return Ok(()) };
    if !need(app, msg, Permissions::MANAGE_MESSAGES, "You do not have permission to use this command.").await {
        return Ok(());
    }
    let Some(target) = msg.mentions.first() else {
        msg.reply(&app.http, "Please mention a user to warn.").await?;
        return Ok(());
    };
    let reason = args.iter().skip(1).cloned().collect::<Vec<_>>().join(" ");
    let reason = if reason.is_empty() { "No reason provided.".to_string() } else { reason };
    match add_warning(&app.store, &target.id.to_string(), &guild.to_string(), &reason).await {
        Ok(_) => msg.reply(&app.http, format!("**{}** has been warned for: **{reason}**", display_tag(target))).await?,
        Err(_) => msg.reply(&app.http, "An error occurred while adding the warning.").await?,
    };
    Ok(())
}

pub async fn text_warnings(app: &App, msg: &Message) -> BotResult<()> {
    let Some(guild) = msg.guild_id else { return Ok(()) };
    if !need(app, msg, Permissions::MANAGE_MESSAGES, "You do not have permission to use this command.").await {
        return Ok(());
    }
    let Some(target) = msg.mentions.first() else {
        msg.reply(&app.http, "Please mention a user to check their warnings.").await?;
        return Ok(());
    };
    let warnings = list_warnings(&app.store, &target.id.to_string(), &guild.to_string()).await;
    if warnings.is_empty() {
        msg.reply(&app.http, format!("**{}** has no warnings.", display_tag(target))).await?;
        return Ok(());
    }
    let list = warnings.iter().enumerate().map(|(i, w)| format!("**{}.** {}", i + 1, w.reason)).collect::<Vec<_>>().join("\n");
    msg.reply(&app.http, format!("Warnings for **{}**:\n{list}", display_tag(target))).await?;
    Ok(())
}

pub async fn text_clearwarns(app: &App, msg: &Message) -> BotResult<()> {
    let Some(guild) = msg.guild_id else { return Ok(()) };
    if !need(app, msg, Permissions::MANAGE_MESSAGES, "You do not have permission to use this command.").await {
        return Ok(());
    }
    let Some(target) = msg.mentions.first() else {
        msg.reply(&app.http, "Please mention a user to clear their warnings.").await?;
        return Ok(());
    };
    match clear_warnings(&app.store, &target.id.to_string(), &guild.to_string()).await {
        Ok(0) => msg.reply(&app.http, format!("**{}** has no warnings to clear.", display_tag(target))).await?,
        Ok(_) => msg.reply(&app.http, format!("Cleared all warnings for **{}**.", display_tag(target))).await?,
        Err(_) => msg.reply(&app.http, "An error occurred while clearing warnings.").await?,
    };
    Ok(())
}

pub async fn text_clean(app: &App, msg: &Message, args: &[String]) -> BotResult<()> {
    if !need(app, msg, Permissions::MANAGE_MESSAGES, "You do not have permission to use this command.").await {
        return Ok(());
    }
    let amount: i64 = args.first().and_then(|a| a.parse().ok()).unwrap_or(0);
    let no_pin = args.iter().any(|a| a == "-nopin");
    let target = msg.mentions.first();
    if !(1..=100).contains(&amount) {
        msg.reply(&app.http, "Please provide a number between 1 and 100.").await?;
        return Ok(());
    }
    let limit = (amount + 50).min(100) as u8;
    let fetched = match msg.channel_id.messages(&app.http, GetMessages::new().limit(limit)).await {
        Ok(m) => m,
        Err(e) => {
            tracing::warn!("clean fetch failed: {e}");
            msg.reply(&app.http, "An error occurred while trying to delete messages.").await?;
            return Ok(());
        }
    };
    let two_weeks = chrono::Utc::now().timestamp() - 14 * 86_400 + 60;
    let mut candidates: Vec<&Message> = fetched.iter().filter(|m| m.id != msg.id).collect();
    if let Some(t) = target {
        candidates.retain(|m| m.author.id == t.id);
    }
    if no_pin {
        candidates.retain(|m| !m.pinned);
    }
    candidates.truncate(amount as usize);
    // Bulk delete only works for messages younger than 14 days (the old code asked discord.js to skip older ones).
    let ids: Vec<MessageId> = candidates.iter().filter(|m| m.timestamp.unix_timestamp() > two_weeks).map(|m| m.id).collect();
    if ids.is_empty() {
        let who = target.map(|t| format!(" from {}", t.name)).unwrap_or_default();
        msg.reply(&app.http, format!("No messages found to delete{who}.")).await?;
        return Ok(());
    }
    let result = if ids.len() == 1 { msg.channel_id.delete_message(&app.http, ids[0]).await } else { msg.channel_id.delete_messages(&app.http, &ids).await };
    match result {
        Ok(()) => {
            let who = target.map(|t| format!(" from **{}**", t.name)).unwrap_or_default();
            let confirm = msg.channel_id.say(&app.http, format!("\u{2705} Deleted **{}** message{}{who}.", ids.len(), if ids.len() != 1 { "s" } else { "" })).await?;
            let http = app.http.clone();
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_secs(3)).await;
                let _ = confirm.delete(&http).await;
            });
        }
        Err(e) if discord_code(&e) == Some(50034) => {
            msg.reply(&app.http, "Cannot delete messages older than 14 days.").await?;
        }
        Err(e) if discord_code(&e) == Some(50013) => {
            msg.reply(&app.http, "I don't have permission to delete messages in this channel.").await?;
        }
        Err(e) => {
            tracing::warn!("clean failed: {e}");
            msg.reply(&app.http, "An error occurred while trying to delete messages.").await?;
        }
    }
    Ok(())
}

pub async fn text_snapban(app: &App, msg: &Message, args: &[String]) -> BotResult<()> {
    let Some(guild) = msg.guild_id else { return Ok(()) };
    if !need(app, msg, Permissions::BAN_MEMBERS, "\u{1f6d1} You don't have the power of the Infinity Gauntlet!").await {
        return Ok(());
    }
    let reason = args.iter().skip(1).cloned().collect::<Vec<_>>().join(" ");
    let reason = if reason.is_empty() { "No reason provided.".to_string() } else { reason };
    let Some(target) = msg.mentions.first() else {
        msg.reply(&app.http, "\u{2757} You need to mention a user to snap! Example: `-snapban @user being sus`").await?;
        return Ok(());
    };
    if target.id == msg.author.id {
        msg.reply(&app.http, "\u{26a0}\u{fe0f} You can't snap yourself, even if you're feeling dramatic.").await?;
        return Ok(());
    }
    let server = guild_name(app, guild).await;
    dm(app, target, CreateMessage::new().content(format!("\u{1f4a5} You have been snapped from **{server}**.\nReason: {reason}"))).await;
    match guild.ban_with_reason(&app.http, target.id, 0, &reason).await {
        Ok(()) => {
            msg.channel_id.say(&app.http, format!("\u{2620}\u{fe0f} *\"You should\u{2019}ve gone for the head...\"*\n\u{1f4a8} **{} has been snapped out of existence!**", display_tag(target))).await?;
        }
        Err(e) if discord_code(&e) == Some(50013) => {
            msg.reply(&app.http, "\u{1f525} I can't snap that user. They're too powerful!").await?;
        }
        Err(e) => return Err(e.into()),
    }
    Ok(())
}

/// `-mkick @role reason`: kick every kickable member with a role, after a yes/no confirmation.
pub async fn text_mkick(app: &App, msg: &Message, args: &[String]) -> BotResult<()> {
    let Some(guild) = msg.guild_id else { return Ok(()) };
    if !need(app, msg, Permissions::ADMINISTRATOR, "\u{274c} This command can only be used by administrators.").await {
        return Ok(());
    }
    let Some(role) = msg.mention_roles.first().copied() else {
        msg.reply(&app.http, "Please mention a valid role to kick its members.").await?;
        return Ok(());
    };
    if args.len() < 2 {
        msg.reply(&app.http, "\u{274c} Error: A reason is required! Usage: -mkick @role [reason]").await?;
        return Ok(());
    }
    let reason = args[1..].join(" ");
    let (owner, roles) = app.guild_roles(guild).await?;
    let role_name = roles.get(&role).map(|r| r.name.clone()).unwrap_or_else(|| "that".into());

    // Fetch every member (1000 at a time).
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
    let targets: Vec<Member> = members.into_iter().filter(|m| m.roles.contains(&role) && m.user.id != owner && !m.user.bot).collect();
    if targets.is_empty() {
        msg.reply(&app.http, "No kickable members found with that role.").await?;
        return Ok(());
    }
    msg.reply(&app.http, format!("Are you sure you want to kick {} members with the {} role? Reply with `yes` to confirm or `no` to cancel.", targets.len(), role_name)).await?;

    // Wait up to 30s for yes/no from the same author.
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<Message>();
    let key = (msg.channel_id, msg.author.id);
    app.collectors.lock().unwrap().insert(key, tx);
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    let mut answer: Option<String> = None;
    loop {
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        match tokio::time::timeout(remaining, rx.recv()).await {
            Ok(Some(m)) => {
                let c = m.content.to_lowercase();
                if c == "yes" || c == "no" {
                    answer = Some(c);
                    break;
                }
            }
            _ => break,
        }
    }
    app.collectors.lock().unwrap().remove(&key);
    match answer.as_deref() {
        Some("yes") => {}
        Some(_) => {
            msg.reply(&app.http, "Operation cancelled.").await?;
            return Ok(());
        }
        None => {
            msg.reply(&app.http, "No confirmation received within 30 seconds. Operation cancelled.").await?;
            return Ok(());
        }
    }

    let server = guild_name(app, guild).await;
    let status = msg.reply(&app.http, "Processing kicks...").await?;
    let (mut kicked, mut failed) = (Vec::new(), Vec::new());
    for (i, member) in targets.iter().enumerate() {
        dm(app, &member.user, CreateMessage::new().embed(kick_dm_embed(&server, &reason))).await;
        match guild.kick_with_reason(&app.http, member.user.id, &reason).await {
            Ok(()) => kicked.push(display_tag(&member.user)),
            Err(e) => {
                tracing::warn!("mkick failed for {}: {e}", member.user.id);
                failed.push(display_tag(&member.user));
            }
        }
        if (i + 1) % 5 == 0 {
            let _ = msg.channel_id.edit_message(&app.http, status.id, EditMessage::new().content(format!("Processed {}/{} members...", i + 1, targets.len()))).await;
        }
        tokio::time::sleep(Duration::from_millis(800)).await;
    }
    let mut embed = CreateEmbed::new()
        .colour(Colour::new(0x00FF00))
        .title("Mass Kick Results")
        .description(format!("Completed kicking members with role: {role_name}"))
        .field("Successfully Kicked", if kicked.is_empty() { "None".to_string() } else { kicked.iter().take(20).cloned().collect::<Vec<_>>().join("\n") }, false)
        .timestamp(Timestamp::now());
    if !failed.is_empty() {
        embed = embed.field("Failed to Kick", failed.iter().take(20).cloned().collect::<Vec<_>>().join("\n"), false);
    }
    msg.channel_id.send_message(&app.http, CreateMessage::new().embed(embed)).await?;
    let _ = status.delete(&app.http).await;
    let _ = resolve_user;
    Ok(())
}
