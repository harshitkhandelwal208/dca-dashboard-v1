//! Recruitment server commands: >recruiter and >unrecruiter role assignment.

use crate::app::App;
use crate::commands::moderation::is_recruitment_ban_server;
use crate::util::*;
use serenity::all::*;

pub const ALLOWED_ROLE_ID: RoleId = RoleId::new(922169747305558036);
pub const RECRUITER_ROLE_ID: RoleId = RoleId::new(1082759575087882271);

#[derive(Clone, Copy)]
pub struct RoleCommand {
    pub name: &'static str,
    pub permission_role: RoleId,
    pub target_role: RoleId,
    pub add: bool,
}

pub const DISCORD2_COMMAND: RoleCommand = RoleCommand { name: "discord2", permission_role: RoleId::new(1570423807951249520), target_role: RoleId::new(1354925755728920576), add: true };
pub const UNDISCORD2_COMMAND: RoleCommand = RoleCommand { name: "undiscord2", add: false, ..DISCORD2_COMMAND };
pub const DISCORD3_COMMAND: RoleCommand = RoleCommand { name: "discord3", permission_role: RoleId::new(1570423850037158039), target_role: RoleId::new(1570423869079027732), add: true };
pub const UNDISCORD3_COMMAND: RoleCommand = RoleCommand { name: "undiscord3", add: false, ..DISCORD3_COMMAND };
pub const DCA_COMMAND: RoleCommand = RoleCommand { name: "dca", permission_role: RoleId::new(712446912946503802), target_role: RoleId::new(724761301129101374), add: true };
pub const UNDCA_COMMAND: RoleCommand = RoleCommand { name: "undca", add: false, ..DCA_COMMAND };

async fn check_permission(app: &App, message: &Message, guild_id: GuildId, permission_role: RoleId) -> bool {
    if !is_recruitment_ban_server(app, guild_id).await {
        let _ = message.reply(&app.http, "\u{274c} This command can only be used in the recruitment server.").await;
        return false;
    }
    let has_role = if let Some(m) = &message.member {
        m.roles.contains(&permission_role)
    } else {
        match app.http.get_member(guild_id, message.author.id).await {
            Ok(m) => m.roles.contains(&permission_role),
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
    if !check_permission(app, message, guild_id, ALLOWED_ROLE_ID).await {
        return Ok(());
    }
    let target = resolve_target_user(app, guild_id, message, args).await;
    let Some(target_id) = target else {
        message.reply(&app.http, "\u{274c} Please provide a valid user mention or nickname. Usage: `>recruiter @nickname`").await?;
        return Ok(());
    };
    match app.http.add_member_role(guild_id, target_id, RECRUITER_ROLE_ID, Some("Assigned via >recruiter")).await {
        Ok(()) => {
            message
                .channel_id
                .send_message(&app.http, CreateMessage::new().content(format!("\u{2705} Assigned role {RECRUITER_ROLE_ID} to user {target_id}.")).allowed_mentions(CreateAllowedMentions::new()))
                .await?;
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
    if !check_permission(app, message, guild_id, ALLOWED_ROLE_ID).await {
        return Ok(());
    }
    let target = resolve_target_user(app, guild_id, message, args).await;
    let Some(target_id) = target else {
        message.reply(&app.http, "\u{274c} Please provide a valid user mention or nickname. Usage: `>unrecruiter @nickname`").await?;
        return Ok(());
    };
    match app.http.remove_member_role(guild_id, target_id, RECRUITER_ROLE_ID, Some("Removed via >unrecruiter")).await {
        Ok(()) => {
            message
                .channel_id
                .send_message(&app.http, CreateMessage::new().content(format!("\u{2705} Removed role {RECRUITER_ROLE_ID} from user {target_id}.")).allowed_mentions(CreateAllowedMentions::new()))
                .await?;
        }
        Err(e) => {
            tracing::warn!("remove_member_role failed: {e}");
            message.reply(&app.http, format!("\u{274c} Failed to remove role: {e}")).await?;
        }
    }
    Ok(())
}

/// Assign or remove one of the recruitment-only team roles without emitting any Discord mentions.
pub async fn text_role_command(app: &App, message: &Message, args: &[String], command: RoleCommand) -> BotResult<()> {
    let Some(guild_id) = message.guild_id else { return Ok(()) };
    if !check_permission(app, message, guild_id, command.permission_role).await {
        return Ok(());
    }
    let Some(target_id) = resolve_target_user(app, guild_id, message, args).await else {
        message.reply(&app.http, format!("\u{274c} Please provide a valid user mention or nickname. Usage: `>{} @nickname`", command.name)).await?;
        return Ok(());
    };
    let result = if command.add {
        app.http.add_member_role(guild_id, target_id, command.target_role, Some(&format!("Assigned via >{}", command.name))).await
    } else {
        app.http.remove_member_role(guild_id, target_id, command.target_role, Some(&format!("Removed via >{}", command.name))).await
    };
    match result {
        Ok(()) => {
            let verb = if command.add { "Assigned" } else { "Removed" };
            let direction = if command.add { "to" } else { "from" };
            message
                .channel_id
                .send_message(
                    &app.http,
                    CreateMessage::new().content(format!("\u{2705} {verb} role {} {direction} user {}.", command.target_role, target_id)).allowed_mentions(CreateAllowedMentions::new()),
                )
                .await?;
        }
        Err(e) => {
            tracing::warn!("{} role operation failed: {e}", command.name);
            message.reply(&app.http, format!("\u{274c} Failed to update role: {e}")).await?;
        }
    }
    Ok(())
}
