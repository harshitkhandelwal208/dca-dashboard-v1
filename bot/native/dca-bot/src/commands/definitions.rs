//! Slash command definitions (names, options and default permissions match the original bot).

use serenity::all::*;

fn sub(name: &str, description: &str) -> CreateCommandOption {
    CreateCommandOption::new(CommandOptionType::SubCommand, name, description)
}

fn opt(kind: CommandOptionType, name: &str, description: &str, required: bool) -> CreateCommandOption {
    CreateCommandOption::new(kind, name, description).required(required)
}

fn string(name: &str, description: &str, required: bool) -> CreateCommandOption {
    opt(CommandOptionType::String, name, description, required)
}

fn user(name: &str, description: &str, required: bool) -> CreateCommandOption {
    opt(CommandOptionType::User, name, description, required)
}

fn role(name: &str, description: &str, required: bool) -> CreateCommandOption {
    opt(CommandOptionType::Role, name, description, required)
}

fn int(name: &str, description: &str, required: bool) -> CreateCommandOption {
    opt(CommandOptionType::Integer, name, description, required)
}

fn attachment(name: &str, description: &str, required: bool) -> CreateCommandOption {
    opt(CommandOptionType::Attachment, name, description, required)
}

fn text_channel(name: &str, description: &str, required: bool) -> CreateCommandOption {
    opt(CommandOptionType::Channel, name, description, required).channel_types(vec![ChannelType::Text, ChannelType::News])
}

// ------------------------------------------------------------------------------------------ /tickets

fn screenshot_type(option: CreateCommandOption) -> CreateCommandOption {
    option.add_string_choice("Driver's license", "license").add_string_choice("Team event scores", "event")
}

fn tickets() -> CreateCommand {
    CreateCommand::new("tickets")
        .description("Manage recruitment tickets")
        .add_option(
            sub("setup", "Configure and post the recruitment Apply panel")
                .add_sub_option(text_channel("channel", "Channel where the Apply panel should be posted", true))
                .add_sub_option(text_channel("log_channel", "Channel where applicant ticket images should be sent on close", false))
                .add_sub_option(role("recruiter_role", "Role that can manage recruitment tickets", false))
                .add_sub_option(string("title", "Panel embed title", false).max_length(120))
                .add_sub_option(string("description", "Panel embed body", false).max_length(2000))
                .add_sub_option(opt(CommandOptionType::Boolean, "private_threads", "Create private application threads when possible", false)),
        )
        .add_option(sub("sync-panel", "Refresh the recruitment Apply panel from saved settings"))
        .add_option(sub("sync-banlist", "Refresh the recruitment team ban list embeds"))
        .add_option(sub("status", "Show current recruitment ticket settings"))
        .add_option(sub("logs", "Show recent recruitment outcomes"))
        .add_option(sub("claim", "Claim the recruitment ticket thread you are in"))
        .add_option(
            sub("close", "Close this recruitment ticket and log the final outcome").add_sub_option(
                string("outcome", "Select a team or reject the application", false)
                    .add_string_choice("Discord", "discord")
                    .add_string_choice("Discord\u{b2}", "discord2")
                    .add_string_choice("Discord 3\u{2122}", "discord3")
                    .add_string_choice("Nascar DC", "nascar-dc")
                    .add_string_choice("Rejected", "rejected"),
            ),
        )
        .add_option(sub("add", "Add a user to this ticket thread").add_sub_option(user("user", "User to add", true)))
        .add_option(sub("massadd", "Add multiple users to this ticket by mentions or IDs").add_sub_option(string("users", "Mentions or user IDs separated by spaces", true).max_length(1000)))
        .add_option(sub("remove", "Remove a user from this ticket thread").add_sub_option(user("user", "User to remove", true)))
        .add_option(sub("rename", "Rename this recruitment ticket thread").add_sub_option(string("name", "New thread name", true).max_length(90)))
        .add_option(sub("screenshot-list", "List saved ticket screenshots and their indexes").add_sub_option(screenshot_type(string("type", "Which screenshot set to list", true))))
        .add_option(
            sub("screenshot-add", "Add corrected screenshots to this ticket")
                .add_sub_option(screenshot_type(string("type", "Which screenshot set to update", true)))
                .add_sub_option(attachment("image", "Image screenshot to save", true))
                .add_sub_option(attachment("image_2", "Additional image screenshot to save", false))
                .add_sub_option(attachment("image_3", "Additional image screenshot to save", false))
                .add_sub_option(attachment("image_4", "Additional image screenshot to save", false))
                .add_sub_option(attachment("image_5", "Additional image screenshot to save", false)),
        )
        .add_option(
            sub("screenshot-remove", "Remove a saved screenshot from this ticket")
                .add_sub_option(screenshot_type(string("type", "Which screenshot set to update", true)))
                .add_sub_option(int("index", "Screenshot index from /tickets screenshot-list", true).min_int_value(1)),
        )
        .add_option(
            sub("screenshot-change", "Replace a saved screenshot in this ticket")
                .add_sub_option(screenshot_type(string("type", "Which screenshot set to update", true)))
                .add_sub_option(int("index", "Screenshot index from /tickets screenshot-list", true).min_int_value(1))
                .add_sub_option(attachment("image", "Image screenshot to save", true)),
        )
        .add_option(sub("archive", "Lock and archive this ticket thread"))
        .add_option(sub("delete", "Delete this ticket thread"))
}

