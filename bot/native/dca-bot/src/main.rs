//! DCA Discord bot + dashboard, one process (the Rust port of `bot/index.js` + `dashboard/server`).

mod app;
mod commands;
#[cfg(test)]
mod e2e_tests;
#[cfg(test)]
mod bench_tests;
#[cfg(test)]
mod command_tests;
#[cfg(test)]
mod flow_tests;
#[cfg(test)]
mod mock_discord;
mod events;
mod logs;
mod managers;
mod opts;
mod responder;
mod util;
mod web;

use app::{App, Dirs};
use dca_core::ocr::{self, MODEL_BASE_URL};
use dca_core::reader::Reader;
use dca_state::StateStore;
use serenity::all::*;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

/// Fetch any missing model file (`bot/scripts/download-models.sh` does the same ahead of time in Docker).
async fn ensure_models(dir: &Path) -> Result<(), String> {
    let base = std::env::var("DCA_MODEL_BASE_URL").unwrap_or_else(|_| MODEL_BASE_URL.to_string());
    std::fs::create_dir_all(dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    let client = reqwest::Client::builder().timeout(Duration::from_secs(600)).build().map_err(|e| e.to_string())?;
    for file in ocr::required_files() {
        let target = dir.join(file);
        if std::fs::metadata(&target).map(|m| m.len() > 0).unwrap_or(false) {
            continue;
        }
        tracing::info!("[ocr] downloading {file}");
        let response = client.get(format!("{base}/{file}")).send().await.and_then(|r| r.error_for_status()).map_err(|e| format!("{file}: {e}"))?;
        let bytes = response.bytes().await.map_err(|e| format!("{file}: {e}"))?;
        let part = dir.join(format!("{file}.part"));
        std::fs::write(&part, &bytes).map_err(|e| format!("{file}: {e}"))?;
        std::fs::rename(&part, &target).map_err(|e| format!("{file}: {e}"))?;
    }
    Ok(())
}

/// Load the PaddleOCR models in the background so the bot (and the web server) come up immediately.
fn start_ocr_loader(app: Arc<App>) {
    tokio::spawn(async move {
        let models = app.dirs.models.clone();
        let fonts = app.dirs.fonts.clone();
        loop {
            let result = match ensure_models(&models).await {
                Ok(()) => {
                    let (m, f) = (models.clone(), fonts.clone());
                    tokio::task::spawn_blocking(move || Reader::new(&m, &f)).await.map_err(|e| e.to_string()).and_then(|r| r)
                }
                Err(error) => Err(error),
            };
            match result {
                Ok(reader) => {
                    *app.reader.write().unwrap() = Some(Arc::new(reader));
                    app.reader_error.write().unwrap().clear();
                    tracing::info!("[ocr] PaddleOCR ready ({})", models.display());
                    return;
                }
                Err(error) => {
                    tracing::error!("[ocr] could not start local OCR: {error}");
                    *app.reader_error.write().unwrap() = error;
                    tokio::time::sleep(Duration::from_secs(300)).await;
                }
            }
        }
    });
}

async fn register_commands(http: &Http) {
    let commands = commands::definitions::all();
    let count = commands.len();
    match Command::set_global_commands(http, commands).await {
        Ok(registered) => tracing::info!("Registered {} global command(s) (of {count}).", registered.len()),
        Err(error) => tracing::error!("Slash command deployment failed: {error}"),
    }
}

fn load_env() {
    for candidate in ["../.env", ".env", "bot/.env", "../../.env"] {
        if Path::new(candidate).exists() {
            let _ = dotenvy::from_filename(candidate);
        }
    }
    let _ = dotenvy::dotenv();
}

fn main() {
    // One malloc arena: with many threads glibc otherwise keeps a heap per thread, which matters on a 512 MB host.
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    unsafe {
        libc::mallopt(libc::M_ARENA_MAX, 1);
    }
    // Deep async handlers + OCR run on these threads: give them room (virtual memory only; it is not resident).
    let runtime = tokio::runtime::Builder::new_multi_thread().enable_all().thread_stack_size(16 * 1024 * 1024).build().expect("tokio runtime");
    runtime.block_on(run());
}

async fn run() {
    load_env();
    tracing_subscriber::fmt().with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info,serenity=warn,tracing=warn,ort=warn".into())).init();

    let token = std::env::var("DISCORD_TOKEN").unwrap_or_default();
    tracing::info!("Token configured: {}", if token.is_empty() { "no" } else { "yes" });
    if token.trim().is_empty() {
        tracing::error!("DISCORD_TOKEN is not set. Refusing to run a disconnected bot process.");
        std::process::exit(1);
    }

    let dirs = Dirs::from_env();
    let store = StateStore::from_env();
    let http = Arc::new(Http::new(&token));
    // Interaction replies and command registration need the application id.
    match http.get_current_application_info().await {
        Ok(info) => http.set_application_id(info.id),
        Err(error) => tracing::error!("could not read the application id: {error}"),
    }
    let app = App::new(http.clone(), store, dirs);
    tracing::info!("dirs: {:?}", app.dirs);

    start_ocr_loader(app.clone());

    // The bot and dashboard share this single HTTP process and port.
    let port: u16 = std::env::var("PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(3000);
    tokio::spawn(web::serve(app.clone(), port));

    if std::env::var("DCA_SKIP_COMMAND_DEPLOY").map(|v| v == "1").unwrap_or(false) {
        tracing::info!("Skipping slash command registration (DCA_SKIP_COMMAND_DEPLOY=1).");
    } else {
        let http = http.clone();
        tokio::spawn(async move { register_commands(&http).await });
    }

    let intents = GatewayIntents::GUILDS
        | GatewayIntents::GUILD_MESSAGES
        | GatewayIntents::GUILD_MEMBERS
        | GatewayIntents::GUILD_MESSAGE_REACTIONS
        | GatewayIntents::GUILD_MODERATION
        | GatewayIntents::MESSAGE_CONTENT
        | GatewayIntents::DIRECT_MESSAGES;

    tracing::info!("Attempting Discord connection...");
    let mut client = match Client::builder(&token, intents).event_handler(events::Handler { app: app.clone() }).await {
        Ok(client) => client,
        Err(error) => {
            tracing::error!("Discord login failed: {error}");
            std::process::exit(1);
        }
    };
    // Keep the gateway heartbeat latency where `/ping` can read it.
    {
        let manager = client.shard_manager.clone();
        let app = app.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(5)).await;
                let runners = manager.runners.lock().await;
                let latency = runners.values().filter_map(|r| r.latency).map(|d| d.as_millis() as u64).max().unwrap_or(0);
                app.gateway_ms.store(latency, std::sync::atomic::Ordering::Relaxed);
            }
        });
    }
    // `start` reconnects on its own; it only returns on fatal errors (invalid token, disallowed intents...).
    if let Err(error) = client.start().await {
        tracing::error!("Discord client error: {error}");
        std::process::exit(1);
    }
}
