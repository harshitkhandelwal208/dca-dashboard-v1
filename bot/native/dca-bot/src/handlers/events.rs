//! Serenity event handler for messages, interactions, buttons, and modals.

use crate::handlers::intake::{handle_potential_spreadsheet_image, handle_recruitment_image_upload};
use crate::Data;
use dca_state::config::load_dashboard_config;
use dca_state::recruitment_store::{RecruitmentStore, Ticket};
use dca_state::spreadsheet_store::SpreadsheetStore;
use poise::serenity_prelude as serenity;

pub async fn handle_event(
    event: &serenity::FullEvent,
    framework: poise::FrameworkContext<'_, Data, crate::Error>,
) -> Result<(), crate::Error> {
    let ctx = framework.serenity_context;
    let data = framework.user_data().await;
    match event {
        serenity::FullEvent::Message { new_message } => {
            if new_message.author.bot {
                return Ok(());
            }

            let config = data.config.read().unwrap().clone();

            // Check recruitment intake
            if handle_recruitment_image_upload(ctx, new_message, data.ocr.clone()).await {
                return Ok(());
            }

            // Check spreadsheet intake
            if handle_potential_spreadsheet_image(ctx, new_message, &config).await {
                return Ok(());
            }
        }

        serenity::FullEvent::InteractionCreate { interaction } => {
            match interaction {
                serenity::Interaction::Component(component) => {
                    let custom_id = &component.data.custom_id;

                    if custom_id == "recruitment:apply" {
                        let content = "**Welcome to DCA Recruitment**\n\n\
                            Please upload your Hill Climb Racing 2 driver profile screenshot directly in this channel.\n\
                            Our native vision engine will verify it instantly (<5ms) and open your recruitment ticket without keeping you waiting!";
                        let response = serenity::CreateInteractionResponse::Message(
                            serenity::CreateInteractionResponseMessage::new()
                                .content(content)
                                .ephemeral(true),
                        );
                        let _ = component.create_response(ctx, response).await;
                        return Ok(());
                    }

                    if custom_id.starts_with("recruitment:manual:") {
                        let name_input = serenity::CreateInputText::new(
                            serenity::InputTextStyle::Short,
                            "In-game Name",
                            "name",
                        ).placeholder("Your exact Hill Climb Racing 2 name").required(true);

                        let gp_input = serenity::CreateInputText::new(
                            serenity::InputTextStyle::Short,
                            "Garage Power (GP)",
                            "gp",
                        ).placeholder("e.g. 7500").required(true);

                        let team_input = serenity::CreateInputText::new(
                            serenity::InputTextStyle::Short,
                            "Preferred Team / Notes",
                            "team",
                        ).placeholder("e.g. Discord, Discord², etc.").required(false);

                        let modal = serenity::CreateModal::new("recruitment:modal_submit", "Submit DCA Application")
                            .components(vec![
                                serenity::CreateActionRow::InputText(name_input),
                                serenity::CreateActionRow::InputText(gp_input),
                                serenity::CreateActionRow::InputText(team_input),
                            ]);

                        let _ = component.create_response(ctx, serenity::CreateInteractionResponse::Modal(modal)).await;
                        return Ok(());
                    }

                    if custom_id.starts_with("recruitment:staff_help:") {
                        let ticket_id = uuid::Uuid::new_v4().to_string().chars().take(8).collect::<String>();
                        let ticket = Ticket {
                            id: ticket_id.clone(),
                            applicant_id: component.user.id.to_string(),
                            applicant_tag: component.user.tag(),
                            status: "open".to_string(),
                            thread_id: None,
                            channel_id: Some(component.channel_id.to_string()),
                            claimed_by_id: None,
                            claimed_by_tag: None,
                            assigned_team: None,
                            created_at: chrono::Utc::now().to_rfc3339(),
                            closed_at: None,
                            license_analysis: None,
                            license_attachments: Vec::new(),
                            event_attachments: Vec::new(),
                            screenshot_check: Some(serde_json::json!({ "staff_help_requested": true })),
                            manual_entry: true,
                        };

                        let _ = RecruitmentStore::save_ticket(ticket);

                        let response = serenity::CreateInteractionResponse::Message(
                            serenity::CreateInteractionResponseMessage::new()
                                .content(format!("Ticket **#{}** opened. A recruiter has been notified to help you shortly.", ticket_id))
                                .ephemeral(true),
                        );
                        let _ = component.create_response(ctx, response).await;
                        return Ok(());
                    }

                    if custom_id.starts_with("spreadsheet:go:") {
                        let session_id = custom_id.trim_start_matches("spreadsheet:go:").to_string();
                        let http = ctx.http.clone();
                        let ocr = data.ocr.clone();
                        let sid = session_id.clone();
                        tokio::spawn(async move {
                            crate::schedulers::spreadsheet_job::process_spreadsheet_session(http, ocr, &sid).await;
                        });

                        let response = serenity::CreateInteractionResponse::Message(
                            serenity::CreateInteractionResponseMessage::new()
                                .content(format!("Building spreadsheet for session `{}` now...", session_id))
                                .ephemeral(true),
                        );
                        let _ = component.create_response(ctx, response).await;
                        return Ok(());
                    }

                    if custom_id.starts_with("recruitment:guide:") {
                        let content = "**How to find your Hill Climb Racing 2 Driver Profile:**\n\n\
                            1. Open **Hill Climb Racing 2**.\n\
                            2. From the main menu, tap your **Driver Avatar / Name** at the top-left.\n\
                            3. Verify your **Driver Name**, **Garage Power (GP)**, and stats are shown.\n\
                            4. Take a screenshot and upload it in the recruitment channel!\n\n\
                            *(If you have difficulty, you can always click 'Enter Details Manually')*";
                        let response = serenity::CreateInteractionResponse::Message(
                            serenity::CreateInteractionResponseMessage::new()
                                .content(content)
                                .ephemeral(true),
                        );
                        let _ = component.create_response(ctx, response).await;
                        return Ok(());
                    }

                    if custom_id.starts_with("ticket:claim:") {
                        let ticket_id = custom_id.trim_start_matches("ticket:claim:");
                        let _ = RecruitmentStore::update_ticket(ticket_id, |t| {
                            t.claimed_by_id = Some(component.user.id.to_string());
                            t.claimed_by_tag = Some(component.user.tag());
                        });

                        let response = serenity::CreateInteractionResponse::Message(
                            serenity::CreateInteractionResponseMessage::new()
                                .content(format!("Ticket **#{}** claimed by <@{}>.", ticket_id, component.user.id)),
                        );
                        let _ = component.create_response(ctx, response).await;
                        return Ok(());
                    }

                    if custom_id.starts_with("ticket:close:") {
                        let ticket_id = custom_id.trim_start_matches("ticket:close:");
                        let config = load_dashboard_config();
                        let mut buttons = Vec::new();

                        for team in config.recruitment.teams.iter().take(4) {
                            buttons.push(serenity::CreateButton::new(format!("ticket:assign:{}:{}", ticket_id, team))
                                .label(team)
                                .style(serenity::ButtonStyle::Success));
                        }

                        buttons.push(serenity::CreateButton::new(format!("ticket:assign:{}:Rejected", ticket_id))
                            .label("Reject")
                            .style(serenity::ButtonStyle::Danger));

                        let row = serenity::CreateActionRow::Buttons(buttons);
                        let response = serenity::CreateInteractionResponse::Message(
                            serenity::CreateInteractionResponseMessage::new()
                                .content("Select team assignment or outcome for this ticket:")
                                .components(vec![row])
                                .ephemeral(true),
                        );
                        let _ = component.create_response(ctx, response).await;
                        return Ok(());
                    }

                    if custom_id.starts_with("ticket:assign:") {
                        let parts: Vec<&str> = custom_id.split(':').collect();
                        if parts.len() >= 4 {
                            let ticket_id = parts[2];
                            let outcome = parts[3..].join(":");

                            let response = serenity::CreateInteractionResponse::Message(
                                serenity::CreateInteractionResponseMessage::new()
                                    .content(format!("⏳ Finalizing ticket **#{}** with outcome **{}**...", ticket_id, outcome))
                                    .ephemeral(true),
                            );
                            let _ = component.create_response(ctx, response).await;

                            let http = ctx.http.clone();
                            let guild_id = component.guild_id;
                            let recruiter_id = component.user.id;
                            let recruiter_tag = component.user.tag();
                            let tid = ticket_id.to_string();
                            let target_ch = component.channel_id;

                            tokio::spawn(async move {
                                crate::handlers::recruitment_auto::execute_recruiter_decision(
                                    &http,
                                    guild_id,
                                    recruiter_id,
                                    &recruiter_tag,
                                    &tid,
                                    &outcome,
                                    target_ch,
                                ).await;
                            });

                            return Ok(());
                        }
                    }

                    if custom_id.starts_with("spreadsheet:cancel:") {
                        let session_id = custom_id.trim_start_matches("spreadsheet:cancel:");
                        let _ = SpreadsheetStore::update_session(session_id, |s| {
                            s.status = "cancelled".to_string();
                        });

                        let response = serenity::CreateInteractionResponse::Message(
                            serenity::CreateInteractionResponseMessage::new()
                                .content(format!("Session `{}` has been cancelled.", session_id))
                                .ephemeral(true),
                        );
                        let _ = component.create_response(ctx, response).await;
                        return Ok(());
                    }
                }

                serenity::Interaction::Modal(modal) => {
                    if modal.data.custom_id == "recruitment:modal_submit" {
                        let mut name = String::new();
                        let mut gp: u32 = 0;
                        let mut team = String::new();

                        for row in &modal.data.components {
                            for comp in &row.components {
                                if let serenity::ActionRowComponent::InputText(input) = comp {
                                    match input.custom_id.as_str() {
                                        "name" => name = input.value.as_deref().unwrap_or("").to_string(),
                                        "gp" => gp = input.value.as_deref().unwrap_or("0").parse().unwrap_or(0),
                                        "team" => team = input.value.as_deref().unwrap_or("").to_string(),
                                        _ => {}
                                    }
                                }
                            }
                        }

                        let ticket_id = uuid::Uuid::new_v4().to_string().chars().take(8).collect::<String>();
                        let ticket = Ticket {
                            id: ticket_id.clone(),
                            applicant_id: modal.user.id.to_string(),
                            applicant_tag: modal.user.tag(),
                            status: "open".to_string(),
                            thread_id: None,
                            channel_id: Some(modal.channel_id.to_string()),
                            claimed_by_id: None,
                            claimed_by_tag: None,
                            assigned_team: if team.is_empty() { None } else { Some(team.clone()) },
                            created_at: chrono::Utc::now().to_rfc3339(),
                            closed_at: None,
                            license_analysis: Some(serde_json::json!({
                                "playerName": name,
                                "garagePower": gp,
                                "manualEntry": true
                            })),
                            license_attachments: Vec::new(),
                            event_attachments: Vec::new(),
                            screenshot_check: Some(serde_json::json!({ "manualEntry": true })),
                            manual_entry: true,
                        };

                        let _ = RecruitmentStore::save_ticket(ticket);

                        let pref_team = if team.is_empty() { None } else { Some(team.as_str()) };
                        let config = load_dashboard_config();
                        let suggested_team = crate::handlers::recruitment_auto::determine_team_for_gp(gp, pref_team, &config);

                        let claim_btn = serenity::CreateButton::new(format!("ticket:claim:{}", ticket_id))
                            .label("Claim Ticket")
                            .style(serenity::ButtonStyle::Primary);
                        let close_btn = serenity::CreateButton::new(format!("ticket:close:{}", ticket_id))
                            .label("Close Ticket")
                            .style(serenity::ButtonStyle::Danger);
                        let row = serenity::CreateActionRow::Buttons(vec![claim_btn, close_btn]);

                        let recruiter_mention = if !config.recruitment.recruiter_role_id.is_empty() {
                            format!("<@&{}>", config.recruitment.recruiter_role_id)
                        } else {
                            "Recruiters".to_string()
                        };

                        let embed = serenity::CreateEmbed::new()
                            .title("Recruitment Application")
                            .description(format!(
                                "Application received for <@{}>:\n\n\
                                • **Driver Name:** `{}`\n\
                                • **Garage Power:** `{} GP`\n\
                                • **Preferred/Suggested Team:** **{}**\n\n\
                                *Recruiter action:* Click **[Claim Ticket]** or **[Close Ticket]** to assign team.",
                                modal.user.id, name, gp, suggested_team
                            ))
                            .color(0x0f766e)
                            .timestamp(serenity::Timestamp::now());

                        let _ = modal.channel_id.send_message(
                            ctx,
                            serenity::CreateMessage::new().embed(embed).components(vec![row]),
                        ).await;

                        let response = serenity::CreateInteractionResponse::Message(
                            serenity::CreateInteractionResponseMessage::new()
                                .content(format!(
                                    "Your application has been submitted as Ticket **#{}**. {} will review it shortly.",
                                    ticket_id, recruiter_mention
                                ))
                                .ephemeral(true),
                        );
                        let _ = modal.create_response(ctx, response).await;
                        return Ok(());
                    }
                }
                _ => {}
            }
        }

        serenity::FullEvent::GuildMemberAddition { new_member } => {
            let config = load_dashboard_config();
            if let Ok(channel_id) = config.bot.welcome_channel_id.parse::<u64>() {
                if channel_id > 0 {
                    let ch = serenity::ChannelId::new(channel_id);
                    let welcome_msg = format!(
                        "Welcome <@{}> to **DCA**! Check out the Apply panel to join one of our teams.",
                        new_member.user.id
                    );
                    let _ = ch.say(ctx, welcome_msg).await;
                }
            }
        }

        _ => {}
    }

    Ok(())
}
