//! HTTP web server for Render keep-alive, health checks, and the DCA Web Dashboard.

use axum::extract::Path;
use axum::http::{header, StatusCode};
use axum::response::{Html, IntoResponse};
use axum::routing::{get, post};
use axum::{Json, Router};
use dca_state::config::{load_dashboard_config, save_dashboard_config, DashboardConfig};
use dca_state::log_store::LogStore;
use dca_state::recruitment_store::RecruitmentStore;
use dca_state::spreadsheet_store::SpreadsheetStore;
use serde_json::json;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Instant;

use super::dashboard_html::render_dashboard_html;

pub async fn start_web_server(port: u16, start_time: Instant) {
    let start_arc = Arc::new(start_time);

    let start_for_dash = start_arc.clone();
    let dashboard_handler = move || {
        let uptime = start_for_dash.elapsed().as_secs();
        let config = load_dashboard_config();
        let tickets = RecruitmentStore::list_tickets();
        let sessions = SpreadsheetStore::list_sessions();
        let logs = LogStore::list_logs();
        let html = render_dashboard_html(uptime, &config, &tickets, &sessions, &logs);
        async move { Html(html) }
    };

    let start_for_health = start_arc.clone();
    let health_handler = move || {
        let uptime = start_for_health.elapsed().as_secs();
        async move {
            Json(json!({
                "status": "healthy",
                "uptime": uptime,
                "discordStatus": "connected",
                "timestamp": chrono::Utc::now().to_rfc3339()
            }))
        }
    };

    let start_for_stats = start_arc.clone();
    let stats_handler = move || {
        let uptime = start_for_stats.elapsed().as_secs();
        let tickets = RecruitmentStore::list_tickets();
        let sessions = SpreadsheetStore::list_sessions();
        let logs = LogStore::list_logs();

        let open_tickets = tickets.iter().filter(|t| t.status == "open" || t.status == "claimed").count();
        let completed_sessions = sessions.iter().filter(|s| s.status == "completed").count();

        async move {
            Json(json!({
                "uptime": uptime,
                "tickets": {
                    "total": tickets.len(),
                    "open": open_tickets,
                },
                "spreadsheets": {
                    "total": sessions.len(),
                    "completed": completed_sessions,
                },
                "logs": logs.len(),
            }))
        }
    };

    let app = Router::new()
        .route("/", get(dashboard_handler.clone()))
        .route("/dashboard", get(dashboard_handler))
        .route("/health", get(health_handler))
        .route("/api/stats", get(stats_handler))
        .route("/api/tickets", get(|| async {
            Json(RecruitmentStore::list_tickets())
        }))
        .route("/api/sessions", get(|| async {
            Json(SpreadsheetStore::list_sessions())
        }))
        .route("/api/logs", get(|| async {
            Json(LogStore::list_logs())
        }))
        .route("/api/config", get(|| async {
            Json(load_dashboard_config())
        }))
        .route("/api/config", post(|Json(new_config): Json<DashboardConfig>| async move {
            match save_dashboard_config(&new_config) {
                Ok(_) => (StatusCode::OK, Json(json!({ "status": "saved" }))),
                Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": e }))),
            }
        }))
        .route("/spreadsheets/{filename}", get(|Path(filename): Path<String>| async move {
            // Validate filename to prevent path traversal
            if filename.contains("..") || filename.contains('/') || filename.contains('\\') {
                return (StatusCode::BAD_REQUEST, [(header::CONTENT_TYPE, "text/plain")], vec![]).into_response();
            }

            let path = format!("data/spreadsheets/{}", filename);
            match tokio::fs::read(&path).await {
                Ok(bytes) => {
                    let content_type = if filename.ends_with(".xlsx") {
                        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"
                    } else if filename.ends_with(".png") {
                        "image/png"
                    } else {
                        "application/octet-stream"
                    };

                    (StatusCode::OK, [(header::CONTENT_TYPE, content_type)], bytes).into_response()
                }
                Err(_) => (StatusCode::NOT_FOUND, [(header::CONTENT_TYPE, "text/plain")], "File not found".as_bytes().to_vec()).into_response(),
            }
        }));

    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    println!("[web] DCA Web Dashboard running on port {port}");

    let listener = match tokio::net::TcpListener::bind(addr).await {
        Ok(l) => l,
        Err(e) => {
            eprintln!("Failed to bind web server port {port}: {e}");
            return;
        }
    };

    let _ = axum::serve(listener, app).await;
}
