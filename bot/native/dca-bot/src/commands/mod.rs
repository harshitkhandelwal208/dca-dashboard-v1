//! Slash and prefix (`-`) commands.

pub mod announce;
pub mod counts;
pub mod definitions;
pub mod info;
pub mod mastery;
pub mod moderation;
pub mod reminders;
pub mod roles;
pub mod spreadsheets;
pub mod tickets;

use crate::app::App;
use crate::responder::{Ix, ReplyData, Responder};
use serenity::all::*;
use std::sync::Arc;

pub const PREFIX: &str = "-";

/// Slash command entry point: run the handler; on failure tell the user (like the old `Slash error` handler).
pub async fn dispatch_slash(app: Arc<App>, cmd: CommandInteraction) {
    let name = cmd.data.name.clone();
    let r = Responder::new(app.http.clone(), Ix::Cmd(cmd.clone()));
    let result = match name.as_str() {
        "ban" => moderation::slash_ban(&app, &r, &cmd).await,
        "warn" => moderation::slash_warn(&app, &r, &cmd).await,
        "clearwarns" => moderation::slash_clearwarns(&app, &r, &cmd).await,
        "snap" => moderation::slash_snap(&app, &r, &cmd).await,
        "whois" => info::slash_whois(&app, &r, &cmd).await,
        "help" => info::slash_help(&app, &r).await,
        "dashboard" => info::slash_dashboard(&app, &r).await,
        "ping" => info::slash_ping(&app, &r, &cmd).await,
        "invite" => info::slash_invite(&app, &r).await,
        "yt" => info::slash_yt(&app, &r, &cmd).await,
        "membercount" => counts::slash_membercount(&app, &r, &cmd).await,
        "teamcount" => counts::slash_teamcount(&app, &r, &cmd).await,
        "updatecount" => counts::slash_updatecount(&app, &r, &cmd).await,
        "roles" => roles::slash_roles(&app, &r, &cmd).await,
        "remindme" => reminders::slash_remindme(&app, &r, &cmd).await,
        "reminders" => reminders::slash_reminders(&app, &r).await,
        "cancelreminder" => reminders::slash_cancel(&app, &r, &cmd).await,
        "top3te" => announce::slash_top3te(&app, &r, &cmd).await,
        "top3km" => announce::slash_top3km(&app, &r, &cmd).await,
        "teameventsummary" => announce::slash_summary(&app, &r, &cmd).await,
        "sportscar" => mastery::slash_sportscar(&app, &r).await,
        "garage" => mastery::slash_garage(&app, &r).await,
        "tickets" => tickets::slash_tickets(&app, &r, &cmd).await,
        "spreadsheets" => spreadsheets::slash_spreadsheets(&app, &r, &cmd).await,
        _ => return,
    };
    if let Err(error) = result {
        tracing::error!("Slash error {name}: {error}");
        let _ = r.reply(ReplyData::text("Error executing this command.").ephemeral()).await;
    }
}

/// `-command args` messages.
pub async fn dispatch_text(app: Arc<App>, message: Message) {
    let Some(rest) = message.content.strip_prefix(PREFIX) else { return };
    if message.author.bot {
        return;
    }
    let mut args = crate::util::split_args(rest.trim());
    if args.is_empty() {
        return;
    }
    let name = args.remove(0).to_lowercase();
    let result = match name.as_str() {
        "announce" => info::text_announce(&app, &message).await,
        "bam" => info::text_bam(&app, &message).await,
        "ban" => moderation::text_ban(&app, &message, &args).await,
        "clean" => moderation::text_clean(&app, &message, &args).await,
        "clearwarns" => moderation::text_clearwarns(&app, &message).await,
        "help" => info::text_help(&app, &message).await,
        "kick" => moderation::text_kick(&app, &message, &args).await,
        "team" => info::text_team(&app, &message).await,
        "mkick" => moderation::text_mkick(&app, &message, &args).await,
        "mute" => moderation::text_mute(&app, &message, &args).await,
        "ping" => info::text_ping(&app, &message).await,
        "pingmessage" => info::text_pingmessage(&app, &message).await,
        "snapban" => moderation::text_snapban(&app, &message, &args).await,
        "temperature" | "temp" | "weather" => info::text_temperature(&app, &message, &args).await,
        "unban" => moderation::text_unban(&app, &message, &args).await,
        "warn" => moderation::text_warn(&app, &message, &args).await,
        "warnings" => moderation::text_warnings(&app, &message).await,
        "whois" => info::text_whois(&app, &message).await,
        "yt" => info::text_yt(&app, &message, &args).await,
        _ => return,
    };
    if let Err(error) = result {
        tracing::error!("Error executing command {name}: {error}");
        let _ = message.reply(&app.http, "\u{274c} Error executing this command.").await;
    }
}

/// Descriptions of the prefix commands (for `-help`).
pub const TEXT_COMMANDS: &[(&str, &str)] = &[
    ("announce", "Send an update announcement"),
    ("bam", "No description"),
    ("ban", "Bans a user by mention or ID."),
    ("clean", "Deletes a specified number of messages. Usage: clean <amount> [@user] [-nopin]"),
    ("clearwarns", "Clears all warnings for a user."),
    ("help", "Shows current bot commands and report workflows"),
    ("kick", "Kicks a user from the server"),
    ("mkick", "Kicks all members with a specific role. Usage: -mkick @role [reason]"),
    ("mute", "Mutes a user for a specified duration (using Discord's timeout feature)."),
    ("ping", "Check that the bot is alive and how fast it answers."),
    ("pingmessage", "Sends a specific ping message in the channel while preventing duplicates."),
    ("snapban", "Thanos-style ban someone out of existence"),
    ("team", "Show team member list"),
    ("temperature", "Get the current temperature of a city"),
    ("unban", "Unbans a user from the server using their ID or username"),
    ("warn", "Warns a user and records it in the database."),
    ("warnings", "Displays all warnings for a user."),
    ("whois", "Displays information about a user."),
    ("yt", "No description"),
];
