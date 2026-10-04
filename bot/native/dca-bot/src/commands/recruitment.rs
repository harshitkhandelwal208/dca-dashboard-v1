//! Recruitment ticket commands: /ticket panel, list, claim, close.

use crate::{Context, Error};
use dca_state::config::{load_dashboard_config, save_dashboard_config};
use dca_state::log_store::LogStore;
use dca_state::recruitment_store::{RecruitmentStore, Ticket};
use poise::serenity_prelude as serenity;

#[poise::command(
    slash_command,
    guild_only,
    subcommands("panel", "list", "claim", "close"),
    subcommand_required
)]
pub async fn ticket(_ctx: Context<'_>) -> Result<(), Error> {
    Ok(())
}

/// Deploy the recruitment Apply panel in the current channel.
#[poise::command(slash_command, guild_only, required_permissions = "MANAGE_GUILD")]
pub async fn panel(ctx: Context<'_>) -> Result<(), Error> {
    let mut config = load_dashboard_config();
    let channel_id = ctx.channel_id();

    let embed = serenity::CreateEmbed::new()
        .title(&config.recruitment.panel_title)
        .description(&config.recruitment.panel_description)
        .color(0x0f766e)
        .timestamp(serenity::Timestamp::now());

    let apply_btn = serenity::CreateButton::new("recruitment:apply")
        .label("Apply!")
        .style(serenity::ButtonStyle::Primary);

    let row = serenity::CreateActionRow::Buttons(vec![apply_btn]);

    let msg = channel_id
        .send_message(ctx.http(), serenity::CreateMessage::new().embed(embed).components(vec![row]))
        .await?;

    config.recruitment.panel_channel_id = channel_id.to_string();
    config.recruitment.panel_message_id = msg.id.to_string();
    save_dashboard_config(&config)?;

    ctx.say("Recruitment panel posted successfully.").await?;
    Ok(())
}

/// List open recruitment tickets.
#[poise::command(slash_command, guild_only)]
pub async fn list(ctx: Context<'_>) -> Result<(), Error> {
    let tickets = RecruitmentStore::list_tickets();
    let open_tickets: Vec<&Ticket> = tickets
        .iter()
        .filter(|t| t.status == "open" || t.status == "claimed")
        .collect();

    if open_tickets.is_empty() {
        ctx.say("No open recruitment tickets at this time.").await?;
        return Ok(());
    }

    let mut out = format!("**Open Recruitment Tickets ({}):**\n", open_tickets.len());
    for t in open_tickets {
        let thread_link = t.thread_id.as_deref().map(|id| format!("<#{id}>")).unwrap_or_else(|| "No thread".to_string());
        let claim_info = t.claimed_by_tag.as_deref().map(|tag| format!(" (claimed by {tag})")).unwrap_or_default();
        out.push_str(&format!("• `{}`: **{}** - {}{}\n", t.id, t.applicant_tag, thread_link, claim_info));
    }

    ctx.say(out).await?;
    Ok(())
}

/// Claim a ticket for review.
#[poise::command(slash_command, guild_only)]
pub async fn claim(
    ctx: Context<'_>,
    #[description = "Ticket ID to claim"] id: String,
) -> Result<(), Error> {
    let author_tag = ctx.author().tag();
    let author_id = ctx.author().id.to_string();

    let updated = RecruitmentStore::update_ticket(&id, |t| {
        t.status = "claimed".to_string();
        t.claimed_by_id = Some(author_id.clone());
        t.claimed_by_tag = Some(author_tag.clone());
    })?;

    if let Some(t) = updated {
        LogStore::log_action("claim_ticket", &author_id, &author_tag, &format!("Claimed ticket {}", t.id));
        ctx.say(format!("Ticket `{}` claimed by **{}**.", t.id, author_tag)).await?;
    } else {
        ctx.say(format!("Could not find ticket `{}`.", id)).await?;
    }

    Ok(())
}

/// Close a recruitment ticket with a team assignment.
#[poise::command(slash_command, guild_only)]
pub async fn close(
    ctx: Context<'_>,
    #[description = "Ticket ID to close"] id: String,
    #[description = "Assigned team (optional)"] team: Option<String>,
) -> Result<(), Error> {
    let author_tag = ctx.author().tag();
    let author_id = ctx.author().id.to_string();

    let updated = RecruitmentStore::update_ticket(&id, |t| {
        t.status = "closed".to_string();
        t.assigned_team = team.clone();
        t.closed_at = Some(chrono::Utc::now().to_rfc3339());
    })?;

    if let Some(t) = updated {
        let team_msg = team.as_deref().map(|m| format!(" and assigned to **{m}**")).unwrap_or_default();
        LogStore::log_action("close_ticket", &author_id, &author_tag, &format!("Closed ticket {}{}", t.id, team_msg));
        ctx.say(format!("Ticket `{}` closed{}", t.id, team_msg)).await?;
    } else {
        ctx.say(format!("Could not find ticket `{}`.", id)).await?;
    }

    Ok(())
}
