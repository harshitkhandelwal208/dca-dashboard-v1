//! Scope-based state store with a write-through in-memory cache.
//!
//! Every scope (`dashboardConfig`, `recruitmentTickets`, ...) is one JSON document, exactly like the original
//! Node bot. Backed by Firebase when it is configured, otherwise by local JSON files.
//!
//! Because the bot and the dashboard now live in one process, the cache is authoritative and each scope has
//! its own lock: a read-modify-write (`mutate`) can never interleave with another one, which removes the
//! lost-update races the old two-process design had.

use crate::firebase::{firebase_configured, Firebase};
use serde_json::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex as StdMutex};
use tokio::sync::Mutex;

struct Slot {
    loaded: bool,
    value: Value,
}

struct Inner {
    json_dir: PathBuf,
    firebase: Option<Firebase>,
    slots: StdMutex<HashMap<String, Arc<Mutex<Slot>>>>,
}

#[derive(Clone)]
pub struct StateStore {
    inner: Arc<Inner>,
}

pub fn data_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("DCA_DATA_DIR") {
        if !dir.trim().is_empty() {
            return PathBuf::from(dir);
        }
    }
    for candidate in ["data", "bot/data", "../data", "../../data", "/app/data"] {
        let path = Path::new(candidate);
        if path.is_dir() {
            return path.to_path_buf();
        }
    }
    PathBuf::from("data")
}

fn env_override(scope: &str) -> Option<PathBuf> {
    let key = match scope {
        "dashboardConfig" => "DASHBOARD_CONFIG_PATH",
        "recruitmentTickets" => "RECRUITMENT_TICKETS_PATH",
        "recruitmentLogs" => "RECRUITMENT_LOGS_PATH",
        "recruitmentBans" => "RECRUITMENT_BANS_PATH",
        "botLogs" => "BOT_LOGS_PATH",
        "warnings" => "WARNINGS_PATH",
        _ => return None,
    };
    std::env::var(key).ok().filter(|v| !v.trim().is_empty()).map(PathBuf::from)
}

impl StateStore {
    /// Firebase when configured (falling back to local files if its settings are unusable), otherwise local files.
    pub fn from_env() -> StateStore {
        let firebase = if firebase_configured() {
            match Firebase::from_env() {
                Ok(fb) => {
                    println!(
                        "[state] Using Firebase {:?} (project '{}', {}) for persistent state.",
                        fb.kind,
                        fb.project_id,
                        if fb.kind == crate::firebase::DatabaseKind::Firestore { &fb.collection } else { &fb.root }
                    );
                    Some(fb)
                }
                Err(error) => {
                    eprintln!("[state] Firebase is configured but unusable ({error}); falling back to local JSON files.");
                    None
                }
            }
        } else {
            println!("[state] Firebase is not configured; state is kept in local JSON files under {}.", data_dir().display());
            None
        };
        StateStore::with_backend(data_dir(), firebase)
    }

    pub fn local(dir: impl Into<PathBuf>) -> StateStore {
        StateStore::with_backend(dir.into(), None)
    }

    fn with_backend(json_dir: PathBuf, firebase: Option<Firebase>) -> StateStore {
        StateStore { inner: Arc::new(Inner { json_dir, firebase, slots: StdMutex::new(HashMap::new()) }) }
    }

    pub fn uses_firebase(&self) -> bool {
        self.inner.firebase.is_some()
    }

    pub fn dir(&self) -> &Path {
        &self.inner.json_dir
    }

    fn slot(&self, scope: &str) -> Arc<Mutex<Slot>> {
        let mut slots = self.inner.slots.lock().unwrap();
        slots.entry(scope.to_string()).or_insert_with(|| Arc::new(Mutex::new(Slot { loaded: false, value: Value::Null }))).clone()
    }

    fn json_path(&self, scope: &str) -> PathBuf {
        env_override(scope).unwrap_or_else(|| self.inner.json_dir.join(format!("{scope}.json")))
    }

    async fn load(&self, scope: &str) -> Result<Value, String> {
        if let Some(firebase) = &self.inner.firebase {
            return Ok(firebase.read(scope).await?.unwrap_or(Value::Null));
        }

        let path = self.json_path(scope);
        let content = match tokio::fs::read_to_string(&path).await {
            Ok(content) => content,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Value::Null),
            Err(error) => return Err(format!("cannot read {}: {error}", path.display())),
        };
        if content.trim().is_empty() {
            return Ok(Value::Null);
        }
        match serde_json::from_str::<Value>(&content) {
            Ok(value) => Ok(value),
            Err(error) => {
                // Keep the unreadable file for inspection instead of silently overwriting it.
                let damaged = path.with_extension("json.damaged");
                let _ = tokio::fs::write(&damaged, &content).await;
                eprintln!("[state] {} is damaged ({error}); a copy was kept at {}", path.display(), damaged.display());
                Ok(Value::Null)
            }
        }
    }

    async fn persist(&self, scope: &str, value: &Value) -> Result<(), String> {
        if let Some(firebase) = &self.inner.firebase {
            return firebase.write(scope, value).await;
        }

        let path = self.json_path(scope);
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await.map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
        }
        let tmp = path.with_extension(format!("json.{}.tmp", std::process::id()));
        let text = serde_json::to_string_pretty(value).map_err(|e| e.to_string())? + "\n";
        tokio::fs::write(&tmp, text).await.map_err(|e| format!("cannot write {}: {e}", tmp.display()))?;
        tokio::fs::rename(&tmp, &path).await.map_err(|e| format!("cannot replace {}: {e}", path.display()))?;
        Ok(())
    }

    /// Current value of a scope (cloned), or `fallback` when it was never written or could not be loaded.
    pub async fn read(&self, scope: &str, fallback: Value) -> Value {
        let slot = self.slot(scope);
        let mut guard = slot.lock().await;
        if !guard.loaded {
            match self.load(scope).await {
                Ok(value) => {
                    guard.value = value;
                    guard.loaded = true;
                }
                Err(error) => {
                    eprintln!("[state] Failed to read {scope}: {error}");
                    return fallback;
                }
            }
        }
        if guard.value.is_null() {
            fallback
        } else {
            guard.value.clone()
        }
    }

    /// Replace a scope's document.
    pub async fn write(&self, scope: &str, value: Value) -> Result<(), String> {
        let slot = self.slot(scope);
        let mut guard = slot.lock().await;
        self.persist(scope, &value).await?;
        guard.value = value;
        guard.loaded = true;
        Ok(())
    }

    /// Atomic read-modify-write. A scope that cannot be loaded is never overwritten.
    pub async fn mutate<R>(&self, scope: &str, fallback: Value, f: impl FnOnce(&mut Value) -> R) -> Result<R, String> {
        let slot = self.slot(scope);
        let mut guard = slot.lock().await;
        if !guard.loaded {
            let value = self.load(scope).await.map_err(|e| format!("refusing to modify {scope}: {e}"))?;
            guard.value = value;
            guard.loaded = true;
        }
        let previous = if guard.value.is_null() { fallback } else { guard.value.clone() };
        let mut next = previous.clone();
        let result = f(&mut next);
        if next == previous && !guard.value.is_null() {
            return Ok(result);
        }
        self.persist(scope, &next).await?;
        guard.value = next;
        Ok(result)
    }
}
