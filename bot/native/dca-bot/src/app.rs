//! Shared application state.

use crate::util::*;
use dca_core::reader::Reader;
use dca_core::render::Renderer;
use dca_state::config::{load_config, DashboardConfig};
use dca_state::StateStore;
use serenity::all::*;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, Semaphore};

#[derive(Clone, Debug)]
pub struct Dirs {
    pub data: PathBuf,
    pub fonts: PathBuf,
    pub models: PathBuf,
    pub assets: PathBuf,
    pub dashboard: PathBuf,
}

fn first_existing(candidates: &[&str], fallback: &str) -> PathBuf {
    for c in candidates {
        let p = PathBuf::from(c);
        if p.exists() {
            return p;
        }
    }
    PathBuf::from(fallback)
}

impl Dirs {
    pub fn from_env() -> Dirs {
        let env = |k: &str| std::env::var(k).ok().filter(|v| !v.trim().is_empty()).map(PathBuf::from);
        Dirs {
            data: dca_state::state_store::data_dir(),
            fonts: env("DCA_FONTS_DIR").unwrap_or_else(|| first_existing(&["fonts", "bot/fonts", "../fonts", "../../fonts", "/app/fonts"], "fonts")),
            models: env("DCA_MODELS_DIR").unwrap_or_else(|| first_existing(&["models", "bot/models", "../models", "../../models", "/app/models"], "models")),
            assets: env("DCA_ASSETS_DIR").unwrap_or_else(|| first_existing(&["assets", "bot/assets", "../assets", "../../assets", "/app/assets"], "assets")),
            dashboard: env("DASHBOARD_DIST_DIR")
                .unwrap_or_else(|| first_existing(&["dashboard/dist", "../dashboard/dist", "../../dashboard/dist", "../../../dashboard/dist", "/app/dashboard"], "dashboard/dist")),
        }
    }
}

/// A recruitment application in progress (the old in-memory `activeSessions`).
#[derive(Clone, Debug)]
pub struct ApplySession {
    pub token: String,
    pub user_id: UserId,
    pub user_tag: String,
    pub username: String,
    pub guild_id: GuildId,
    pub channel_id: ChannelId,
    pub license: Vec<dca_state::models::Attachment>,
    pub events: Vec<dca_state::models::Attachment>,
    /// Some screenshot could not be checked in time and was accepted on trust (recruiters are told).
    pub unverified: bool,
}

/// Per guild: when it was fetched, the guild owner and the guild's roles.
pub type RolesCache = HashMap<GuildId, (Instant, UserId, HashMap<RoleId, Role>)>;

pub struct App {
    pub http: Arc<Http>,
    pub store: StateStore,
    pub dirs: Dirs,
    pub started: Instant,
    pub ready: AtomicBool,
    pub bot_id: RwLock<Option<UserId>>,
    pub bot_tag: RwLock<String>,
    pub reader: RwLock<Option<Arc<Reader>>>,
    pub reader_error: RwLock<String>,
    pub ocr_gate: Semaphore,
    /// Applicant-facing screenshot checks run on their own lane so a long background read never makes them wait.
    pub quick_gate: Semaphore,
    /// Seconds the instant check may take (0 = the default / `DCA_OCR_QUICK_SECS`); tests shorten it.
    pub quick_secs: std::sync::atomic::AtomicU64,
    pub renderer: Arc<Renderer>,
    pub web: reqwest::Client,
    pub roles_cache: Mutex<RolesCache>,
    pub collectors: Mutex<HashMap<(ChannelId, UserId), mpsc::UnboundedSender<Message>>>,
    pub apply_sessions: Mutex<HashMap<String, ApplySession>>,
    pub button_locks: Mutex<HashSet<String>>,
    pub processing: Mutex<HashSet<String>>,
    /// Gateway heartbeat latency in ms (0 until the first heartbeat was acknowledged).
    pub gateway_ms: std::sync::atomic::AtomicU64,
    me: std::sync::Weak<App>,
}

