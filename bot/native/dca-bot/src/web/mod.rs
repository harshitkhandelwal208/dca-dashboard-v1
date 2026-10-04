//! HTTP server: health checks, Discord OAuth sign-in and the dashboard API (the Rust port of
//! `dashboard/server/*.js`). The React dashboard (`dashboard/`, built to `dashboard/dist`) is served from here too.

mod auth;
mod routes;

use crate::app::App;
use axum::routing::{get, post};
use axum::Router;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

pub use auth::Web;

pub fn router(app: Arc<App>) -> Router {
    let web = Arc::new(Web {
        app,
        secret: auth::session_secret(),
        sessions: Mutex::new(Default::default()),
    });
    Router::new()
        .route("/", get(routes::root))
        .route("/health", get(routes::health))
        .route("/auth/discord", get(auth::login))
        .route("/auth/discord/callback", get(auth::callback))
        .route("/auth/logout", get(auth::logout_redirect))
        .route("/api/dashboard/logout", post(auth::logout_api))
        .route("/api/dashboard/me", get(routes::me))
        .route("/api/dashboard/config", get(routes::get_config).put(routes::put_config))
        .route("/api/dashboard/reaction-roles/sync", post(routes::sync_reaction_roles))
        .route("/api/dashboard/member-counts/sync", post(routes::sync_member_counts))
        .route("/api/dashboard/recruitment/panel/sync", post(routes::sync_panel))
        .route("/api/dashboard/recruitment/ban-list/sync", post(routes::sync_ban_list))
        .route("/api/dashboard/recruitment/logs", get(routes::recruitment_logs))
        .route("/api/dashboard/tickets", get(routes::tickets))
        .route("/api/dashboard/tickets/{thread_id}/transcript", get(routes::transcript))
        .route("/api/dashboard/logs", get(routes::logs))
        .route(
            "/api/dashboard/recruitment/tutorials/{id}/upload",
            post(routes::upload_tutorial).layer(axum::extract::DefaultBodyLimit::max(routes::upload_limit())),
        )
        .route("/dashboard", get(routes::index))
        .route("/dashboard/", get(routes::index))
        .route("/dashboard/{*path}", get(routes::dashboard_file))
        .route("/uploads/{*path}", get(routes::upload_file))
        .fallback(routes::static_file)
        .with_state(web)
}

pub async fn serve(app: Arc<App>, port: u16) {
    let router = router(app);
    let address = SocketAddr::from(([0, 0, 0, 0], port));
    match tokio::net::TcpListener::bind(address).await {
        Ok(listener) => {
            tracing::info!("Web server running on {port}");
            if let Err(error) = axum::serve(listener, router).await {
                tracing::error!("Web server stopped: {error}");
            }
        }
        Err(error) => tracing::error!("Could not bind the web server on port {port}: {error}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::Dirs;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use dca_state::StateStore;
    use serenity::http::Http;
    use tower::ServiceExt;

    fn test_app(name: &str) -> Arc<App> {
        let dir = std::env::temp_dir().join(format!("dca-web-test-{name}-{}", std::process::id()));
        let dirs = Dirs { data: dir.clone(), fonts: dir.join("fonts"), models: dir.join("models"), assets: dir.join("assets"), dashboard: dir.join("dist") };
        App::new(Arc::new(Http::new("test-token")), StateStore::local(dir), dirs)
    }

    async fn call(app: Arc<App>, method: &str, uri: &str) -> (StatusCode, String) {
        let response = router(app).oneshot(Request::builder().method(method).uri(uri).body(Body::empty()).unwrap()).await.unwrap();
        let status = response.status();
        let body = axum::body::to_bytes(response.into_body(), 1 << 20).await.unwrap();
        (status, String::from_utf8_lossy(&body).to_string())
    }

    #[tokio::test]
    async fn health_reflects_discord_status() {
        let app = test_app("health");
        let (status, body) = call(app.clone(), "GET", "/health").await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert!(body.contains("unhealthy"));
        app.ready.store(true, std::sync::atomic::Ordering::SeqCst);
        let (status, body) = call(app.clone(), "GET", "/health").await;
        assert_eq!(status, StatusCode::OK);
        assert!(body.contains("\"healthy\""));
        let (status, body) = call(app, "GET", "/").await;
        assert_eq!(status, StatusCode::OK);
        assert!(body.contains("alive"));
    }

    #[tokio::test]
    async fn dashboard_api_requires_sign_in() {
        let app = test_app("auth");
        for (method, uri) in [
            ("GET", "/api/dashboard/config"),
            ("PUT", "/api/dashboard/config"),
            ("POST", "/api/dashboard/reaction-roles/sync"),
            ("POST", "/api/dashboard/member-counts/sync"),
            ("POST", "/api/dashboard/recruitment/panel/sync"),
            ("POST", "/api/dashboard/recruitment/ban-list/sync"),
            ("GET", "/api/dashboard/tickets"),
            ("GET", "/api/dashboard/logs"),
            ("GET", "/api/dashboard/tickets/123/transcript"),
        ] {
            let (status, body) = call(app.clone(), method, uri).await;
            assert_eq!(status, StatusCode::UNAUTHORIZED, "{method} {uri}: {body}");
        }
    }

    #[tokio::test]
    async fn login_without_setup_redirects_to_the_setup_notice() {
        let app = test_app("login");
        let response = router(app).oneshot(Request::builder().uri("/auth/discord").body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        assert_eq!(response.headers().get("location").unwrap(), "/dashboard?auth=setup");
    }

    #[tokio::test]
    async fn sessions_cookies_are_signed() {
        let web = Web { app: test_app("cookie"), secret: b"secret".to_vec(), sessions: Mutex::new(Default::default()) };
        let signed = web.sign_for_test("abc");
        assert_eq!(web.unsign_for_test(&signed).as_deref(), Some("abc"));
        assert!(web.unsign_for_test(&format!("{signed}x")).is_none());
        assert!(web.unsign_for_test("abc.AAAA").is_none());
    }

    #[tokio::test]
    async fn serves_the_built_dashboard() {
        let app = test_app("static");
        let dist = app.dirs.dashboard.clone();
        std::fs::create_dir_all(dist.join("assets")).unwrap();
        std::fs::write(dist.join("index.html"), "<html>dash</html>").unwrap();
        std::fs::write(dist.join("assets/app.js"), "console.log(1)").unwrap();
        for uri in ["/dashboard", "/dashboard/"] {
            let (status, body) = call(app.clone(), "GET", uri).await;
            assert_eq!(status, StatusCode::OK, "{uri}");
            assert!(body.contains("dash"));
        }
        for uri in ["/assets/app.js", "/dashboard/assets/app.js"] {
            let (status, body) = call(app.clone(), "GET", uri).await;
            assert_eq!(status, StatusCode::OK, "{uri}");
            assert!(body.contains("console.log"));
        }
        let (status, _) = call(app.clone(), "GET", "/assets/missing.js").await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        let (status, _) = call(app, "GET", "/uploads/missing.mp4").await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }
}
