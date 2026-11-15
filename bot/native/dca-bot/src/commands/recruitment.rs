//! Recruitment server commands: >recruiter and >unrecruiter role assignment.

use crate::app::App;
use crate::commands::moderation::is_recruitment_ban_server;
use crate::util::*;
use serenity::all::*;

pub const ALLOWED_ROLE_ID: RoleId = RoleId::new(922169747305558036);
pub const RECRUITER_ROLE_ID: RoleId = RoleId::new(1082759575087882271);

async fn check_permission(app: &App, message: &Message, guild_id: GuildId) -> bool {
    if !is_recruitment_ban_server(app, guild_id).await {
        let _ = message.reply(&app.http, "\u{274c} This command can only be used in the recruitment server.").await;
        return false;
    }
    let has_role = if let Some(m) = &message.member {
        m.roles.contains(&ALLOWED_ROLE_ID)
    } else {
        match app.http.get_member(guild_id, message.author.id).await {
            Ok(m) => m.roles.contains(&ALLOWED_ROLE_ID),
            Err(_) => false,
        }
    };
    if !has_role {
        let _ = message.reply(&app.http, "\u{274c} You do not have permission to use this command.").await;
        return false;
    }
    true
}

async fn resolve_target_user(app: &App, guild_id: GuildId, message: &Message, args: &[String]) -> Option<UserId> {
    if let Some(user) = message.mentions.first() {
        return Some(user.id);
    }
    let arg = args.first()?;
    if let Some(id) = user_id(arg) {
        return Some(id);
    }
    let query = arg.trim().trim_start_matches('@').to_lowercase();
    if query.is_empty() {
        return None;
    }
    if let Ok(members) = app.http.search_guild_members(guild_id, &query, Some(10)).await {
        for m in members {
            let nick_match = m.nick.as_ref().is_some_and(|n| n.to_lowercase().contains(&query));
            let name_match = m.user.name.to_lowercase().contains(&query);
            let global_match = m.user.global_name.as_ref().is_some_and(|g| g.to_lowercase().contains(&query));
            if nick_match || name_match || global_match {
                return Some(m.user.id);
            }
        }
    }
    if let Ok(members) = app.http.get_guild_members(guild_id, Some(1000), None).await {
        for m in members {
            let nick_match = m.nick.as_ref().is_some_and(|n| n.to_lowercase() == query);
            let name_match = m.user.name.to_lowercase() == query;
            let global_match = m.user.global_name.as_ref().is_some_and(|g| g.to_lowercase() == query);
            if nick_match || name_match || global_match {
                return Some(m.user.id);
            }
        }
    }
    None
}

pub async fn text_recruiter(app: &App, message: &Message, args: &[String]) -> BotResult<()> {
    let Some(guild_id) = message.guild_id else { return Ok(()) };
    if !check_permission(app, message, guild_id).await {
        return Ok(());
    }
    let target = resolve_target_user(app, guild_id, message, args).await;
    let Some(target_id) = target else {
        message.reply(&app.http, "\u{274c} Please provide a valid user mention or nickname. Usage: `>recruiter @nickname`").await?;
        return Ok(());
    };
    match app.http.add_member_role(guild_id, target_id, RECRUITER_ROLE_ID, Some("Assigned via >recruiter")).await {
        Ok(()) => {
            message.channel_id.say(&app.http, format!("\u{2705} Assigned role <@&{RECRUITER_ROLE_ID}> to <@{target_id}>.")).await?;
        }
        Err(e) => {
            tracing::warn!("add_member_role failed: {e}");
            message.reply(&app.http, format!("\u{274c} Failed to assign role: {e}")).await?;
        }
    }
    Ok(())
}

pub async fn text_unrecruiter(app: &App, message: &Message, args: &[String]) -> BotResult<()> {
    let Some(guild_id) = message.guild_id else { return Ok(()) };
    if !check_permission(app, message, guild_id).await {
        return Ok(());
    }
    let target = resolve_target_user(app, guild_id, message, args).await;
    let Some(target_id) = target else {
        message.reply(&app.http, "\u{274c} Please provide a valid user mention or nickname. Usage: `>unrecruiter @nickname`").await?;
        return Ok(());
    };
    match app.http.remove_member_role(guild_id, target_id, RECRUITER_ROLE_ID, Some("Removed via >unrecruiter")).await {
        Ok(()) => {
            message.channel_id.say(&app.http, format!("\u{2705} Removed role <@&{RECRUITER_ROLE_ID}> from <@{target_id}>.")).await?;
        }
        Err(e) => {
            tracing::warn!("remove_member_role failed: {e}");
            message.reply(&app.http, format!("\u{274c} Failed to remove role: {e}")).await?;
        }
    }
    Ok(())
}
