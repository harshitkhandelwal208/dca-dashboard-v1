//! Welcome / leave messages and the welcome team-role buttons.

use crate::app::App;
use crate::logs::{log_action, LogEntry};
use crate::managers::team_roles::{community_guild_id, recruitment_guild_id};
use crate::responder::{ReplyData, Responder};
use crate::util::*;
use serde_json::json;
use serenity::all::*;

pub struct MemberInfo {
    pub id: UserId,
    pub username: String,
    pub display_name: String,
    pub tag: String,
    pub server: String,
    pub member_count: u64,
}

pub fn render_member_template(template: &str, m: &MemberInfo) -> String {
    template
        .replace("{member}", &format!("<@{}>", m.id))
        .replace("{username}", &m.username)
        .replace("{displayName}", &m.display_name)
        .replace("{tag}", &m.tag)
        .replace("{server}", &m.server)
        .replace("{memberCount}", &m.member_count.to_string())
}

async fn guild_info(app: &App, guild: GuildId) -> (String, u64) {
    match app.http.get_guild_with_counts(guild).await {
        Ok(g) => (g.name, g.approximate_member_count.unwrap_or(0)),
        Err(_) => (String::from("this server"), 0),
    }
}

pub async fn on_member_add(app: &App, member: &Member) {
    let config = app.config().await;
    super::team_roles::handle_community_member_join(app, member).await;

    let community = community_guild_id(&config);
    if !community.is_empty() && member.guild_id.to_string() != community {
        return;
    }
    let welcome = &config.welcome;
    if !welcome.enabled {
        return;
    }
    let Some(channel) = channel_id(&welcome.channel_id) else { return };
    let (server, count) = guild_info(app, member.guild_id).await;
    let info = MemberInfo {
        id: member.user.id,
        username: member.user.name.clone(),
        display_name: member.display_name().to_string(),
        tag: display_tag(&member.user),
        server,
        member_count: count,
    };
    let content = render_member_template(&welcome.message, &info);
    let mentions = CreateAllowedMentions::new().users(vec![member.user.id]);
    if let Err(error) = channel.send_message(&app.http, CreateMessage::new().content(content).allowed_mentions(mentions)).await {
        tracing::warn!("Error sending welcome message: {error}");
        return;
    }
    log_action(
        app,
        LogEntry::new("system", "Member Joined", format!("<@{}> joined the server.", member.user.id))
            .guild(member.guild_id)
            .target(member.user.id, display_tag(&member.user))
            .meta(json!({ "memberCount": count })),
    )
    .await;
}

pub async fn on_member_remove(app: &App, guild: GuildId, user: &User, member: Option<&Member>) {
    let config = app.config().await;
    let community = community_guild_id(&config);
    if !community.is_empty() && guild.to_string() != community {
        return;
    }
    let leave = &config.leave;
    if !leave.enabled {
        return;
    }
    let Some(channel) = channel_id(&leave.channel_id) else { return };
    let display_name = user.global_name.clone().unwrap_or_else(|| user.name.clone());
    let username = user.name.clone();
    let (server, count) = guild_info(app, guild).await;
    let info = MemberInfo {
        id: user.id,
        username: username.clone(),
        display_name: member.map(|m| m.display_name().to_string()).unwrap_or_else(|| display_name.clone()),
        tag: display_tag(user),
        server,
        member_count: count,
    };
    let mut message = render_member_template(&leave.message, &info);
    let mention = regex::Regex::new(&format!(r"<@!?{}>", user.id)).unwrap();
    message = mention.replace_all(&message, format!("**{display_name}** (@{username})").as_str()).to_string();
    if !message.contains(&display_name) && !message.contains(&format!("@{username}")) {
        message = format!("**{display_name}** (@{username}) {message}").trim().to_string();
    }
    if let Err(error) = channel.send_message(&app.http, CreateMessage::new().content(message).allowed_mentions(CreateAllowedMentions::new())).await {
        tracing::warn!("Error sending leave message: {error}");
        return;
    }
    log_action(
        app,
        LogEntry::new("system", "Member Left", format!("**{display_name}** (@{username}) left the server."))
            .guild(guild)
            .target(user.id, display_tag(user))
            .meta(json!({ "memberCount": count })),
    )
    .await;
}