impl App {
    pub fn new(http: Arc<Http>, store: StateStore, dirs: Dirs) -> Arc<App> {
        let renderer = Arc::new(Renderer::new(&dirs.fonts));
        Arc::new_cyclic(|me| App {
            me: me.clone(),
            gateway_ms: std::sync::atomic::AtomicU64::new(0),
            http,
            store,
            started: Instant::now(),
            ready: AtomicBool::new(false),
            bot_id: RwLock::new(None),
            bot_tag: RwLock::new(String::new()),
            reader: RwLock::new(None),
            reader_error: RwLock::new(String::new()),
            ocr_gate: Semaphore::new(1),
            quick_gate: Semaphore::new(1),
            quick_secs: std::sync::atomic::AtomicU64::new(0),
            renderer,
            web: reqwest::Client::builder().timeout(Duration::from_secs(60)).user_agent("dca-bot").build().unwrap(),
            roles_cache: Mutex::new(HashMap::new()),
            collectors: Mutex::new(HashMap::new()),
            apply_sessions: Mutex::new(HashMap::new()),
            button_locks: Mutex::new(HashSet::new()),
            processing: Mutex::new(HashSet::new()),
            dirs,
        })
    }

    /// This app as an `Arc` (for work that outlives the current handler).
    pub fn arc(&self) -> Arc<App> {
        self.me.upgrade().expect("the App is alive while it is used")
    }

    pub async fn config(&self) -> DashboardConfig {
        load_config(&self.store).await
    }

    pub fn is_ready(&self) -> bool {
        self.ready.load(Ordering::Relaxed)
    }

    pub fn reader(&self) -> Option<Arc<Reader>> {
        self.reader.read().unwrap().clone()
    }

    pub fn bot_user_id(&self) -> Option<UserId> {
        *self.bot_id.read().unwrap()
    }

    /// The bot's own user id (falls back to a REST lookup before the gateway is ready).
    pub async fn ensure_bot_id(&self) -> BotResult<UserId> {
        if let Some(id) = self.bot_user_id() {
            return Ok(id);
        }
        let me = self.http.get_current_user().await?;
        *self.bot_id.write().unwrap() = Some(me.id);
        *self.bot_tag.write().unwrap() = display_tag(&me.into());
        Ok(self.bot_user_id().unwrap())
    }

    pub fn uptime_secs(&self) -> u64 {
        self.started.elapsed().as_secs()
    }

    /// Roles of a guild (cached for a minute) together with its owner.
    pub async fn guild_roles(&self, guild: GuildId) -> BotResult<(UserId, HashMap<RoleId, Role>)> {
        if let Some((at, owner, roles)) = self.roles_cache.lock().unwrap().get(&guild) {
            if at.elapsed() < Duration::from_secs(60) {
                return Ok((*owner, roles.clone()));
            }
        }
        let partial = self.http.get_guild(guild).await?;
        let owner = partial.owner_id;
        let roles: HashMap<RoleId, Role> = partial.roles.into_iter().collect();
        self.roles_cache.lock().unwrap().insert(guild, (Instant::now(), owner, roles.clone()));
        Ok((owner, roles))
    }

    /// Effective guild-level permissions of a member.
    pub async fn member_permissions(&self, guild: GuildId, user: UserId) -> Permissions {
        let Ok((owner, roles)) = self.guild_roles(guild).await else { return Permissions::empty() };
        if owner == user {
            return Permissions::all();
        }
        let Ok(member) = self.http.get_member(guild, user).await else { return Permissions::empty() };
        let mut perms = roles.get(&RoleId::new(guild.get())).map(|r| r.permissions).unwrap_or(Permissions::empty());
        for role in &member.roles {
            if let Some(r) = roles.get(role) {
                perms |= r.permissions;
            }
        }
        if perms.contains(Permissions::ADMINISTRATOR) {
            return Permissions::all();
        }
        perms
    }

    pub async fn member_has(&self, guild: GuildId, user: UserId, needed: Permissions) -> bool {
        let perms = self.member_permissions(guild, user).await;
        perms.contains(Permissions::ADMINISTRATOR) || perms.contains(needed)
    }
}