// -------------------------------------------------------------------------------------- /spreadsheets

fn team_opt() -> CreateCommandOption {
    string("team", "Configured spreadsheet team name or ID", true).max_length(80)
}

fn session_opt(required: bool) -> CreateCommandOption {
    string("session_id", "Spreadsheet session ID. Defaults to the latest matching session.", required).max_length(80)
}

fn row_opt() -> CreateCommandOption {
    int("row", "Placement/rank row to correct", true).min_int_value(1).max_int_value(100)
}

fn value_opt(description: &str, required: bool) -> CreateCommandOption {
    string("value", description, required).max_length(120)
}

fn anchor_opt() -> CreateCommandOption {
    string("anchor_date", "Optional YYYY-MM-DD date inside the week or month to report. Defaults to latest processed session.", false).max_length(10)
}

fn spreadsheets() -> CreateCommand {
    CreateCommand::new("spreadsheets")
        .description("Manage team-event screenshot spreadsheet sessions")
        .add_option(sub("status", "Show configured spreadsheet status for a team").add_sub_option(team_opt()))
        .add_option(sub("sessions", "List recent spreadsheet sessions for a team").add_sub_option(team_opt()))
        .add_option(
            sub("generate", "Generate or refresh a spreadsheet from a pending screenshot session")
                .add_sub_option(team_opt())
                .add_sub_option(session_opt(false))
                .add_sub_option(opt(CommandOptionType::Boolean, "rerun_ocr", "Download images and read them with the local OCR again", false)),
        )
        .add_option(sub("summary", "View the parsed summary for a spreadsheet session").add_sub_option(team_opt()).add_sub_option(session_opt(false)))
        .add_option(sub("weekly", "Build the weekly team-event report with missed events scored as zero").add_sub_option(team_opt()).add_sub_option(anchor_opt()))
        .add_option(sub("monthly", "Build the monthly team-event report with missed events scored as zero").add_sub_option(team_opt()).add_sub_option(anchor_opt()))
        .add_option(sub("file", "Get the generated Calc spreadsheet file").add_sub_option(team_opt()).add_sub_option(session_opt(false)))
        .add_option(sub("chart", "Get the generated summary chart").add_sub_option(team_opt()).add_sub_option(session_opt(false)))
        .add_option(
            sub("correct", "Correct a parsed field and rebuild the spreadsheet")
                .add_sub_option(team_opt())
                .add_sub_option(session_opt(true))
                .add_sub_option(row_opt())
                .add_sub_option(
                    string("field", "Field to correct", true)
                        .add_string_choice("Player name", "player_name")
                        .add_string_choice("Team name", "team_name")
                        .add_string_choice("Placement", "placement")
                        .add_string_choice("Points", "points")
                        .add_string_choice("Score", "score")
                        .add_string_choice("Team type", "team_type")
                        .add_string_choice("Event name", "event_name"),
                )
                .add_sub_option(value_opt("Replacement value", true)),
        )
        .add_option(sub("correct-name", "Staff: correct a player name and rebuild").add_sub_option(team_opt()).add_sub_option(session_opt(true)).add_sub_option(row_opt()).add_sub_option(value_opt("Correct player name", true)))
        .add_option(
            sub("correct-team", "Staff: correct a player team assignment and rebuild")
                .add_sub_option(team_opt())
                .add_sub_option(session_opt(true))
                .add_sub_option(row_opt())
                .add_sub_option(string("team_type", "Corrected team classification", true).add_string_choice("Own team", "own").add_string_choice("Opponent", "opponent"))
                .add_sub_option(value_opt("Optional team label", false)),
        )
        .add_option(
            sub("correct-placement", "Staff: correct a placement/rank and rebuild")
                .add_sub_option(team_opt())
                .add_sub_option(session_opt(true))
                .add_sub_option(row_opt())
                .add_sub_option(int("placement", "Correct placement/rank", true).min_int_value(1).max_int_value(100)),
        )
        .add_option(
            sub("correct-points", "Staff: correct event points or score and rebuild")
                .add_sub_option(team_opt())
                .add_sub_option(session_opt(true))
                .add_sub_option(row_opt())
                .add_sub_option(string("field", "Which numeric field to correct", true).add_string_choice("Event points", "points").add_string_choice("Score", "score"))
                .add_sub_option(value_opt("Replacement value", true)),
        )
        .add_option(sub("correct-event-name", "Staff: correct the event name and rebuild").add_sub_option(team_opt()).add_sub_option(session_opt(true)).add_sub_option(value_opt("Correct event name", true)))
        .add_option(sub("rebuild", "Rebuild spreadsheet outputs from the saved readings and corrections").add_sub_option(team_opt()).add_sub_option(session_opt(true)))
        .add_option(sub("regenerate-weekly", "Staff: regenerate and post the weekly report").add_sub_option(team_opt()).add_sub_option(anchor_opt()))
        .add_option(sub("regenerate-monthly", "Staff: regenerate and post the monthly report").add_sub_option(team_opt()).add_sub_option(anchor_opt()))
        .add_option(sub("test-ocr", "TEMP: test the local OCR reading for a session").add_sub_option(team_opt()).add_sub_option(session_opt(false)))
        .add_option(sub("test-grouping", "TEMP: inspect current session grouping state").add_sub_option(team_opt()))
        .add_option(sub("preview", "TEMP: preview parsed output without posting final files").add_sub_option(team_opt()).add_sub_option(session_opt(false)))
        .add_option(sub("rebuild-event", "TEMP: rebuild one event output").add_sub_option(team_opt()).add_sub_option(session_opt(true)))
        .add_option(sub("force-weekly", "TEMP: force weekly report generation").add_sub_option(team_opt()).add_sub_option(anchor_opt()))
        .add_option(sub("force-monthly", "TEMP: force monthly report generation").add_sub_option(team_opt()).add_sub_option(anchor_opt()))
}