const WELCOME_TEAM_PREFIX: &str = "welcome-team:";

/// Buttons a recruiter can press to hand a joined member their team role.
pub fn team_buttons_for_member(member: UserId, config: &dca_state::config::DashboardConfig) -> Vec<CreateActionRow> {
    let buttons: Vec<CreateButton> = config
        .member_counts
        .teams
        .iter()
        .filter(|t| !t.community_role_id.is_empty() || !t.role_id.is_empty())
        .take(25)
        .map(|t| CreateButton::new(format!("{WELCOME_TEAM_PREFIX}{member}:{}", t.id)).label(truncate(&t.name, 80)).style(ButtonStyle::Secondary))
        .collect();
    buttons.chunks(5).map(|c| CreateActionRow::Buttons(c.to_vec())).collect()
}

pub async fn handle_welcome_team_button(app: &App, r: &Responder, custom_id: &str) -> bool {
    if !custom_id.starts_with(WELCOME_TEAM_PREFIX) {
        return false;
    }
    let config = app.config().await;
    let community = community_guild_id(&config);
    let here = r.guild_id().map(|g| g.to_string()).unwrap_or_default();
    if !community.is_empty() && here != community {
        let _ = r.ephemeral("This welcome role button belongs to the configured community server.").await;
        return true;
    }
    let re = regex::Regex::new(r"^welcome-team:(\d{10,25}):(.+)$").unwrap();
    let Some(caps) = re.captures(custom_id) else {
        let _ = r.ephemeral("This welcome button is invalid.").await;
        return true;
    };
    let member_id = UserId::new(caps[1].parse().unwrap_or(0));
    let team_id = caps[2].to_string();

    // Only recruiters of the recruitment server may use these buttons.
    let role = if config.recruitment.recruiter_role_id.is_empty() { config.bot.recruiter_role_id.clone() } else { config.recruitment.recruiter_role_id.clone() };
    let allowed = match (role_id(&role), guild_id(&recruitment_guild_id(&config))) {
        (Some(role), Some(rg)) => match app.http.get_member(rg, r.user().id).await {
            Ok(m) => {
                m.roles.contains(&role) || {
                    let perms = app.member_permissions(rg, r.user().id).await;
                    perms.contains(Permissions::ADMINISTRATOR) || perms.contains(Permissions::MANAGE_GUILD)
                }
            }
            Err(_) => false,
        },
        _ => false,
    };
    if !allowed {
        let _ = r.ephemeral("Only recruitment-server recruiters can use these team role buttons.").await;
        return true;
    }
    let Some(team) = config.member_counts.teams.iter().find(|t| t.id == team_id) else {
        let _ = r.ephemeral("That team role is not configured anymore.").await;
        return true;
    };
    let role = if team.community_role_id.is_empty() { team.role_id.clone() } else { team.community_role_id.clone() };
    let Some(role) = role_id(&role) else {
        let _ = r.ephemeral("That team role is not configured anymore.").await;
        return true;
    };
    let Some(guild) = r.guild_id() else { return true };
    if app.http.get_member(guild, member_id).await.is_err() {
        let _ = r.ephemeral("That member is no longer in this server.").await;
        return true;
    }
    let reason = format!("Welcome team role assigned by {}", display_tag(r.user()));
    if let Err(error) = app.http.add_member_role(guild, member_id, role, Some(&reason)).await {
        let _ = r.ephemeral(format!("Could not assign the role: {error}")).await;
        return true;
    }
    log_action(
        app,
        LogEntry::new("system", "Welcome Team Role Assigned", format!("<@{}> assigned **{}** to <@{}>.", r.user().id, team.name, member_id))
            .guild(guild)
            .actor(r.user().id, display_tag(r.user()))
            .target(member_id, "")
            .meta(json!({ "teamId": team.id, "roleId": role.to_string() })),
    )
    .await;
    let _ = r.reply(ReplyData::text(format!("Assigned **{}** to <@{}>.", team.name, member_id)).ephemeral()).await;
    true
}
