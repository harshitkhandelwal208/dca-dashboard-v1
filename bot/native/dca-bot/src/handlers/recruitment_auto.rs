//! Recruitment helper functions: GP team qualification and recruiter decision execution.

use dca_state::config::DashboardConfig;
use dca_state::log_store::LogStore;
use dca_state::recruitment_store::RecruitmentStore;
use poise::serenity_prelude as serenity;

/// Determines the best team matching the driver's Garage Power (GP).
pub fn determine_team_for_gp(gp: u32, preferred_team: Option<&str>, config: &DashboardConfig) -> String {
    if let Some(pref) = preferred_team {
        let trimmed = pref.trim();
        if !trimmed.is_empty() {
            if let Some(matched) = config.recruitment.teams.iter().find(|t| t.eq_ignore_ascii_case(trimmed)) {
                return matched.clone();
            }
        }
    }

    if gp >= 8000 {
        "Discord".to_string()
    } else if gp >= 6500 {
        "Discord²".to_string()
    } else if gp >= 5000 {
        "Discord 3™".to_string()
    } else {
        "Nascar DC".to_string()
    }
}

/// Executes a human recruiter's decision:
/// - If a team is assigned: updates ticket, assigns Discord team role, updates nickname to [DCA] name, logs action, and posts welcome embed.
/// - If rejected: updates ticket to closed/rejected, logs action, and posts rejection notice.
pub async fn execute_recruiter_decision(
    http: &serenity::Http,
    guild_id: Option<serenity::GuildId>,
    recruiter_id: serenity::UserId,
    recruiter_tag: &str,
    ticket_id: &str,
    outcome: &str,
    target_channel_id: serenity::ChannelId,
) {
    let ticket = RecruitmentStore::get_ticket(ticket_id);
    let Some(ticket) = ticket else {
        return;
    };

    let applicant_user_id: u64 = ticket.applicant_id.parse().unwrap_or(0);
    let applicant_id = serenity::UserId::new(applicant_user_id);

    // Update ticket in store
    let _ = RecruitmentStore::update_ticket(ticket_id, |t| {
        t.status = "closed".to_string();
        t.assigned_team = Some(outcome.to_string());
        t.closed_at = Some(chrono::Utc::now().to_rfc3339());
    });

    if outcome.eq_ignore_ascii_case("Rejected") {
        LogStore::log_action(
            "recruitment_rejected",
            &recruiter_id.to_string(),
            recruiter_tag,
            &format!("Rejected applicant {} ({}) for ticket #{}", ticket.applicant_tag, ticket.applicant_id, ticket_id),
        );

        let embed = serenity::CreateEmbed::new()
            .title("Recruitment Ticket Closed")
            .description(format!(
                "Ticket **#{}** for <@{}> has been closed by <@{}>.\n\
                *Outcome:* Application not accepted at this time.",
                ticket_id, ticket.applicant_id, recruiter_id
            ))
            .color(0xef4444)
            .timestamp(serenity::Timestamp::now());

        let _ = target_channel_id.send_message(http, serenity::CreateMessage::new().embed(embed)).await;
        return;
    }

    // Team assignment outcome
    let mut role_assigned = false;
    let mut assigned_role_id = None;
    let mut nickname_set = false;

    // Extract in-game player name if parsed
    let player_name = ticket.license_analysis.as_ref()
        .and_then(|v| v.get("player_name"))
        .and_then(|n| n.as_str())
        .unwrap_or("");

    if let Some(gid) = guild_id {
        if let Ok(mut member) = gid.member(http, applicant_id).await {
            // Assign team role
            if let Ok(roles) = gid.roles(http).await {
                if let Some(role) = roles.values().find(|r| r.name.eq_ignore_ascii_case(outcome)) {
                    if member.add_role(http, role.id).await.is_ok() {
                        role_assigned = true;
                        assigned_role_id = Some(role.id);
                    }
                }
            }

            // Update nickname to [DCA] Name
            if !player_name.is_empty() && player_name != "Applicant" {
                let clean_name = player_name.chars().take(24).collect::<String>();
                let new_nick = format!("[DCA] {}", clean_name);
                if member.edit(http, serenity::EditMember::new().nickname(new_nick)).await.is_ok() {
                    nickname_set = true;
                }
            }
        }
    }

    LogStore::log_action(
        "recruitment_approved",
        &recruiter_id.to_string(),
        recruiter_tag,
        &format!("Recruiter {} placed {} ({}) into {} for ticket #{}", recruiter_tag, ticket.applicant_tag, ticket.applicant_id, outcome, ticket_id),
    );

    let role_mention = assigned_role_id
        .map(|r| format!("<@&{}>", r))
        .unwrap_or_else(|| format!("**{}**", outcome));

    let mut actions = Vec::new();
    if role_assigned {
        actions.push(format!("• Assigned Role: {}", role_mention));
    }
    if nickname_set {
        actions.push(format!("• Updated Server Nickname: `[DCA] {}`", player_name));
    }
    actions.push(format!("• Ticket **#{}** closed.", ticket_id));

    let embed = serenity::CreateEmbed::new()
        .title("Recruitment Approved")
        .description(format!(
            "Welcome to **DCA**, <@{}>! Recruiter <@{}> has approved your application and assigned you to **{}**.\n\n\
            {}\n\n\
            *Head over to your team channels and check the pinned announcements!*",
            ticket.applicant_id, recruiter_id, outcome, actions.join("\n")
        ))
        .color(0x0f766e)
        .timestamp(serenity::Timestamp::now());

    let _ = target_channel_id.send_message(http, serenity::CreateMessage::new().embed(embed)).await;
}