pub fn all() -> Vec<CreateCommand> {
    vec![
        CreateCommand::new("ban")
            .description("Ban a user from the server")
            .add_option(user("user", "User to ban", true))
            .add_option(string("reason", "Reason for the ban", true))
            .default_member_permissions(Permissions::BAN_MEMBERS),
        CreateCommand::new("clearwarns")
            .description("Clears all warnings for a user.")
            .add_option(user("user", "User whose warnings will be cleared", true))
            .default_member_permissions(Permissions::MANAGE_MESSAGES),
        CreateCommand::new("dashboard").description("Get a quick link to the bot dashboard."),
        CreateCommand::new("ping").description("Check that the bot is alive and how fast it answers."),
        CreateCommand::new("help").description("Show useful bot commands and dashboard links."),
        CreateCommand::new("invite").description("Send the ticket applicant a single-use destination server invite."),
        CreateCommand::new("membercount")
            .description("Manage the team member count message")
            .default_member_permissions(Permissions::MANAGE_GUILD)
            .add_option(sub("sync", "Create or update the member count message from dashboard settings"))
            .add_option(
                sub("set", "Set a team's player count")
                    .add_sub_option(string("team", "Team name", true))
                    .add_sub_option(int("players", "Player count", true).min_int_value(0).max_int_value(999))
                    .add_sub_option(string("status", "Recruitment status", false)),
            )
            .add_option(sub("list", "Show configured member counts")),
        CreateCommand::new("whois").description("Displays information about a user.").add_option(user("target", "The user to get information about", false)),
        CreateCommand::new("roles")
            .description("Manage roles")
            .default_member_permissions(Permissions::MANAGE_ROLES)
            .add_option(sub("add", "Add a role to a user").add_sub_option(user("user", "User to assign the role", true)).add_sub_option(role("role", "Role to assign", true)))
            .add_option(sub("remove", "Remove a role from a user").add_sub_option(user("user", "User to remove the role from", true)).add_sub_option(role("role", "Role to remove", true))),
        CreateCommand::new("yt")
            .description("Inspect dashboard-managed YouTube notifications")
            .default_member_permissions(Permissions::MANAGE_GUILD)
            .add_option(sub("list", "Show configured YouTube feeds"))
            .add_option(sub("check", "Run one YouTube feed check now")),
        CreateCommand::new("remindme")
            .description("Set a reminder")
            .add_option(string("duration", "e.g., 10m, 1h", true))
            .add_option(string("message", "Reminder message", true)),
        CreateCommand::new("reminders").description("List your active reminders"),
        CreateCommand::new("cancelreminder").description("Cancel a reminder by ID").add_option(int("id", "The ID of the reminder to cancel", true)),
        CreateCommand::new("snap")
            .description("Thanos-style moderation command")
            .default_member_permissions(Permissions::ADMINISTRATOR)
            .add_option(sub("ban", "Ban someone... the Thanos way").add_sub_option(user("user", "User to ban", true)).add_sub_option(string("reason", "Reason for the snap", false)))
            .add_option(sub("kick", "Kick someone... dramatically").add_sub_option(user("user", "User to kick", true)).add_sub_option(string("reason", "Reason for the snap", false))),
        CreateCommand::new("teamcount")
            .description("Quickly change a team member count.")
            .default_member_permissions(Permissions::MANAGE_GUILD)
            .add_option(string("team", "Team name or alias", true))
            .add_option(int("players", "New player count", true).min_int_value(0).max_int_value(999))
            .add_option(string("status", "Optional recruitment status", false)),
        CreateCommand::new("updatecount")
            .description("Update a dashboard-managed team member count")
            .default_member_permissions(Permissions::MANAGE_GUILD)
            .add_option(string("team", "Team name", true))
            .add_option(string("field", "Field to update", true).add_string_choice("Number of Players", "players").add_string_choice("Recruitment Status", "recruitment"))
            .add_option(string("value", "New value", true)),
        CreateCommand::new("top3te")
            .description("Announce the top 3 winners of the team event")
            .add_option(role("pingrole", "Ping role to tag", true))
            .add_option(string("first", "First place winner (mention or name)", true))
            .add_option(string("second", "Second place winner (mention or name)", true))
            .add_option(string("third", "Third place winner (mention or name)", true))
            .add_option(attachment("screenshot", "Attach the event screenshot", true)),
        CreateCommand::new("top3km")
            .description("Announce the top 3 KM drivers of the week")
            .add_option(role("pingrole", "Ping role to mention at the top", true))
            .add_option(int("chestlevel", "Chest level achieved", true))
            .add_option(string("first", "First place (mention or user ID)", true))
            .add_option(string("second", "Second place (mention or user ID)", true))
            .add_option(string("third", "Third place (mention or user ID)", true))
            .add_option(attachment("screenshot", "Attach the event screenshot", true)),
        CreateCommand::new("teameventsummary")
            .description("Summary of Team Event after Match, for formula dc")
            .add_option(string("won_streak", "Enter the won streak (e.g., \"5 wins\")", true))
            .add_option(string("lost_streak", "Enter the lost streak (e.g., \"2 losses\")", true))
            .add_option(string("topper1", "Enter the first match topper name", true))
            .add_option(string("topper2", "Enter the second match topper name", true))
            .add_option(string("topper3", "Enter the third match topper name", true))
            .add_option(string("total_points_match", "Enter the total points of the match", true))
            .add_option(string("total_points", "Enter the total points overall", true))
            .add_option(string("total_points_gained", "Enter the total points gained", true))
            .add_option(string("aura_count", "Enter the aura count", true))
            .add_option(role("ping_role", "Select the role to ping outside the embed", false))
            .add_option(attachment("screenshot", "Upload a screenshot for the event summary", false)),
        CreateCommand::new("sportscar").description("Displays masteries of the sports car."),
        CreateCommand::new("garage").description("Open your garage and view car masteries"),
        CreateCommand::new("warn")
            .description("Warn a user and log it in the database.")
            .add_option(user("user", "User to warn", true))
            .add_option(string("reason", "Reason for the warning", true))
            .default_member_permissions(Permissions::MANAGE_MESSAGES),
        tickets(),
        spreadsheets(),
    ]
}

#[cfg(test)]
mod tests {
    /// `DCA_DUMP_COMMANDS=<file>` writes the registered command JSON (used to diff against the old bot's definitions).
    #[test]
    fn dump_commands() {
        let json = serde_json::to_value(super::all()).unwrap();
        if let Ok(path) = std::env::var("DCA_DUMP_COMMANDS") {
            std::fs::write(path, serde_json::to_string_pretty(&json).unwrap()).unwrap();
        }
        let names: Vec<&str> = json.as_array().unwrap().iter().filter_map(|c| c["name"].as_str()).collect();
        let mut sorted = names.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), names.len(), "duplicate command names");
        assert_eq!(names.len(), 24);
    }
}
