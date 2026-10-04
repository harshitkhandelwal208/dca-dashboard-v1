//! `/tickets ...` (recruitment ticket management).

use crate::app::App;
use crate::managers::ban_panel::sync_recruitment_ban_list;
use crate::managers::recruitment::*;
use crate::opts;
use crate::responder::{ReplyData, Responder};
use crate::util::*;
use dca_state::config::update_config;
use dca_state::stores::*;
use serenity::all::*;

fn is_manager(r: &Responder) -> bool {
    r.permissions().contains(Permissions::ADMINISTRATOR) || r.permissions().contains(Permissions::MANAGE_GUILD)
}

async fn require_manager(r: &Responder) -> BotResult<bool> {
    if is_manager(r) {
        return Ok(true);
    }
    r.edit_reply(ReplyData::text("You need **Manage Server** to change ticket configuration.")).await?;
    Ok(false)
}

fn format_sync(sync: &PanelSync) -> String {
    if sync.skipped {
        return format!("Saved, but panel sync was skipped: {}", sync.reason);
    }
    format!("Recruitment panel {} in <#{}>.", if sync.created { "created" } else { "synced" }, sync.channel_id)
}

pub async fn slash_tickets(app: &App, r: &Responder, cmd: &CommandInteraction) -> BotResult<()> {
    let options = cmd.data.options();
    let Some((sub, o)) = opts::subcommand(&options) else { return Ok(()) };
    r.defer(true).await?;
    match sub {
        "setup" => {
            if !require_manager(r).await? {
                return Ok(());
            }
            let Some(channel) = opts::channel(o, "channel") else { return err("Missing channel.") };
            let log_channel = opts::channel(o, "log_channel").map(|c| c.id.to_string());
            let recruiter_role = opts::role(o, "recruiter_role").map(|r| r.id.to_string());
            let title = opts::string(o, "title").map(str::to_string);
            let description = opts::string(o, "description").map(str::to_string);
            let private = opts::boolean(o, "private_threads");
            let chan_id = channel.id.to_string();
            update_config(&app.store, |c| {
                c.recruitment.enabled = true;
                c.recruitment.panel_channel_id = chan_id.clone();
                if let Some(t) = &title {
                    c.recruitment.panel_title = t.clone();
                }
                if let Some(d) = &description {
                    c.recruitment.panel_description = d.clone();
                }
                if let Some(l) = &log_channel {
                    c.recruitment.log_channel_id = l.clone();
                }
                if let Some(rr) = &recruiter_role {
                    c.recruitment.recruiter_role_id = rr.clone();
                    c.bot.recruiter_role_id = rr.clone();
                }
                if let Some(p) = private {
                    c.recruitment.private_threads = p;
                }
            })
            .await?;
            let sync = ensure_recruitment_panel(app).await;
            r.edit_reply(ReplyData::text(format_sync(&sync))).await
        }
        "sync-panel" => {
            if !require_manager(r).await? {
                return Ok(());
            }
            let sync = ensure_recruitment_panel(app).await;
            r.edit_reply(ReplyData::text(format_sync(&sync))).await
        }
        "sync-banlist" => {
            if !require_manager(r).await? {
                return Ok(());
            }
            let sync = sync_recruitment_ban_list(app, false).await;
            r.edit_reply(ReplyData::text(if sync.skipped { format!("Skipped: {}", sync.reason) } else { format!("Recruitment ban list synced in <#{}> ({} users).", sync.channel_id, sync.count) }))
                .await
        }
        "status" => {
            let config = app.config().await;
            let open = list_tickets(&app.store, &TicketFilter { status: Some("open".into()), ..Default::default() }).await;
            let rec = &config.recruitment;
            let ch = |id: &str| if id.is_empty() { "not set".to_string() } else { format!("<#{id}>") };
            r.edit_reply(ReplyData::text(
                [
                    format!("Enabled: **{}**", if rec.enabled { "yes" } else { "no" }),
                    format!("Panel Channel: {}", ch(&rec.panel_channel_id)),
                    format!("Panel Message: {}", if rec.panel_message_id.is_empty() { "not set" } else { &rec.panel_message_id }),
                    format!("Image Log Channel: {}", ch(&rec.log_channel_id)),
                    format!("Combined Log Channel: {}", ch(&config.logging.channel_id)),
                    format!("Private Threads: **{}**", if rec.private_threads { "yes" } else { "no" }),
                    format!("Transcript On Close: **{}**", if rec.transcript_on_close { "yes" } else { "no" }),
                    "Close Behavior: **lock + archive**".to_string(),
                    format!("Recruiter Role: {}", if rec.recruiter_role_id.is_empty() { "not set".to_string() } else { format!("<@&{}>", rec.recruiter_role_id) }),
                    format!("Invite Channel: {}", if rec.invite_channel_id.is_empty() { "not set" } else { &rec.invite_channel_id }),
                    format!("Open Tickets: **{}**", open.len()),
                    format!("Member Count Auto-Update: **{}**", if config.member_counts.update_on_recruitment_close { "yes" } else { "no" }),
                ]
                .join("\n"),
            ))
            .await
        }
        "logs" => {
            let logs = list_recruitment_logs(&app.store, 10).await;
            if logs.is_empty() {
                return r.edit_reply(ReplyData::text("No recruitment logs yet.")).await;
            }
            let lines: Vec<String> = logs
                .iter()
                .map(|l| {
                    let s = |k: &str| l.get(k).and_then(|v| v.as_str()).unwrap_or("").to_string();
                    let outcome = if s("outcome") == "accepted" { s("team") } else { "Rejected".into() };
                    let when = chrono::DateTime::parse_from_rfc3339(&s("closedAt")).map(|d| d.format("%-m/%-d/%Y, %-I:%M:%S %p").to_string()).unwrap_or_else(|_| s("closedAt"));
                    format!("**{outcome}** - <@{}> closed by <@{}> on {when}", s("applicantId"), s("closedById"))
                })
                .collect();
            r.edit_reply(ReplyData::text(lines.join("\n"))).await
        }
        "claim" => {
            claim_ticket(app, r).await;
            Ok(())
        }
        "close" => {
            match opts::string(o, "outcome") {
                Some(outcome) => finish_close(app, r, outcome).await,
                None => start_close(app, r).await,
            }
            Ok(())
        }
        "add" => {
            if let Some(u) = opts::user(o, "user") {
                add_user_to_ticket(app, r, u.clone()).await;
            }
            Ok(())
        }
        "massadd" => {
            mass_add_users_to_ticket(app, r, opts::string(o, "users").unwrap_or("")).await;
            Ok(())
        }
        "remove" => {
            if let Some(u) = opts::user(o, "user") {
                remove_user_from_ticket(app, r, u.clone()).await;
            }
            Ok(())
        }
        "rename" => {
            rename_ticket(app, r, opts::string(o, "name").unwrap_or("")).await;
            Ok(())
        }
        "screenshot-list" => {
            list_ticket_screenshots(app, r, opts::string(o, "type").unwrap_or("")).await;
            Ok(())
        }
        "screenshot-add" => {
            let atts: Vec<&Attachment> = ["image", "image_2", "image_3", "image_4", "image_5"].iter().filter_map(|n| opts::attachment(o, n)).collect();
            add_ticket_screenshots(app, r, opts::string(o, "type").unwrap_or(""), atts).await;
            Ok(())
        }
        "screenshot-remove" => {
            remove_ticket_screenshot(app, r, opts::string(o, "type").unwrap_or(""), opts::integer(o, "index").unwrap_or(0)).await;
            Ok(())
        }
        "screenshot-change" => {
            if let Some(a) = opts::attachment(o, "image") {
                change_ticket_screenshot(app, r, opts::string(o, "type").unwrap_or(""), opts::integer(o, "index").unwrap_or(0), a).await;
            }
            Ok(())
        }
        "archive" => {
            archive_ticket(app, r).await;
            Ok(())
        }
        "delete" => {
            delete_ticket(app, r).await;
            Ok(())
        }
        _ => Ok(()),
    }
}
