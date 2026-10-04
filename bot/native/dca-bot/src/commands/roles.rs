//! `/roles add|remove` (Manage Roles).

use crate::app::App;
use crate::opts;
use crate::responder::{ReplyData, Responder};
use crate::util::*;
use serenity::all::*;

pub async fn slash_roles(app: &App, r: &Responder, cmd: &CommandInteraction) -> BotResult<()> {
    let Some(guild) = cmd.guild_id else { return err("Use this command in a server.") };
    if !r.permissions().contains(Permissions::MANAGE_ROLES) && !r.permissions().contains(Permissions::ADMINISTRATOR) {
        return r.reply(ReplyData::text("\u{274c} You don't have permission to use this command.").ephemeral()).await;
    }
    let me = app.ensure_bot_id().await?;
    if !app.member_has(guild, me, Permissions::MANAGE_ROLES).await {
        return r.reply(ReplyData::text("\u{274c} I need the 'Manage Roles' permission to perform this action.").ephemeral()).await;
    }
    let options = cmd.data.options();
    let Some((sub, sub_opts)) = opts::subcommand(&options) else { return Ok(()) };
    let Some(target) = opts::user(sub_opts, "user") else { return err("Missing user.") };
    let Some(role) = opts::role(sub_opts, "role") else { return err("Missing role.") };
    let Ok(member) = app.http.get_member(guild, target.id).await else {
        return r.reply(ReplyData::text("User not found.").ephemeral()).await;
    };
    let has = member.roles.contains(&role.id);
    if sub == "add" {
        if has {
            return r.reply(ReplyData::text(format!("<@{}> already has the <@&{}> role.", target.id, role.id)).ephemeral()).await;
        }
        app.http.add_member_role(guild, target.id, role.id, None).await?;
        return r.reply(ReplyData::text(format!("\u{2705} Added <@&{}> to <@{}>.", role.id, target.id)).ephemeral()).await;
    }
    if !has {
        return r.reply(ReplyData::text(format!("<@{}> does not have the <@&{}> role.", target.id, role.id)).ephemeral()).await;
    }
    app.http.remove_member_role(guild, target.id, role.id, None).await?;
    r.reply(ReplyData::text(format!("\u{2705} Removed <@&{}> from <@{}>.", role.id, target.id)).ephemeral()).await
}
