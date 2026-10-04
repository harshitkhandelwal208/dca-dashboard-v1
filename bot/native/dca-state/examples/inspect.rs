//! Read-only check of a state store (Firebase or local JSON): every scope is loaded and parsed with the typed models,
//! nothing is written. `cargo run -p dca-state --example inspect` with the production environment variables set.

use dca_state::config::load_config;
use dca_state::stores::*;
use dca_state::StateStore;

#[tokio::main]
async fn main() {
    let store = StateStore::from_env();
    for scope in ["dashboardConfig", "recruitmentTickets", "recruitmentLogs", "recruitmentBans", "botLogs", "warnings", "spreadsheetSessions", "spreadsheetReportEmissions", "teamRoleAssignments", "reminders"] {
        let value = store.read(scope, serde_json::Value::Null).await;
        let size = serde_json::to_string(&value).map(|s| s.len()).unwrap_or(0);
        println!("{scope:<28} {:<8} {size:>9} bytes", if value.is_null() { "(empty)" } else { "present" });
    }
    let config = load_config(&store).await;
    println!("\nconfig version {} | community guild {:?} | recruitment guild {:?}", config.version, config.bot.community_guild_id, config.bot.recruitment_guild_id);
    println!("dashboard url {:?}", config.bot.dashboard_url);
    println!("recruitment: enabled {} panel channel {:?} log channel {:?} teams {:?}", config.recruitment.enabled, config.recruitment.panel_channel_id, config.recruitment.log_channel_id, config.recruitment.teams);
    println!("member counts: {} teams | reaction roles: {} groups | youtube feeds: {} | spreadsheet teams: {} (enabled {})", config.member_counts.teams.len(), config.reaction_roles.len(), config.youtube.feeds.len(), config.spreadsheets.teams.len(), config.spreadsheets.teams.iter().filter(|t| t.enabled).count());
    let tickets = list_tickets(&store, &TicketFilter::default()).await;
    println!("tickets: {} ({} open)", tickets.len(), tickets.iter().filter(|t| t.status == "open").count());
    println!("recruitment logs: {} | bans: {} | bot logs: {}", list_recruitment_logs(&store, 500).await.len(), list_recruitment_bans(&store).await.len(), list_bot_logs(&store, 500, "").await.len());
    let sessions = list_sessions(&store, &SessionFilter::default()).await;
    println!("spreadsheet sessions: {} ({} processed, {} with stored readings)", sessions.len(), sessions.iter().filter(|s| s.status == "processed").count(), sessions.iter().filter(|s| !s.readings.is_empty()).count());
    println!("reminders: {}", list_reminders(&store, None).await.len());
}
