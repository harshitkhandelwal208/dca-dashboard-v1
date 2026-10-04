//! Firebase persistence over REST (Cloud Firestore or Realtime Database).
//!
//! The on-disk layout is the same one the original Node bot used, so existing production data keeps working:
//!
//! * Firestore: document `<collection>/<scope>` holding `{ data, updatedAt }`
//! * Realtime Database: node `<root>/<scope>` holding `{ data, updatedAt }`
//!
//! Authentication uses a Google service account (raw JSON or base64 JSON in `FIREBASE_SERVICE_ACCOUNT`, a file
//! in `FIREBASE_SERVICE_ACCOUNT_PATH` / `GOOGLE_APPLICATION_CREDENTIALS`) or the GCP metadata server.

use base64::Engine;
use serde::Deserialize;
use serde_json::{json, Map, Value};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

const TOKEN_SCOPES: &str = "https://www.googleapis.com/auth/datastore https://www.googleapis.com/auth/firebase.database https://www.googleapis.com/auth/userinfo.email https://www.googleapis.com/auth/cloud-platform";

#[derive(Clone, Debug, Deserialize)]
struct ServiceAccount {
    client_email: String,
    private_key: String,
    #[serde(default)]
    project_id: String,
    #[serde(default)]
    token_uri: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DatabaseKind {
    Firestore,
    Realtime,
}

struct CachedToken {
    value: String,
    expires_at: Instant,
}

#[derive(Clone)]
pub struct Firebase {
    http: reqwest::Client,
    account: Option<Arc<ServiceAccount>>,
    token: Arc<Mutex<Option<CachedToken>>>,
    pub kind: DatabaseKind,
    pub project_id: String,
    pub collection: String,
    pub root: String,
    pub database_url: String,
}

pub fn firebase_configured() -> bool {
    ["FIREBASE_SERVICE_ACCOUNT", "FIREBASE_SERVICE_ACCOUNT_PATH", "GOOGLE_APPLICATION_CREDENTIALS", "FIREBASE_PROJECT_ID"]
        .iter()
        .any(|key| std::env::var(key).map(|v| !v.trim().is_empty()).unwrap_or(false))
}

fn env_nonempty(key: &str) -> Option<String> {
    std::env::var(key).ok().map(|v| v.trim().to_string()).filter(|v| !v.is_empty())
}

fn parse_service_account() -> Result<Option<ServiceAccount>, String> {
    let raw = if let Some(path) = env_nonempty("FIREBASE_SERVICE_ACCOUNT_PATH") {
        Some(std::fs::read_to_string(&path).map_err(|e| format!("cannot read FIREBASE_SERVICE_ACCOUNT_PATH ({path}): {e}"))?)
    } else if let Some(inline) = env_nonempty("FIREBASE_SERVICE_ACCOUNT") {
        if inline.starts_with('{') {
            Some(inline)
        } else {
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(inline.as_bytes())
                .or_else(|_| base64::engine::general_purpose::URL_SAFE.decode(inline.as_bytes()))
                .map_err(|e| format!("FIREBASE_SERVICE_ACCOUNT is neither JSON nor base64: {e}"))?;
            Some(String::from_utf8_lossy(&bytes).into_owned())
        }
    } else if let Some(path) = env_nonempty("GOOGLE_APPLICATION_CREDENTIALS") {
        std::fs::read_to_string(&path).ok()
    } else {
        None
    };

    match raw {
        None => Ok(None),
        Some(text) => {
            let mut account: ServiceAccount =
                serde_json::from_str(&text).map_err(|e| format!("invalid service account JSON: {e}"))?;
            // Keys pasted through env dashboards frequently carry literal "\n" sequences.
            account.private_key = account.private_key.replace("\\n", "\n");
            Ok(Some(account))
        }
    }
}

impl Firebase {
    pub fn from_env() -> Result<Firebase, String> {
        let account = parse_service_account()?;
        let project_id = env_nonempty("FIREBASE_PROJECT_ID")
            .or_else(|| account.as_ref().map(|a| a.project_id.clone()).filter(|p| !p.is_empty()))
            .unwrap_or_default();

        let forced = env_nonempty("FIREBASE_DATABASE_TYPE").unwrap_or_default().to_lowercase();
        let database_url = env_nonempty("FIREBASE_DATABASE_URL").unwrap_or_default();
        let kind = if ["realtime", "rtdb", "database"].contains(&forced.as_str()) {
            DatabaseKind::Realtime
        } else if ["firestore", "cloud-firestore"].contains(&forced.as_str()) {
            DatabaseKind::Firestore
        } else if !database_url.is_empty() {
            DatabaseKind::Realtime
        } else {
            DatabaseKind::Firestore
        };

        if kind == DatabaseKind::Firestore && project_id.is_empty() {
            return Err("Firestore needs FIREBASE_PROJECT_ID (or a service account that carries project_id).".into());
        }
        if kind == DatabaseKind::Realtime && database_url.is_empty() {
            return Err("Realtime Database needs FIREBASE_DATABASE_URL.".into());
        }

        let collection = env_nonempty("FIREBASE_STATE_COLLECTION").unwrap_or_else(|| "dca_bot_state".to_string());
        let root = env_nonempty("FIREBASE_STATE_ROOT").unwrap_or_else(|| collection.clone());

        Ok(Firebase {
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(30))
                .build()
                .map_err(|e| e.to_string())?,
            account: account.map(Arc::new),
            token: Arc::new(Mutex::new(None)),
            kind,
            project_id,
            collection,
            root,
            database_url: database_url.trim_end_matches('/').to_string(),
        })
    }

    async fn access_token(&self) -> Result<String, String> {
        let mut guard = self.token.lock().await;
        if let Some(cached) = guard.as_ref() {
            if cached.expires_at > Instant::now() + Duration::from_secs(60) {
                return Ok(cached.value.clone());
            }
        }

        let (value, ttl) = match &self.account {
            Some(account) => self.exchange_service_account(account).await?,
            None => self.metadata_token().await?,
        };
        *guard = Some(CachedToken {
            value: value.clone(),
            expires_at: Instant::now() + Duration::from_secs(ttl.max(120)),
        });
        Ok(value)
    }

    async fn exchange_service_account(&self, account: &ServiceAccount) -> Result<(String, u64), String> {
        let token_uri = if account.token_uri.is_empty() { "https://oauth2.googleapis.com/token" } else { account.token_uri.as_str() };
        let now = chrono::Utc::now().timestamp();
        let claims = json!({
            "iss": account.client_email,
            "scope": TOKEN_SCOPES,
            "aud": token_uri,
            "iat": now,
            "exp": now + 3600,
        });
        let key = jsonwebtoken::EncodingKey::from_rsa_pem(account.private_key.as_bytes())
            .map_err(|e| format!("service account private key is not valid RSA PEM: {e}"))?;
        let assertion = jsonwebtoken::encode(&jsonwebtoken::Header::new(jsonwebtoken::Algorithm::RS256), &claims, &key)
            .map_err(|e| format!("could not sign the Google token request: {e}"))?;

        let response = self
            .http
            .post(token_uri)
            .form(&[("grant_type", "urn:ietf:params:oauth:grant-type:jwt-bearer"), ("assertion", assertion.as_str())])
            .send()
            .await
            .map_err(|e| format!("Google token request failed: {e}"))?;
        let status = response.status();
        let body: Value = response.json().await.map_err(|e| format!("Google token response unreadable: {e}"))?;
        if !status.is_success() {
            return Err(format!("Google token request returned {status}: {body}"));
        }
        let token = body["access_token"].as_str().ok_or("Google did not return an access token")?.to_string();
        Ok((token, body["expires_in"].as_u64().unwrap_or(3000)))
    }

    async fn metadata_token(&self) -> Result<(String, u64), String> {
        let response = self
            .http
            .get("http://metadata.google.internal/computeMetadata/v1/instance/service-accounts/default/token")
            .header("Metadata-Flavor", "Google")
            .timeout(Duration::from_secs(5))
            .send()
            .await
            .map_err(|e| format!("no Firebase credentials configured and the GCP metadata server is unreachable: {e}"))?;
        let body: Value = response.json().await.map_err(|e| e.to_string())?;
        let token = body["access_token"].as_str().ok_or("metadata server returned no access token")?.to_string();
        Ok((token, body["expires_in"].as_u64().unwrap_or(1800)))
    }

    fn firestore_url(&self, scope: &str) -> String {
        format!(
            "https://firestore.googleapis.com/v1/projects/{}/databases/(default)/documents/{}/{}",
            self.project_id,
            urlencode(&self.collection),
            urlencode(scope)
        )
    }

    fn realtime_url(&self, scope: &str) -> String {
        format!("{}/{}/{}.json", self.database_url, self.root.trim_matches('/'), scope)
    }

    /// Returns the stored `data` payload for a scope, or `None` when nothing was ever written.
    pub async fn read(&self, scope: &str) -> Result<Option<Value>, String> {
        let token = self.access_token().await?;
        match self.kind {
            DatabaseKind::Firestore => {
                let response = self
                    .http
                    .get(self.firestore_url(scope))
                    .bearer_auth(&token)
                    .send()
                    .await
                    .map_err(|e| format!("Firestore read failed: {e}"))?;
                if response.status().as_u16() == 404 {
                    return Ok(None);
                }
                let status = response.status();
                let body: Value = response.json().await.map_err(|e| e.to_string())?;
                if !status.is_success() {
                    return Err(format!("Firestore read returned {status}: {body}"));
                }
                Ok(body["fields"]["data"].as_object().map(|_| from_firestore(&body["fields"]["data"])))
            }
            DatabaseKind::Realtime => {
                let response = self
                    .http
                    .get(self.realtime_url(scope))
                    .bearer_auth(&token)
                    .send()
                    .await
                    .map_err(|e| format!("Realtime Database read failed: {e}"))?;
                let status = response.status();
                let body: Value = response.json().await.map_err(|e| e.to_string())?;
                if !status.is_success() {
                    return Err(format!("Realtime Database read returned {status}: {body}"));
                }
                Ok(body.get("data").filter(|v| !v.is_null()).cloned())
            }
        }
    }

    pub async fn write(&self, scope: &str, data: &Value) -> Result<(), String> {
        let token = self.access_token().await?;
        let updated_at = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        match self.kind {
            DatabaseKind::Firestore => {
                let body = json!({ "fields": { "data": to_firestore(data), "updatedAt": { "stringValue": updated_at } } });
                let response = self
                    .http
                    .patch(self.firestore_url(scope))
                    .query(&[("updateMask.fieldPaths", "data"), ("updateMask.fieldPaths", "updatedAt")])
                    .bearer_auth(&token)
                    .json(&body)
                    .send()
                    .await
                    .map_err(|e| format!("Firestore write failed: {e}"))?;
                if !response.status().is_success() {
                    let status = response.status();
                    let text = response.text().await.unwrap_or_default();
                    return Err(format!("Firestore write returned {status}: {text}"));
                }
                Ok(())
            }
            DatabaseKind::Realtime => {
                let response = self
                    .http
                    .put(self.realtime_url(scope))
                    .bearer_auth(&token)
                    .json(&json!({ "data": data, "updatedAt": updated_at }))
                    .send()
                    .await
                    .map_err(|e| format!("Realtime Database write failed: {e}"))?;
                if !response.status().is_success() {
                    let status = response.status();
                    let text = response.text().await.unwrap_or_default();
                    return Err(format!("Realtime Database write returned {status}: {text}"));
                }
                Ok(())
            }
        }
    }
}

fn urlencode(value: &str) -> String {
    let mut out = String::new();
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(byte as char),
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// JSON -> Firestore typed value.
pub fn to_firestore(value: &Value) -> Value {
    match value {
        Value::Null => json!({ "nullValue": null }),
        Value::Bool(b) => json!({ "booleanValue": b }),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                json!({ "integerValue": i.to_string() })
            } else if let Some(u) = n.as_u64() {
                json!({ "integerValue": u.to_string() })
            } else {
                json!({ "doubleValue": n.as_f64().unwrap_or(0.0) })
            }
        }
        Value::String(s) => json!({ "stringValue": s }),
        Value::Array(items) => {
            if items.is_empty() {
                json!({ "arrayValue": {} })
            } else {
                json!({ "arrayValue": { "values": items.iter().map(to_firestore).collect::<Vec<_>>() } })
            }
        }
        Value::Object(map) => {
            let mut fields = Map::new();
            for (key, item) in map {
                if key.is_empty() {
                    continue;
                }
                fields.insert(key.clone(), to_firestore(item));
            }
            json!({ "mapValue": { "fields": fields } })
        }
    }
}

/// Firestore typed value -> JSON.
pub fn from_firestore(value: &Value) -> Value {
    let Some(object) = value.as_object() else { return Value::Null };
    if let Some(s) = object.get("stringValue") {
        return s.clone();
    }
    if let Some(i) = object.get("integerValue") {
        return match i {
            Value::String(text) => text.parse::<i64>().map(Value::from).unwrap_or(Value::Null),
            other => other.clone(),
        };
    }
    if let Some(d) = object.get("doubleValue") {
        return match d {
            Value::String(text) => text.parse::<f64>().ok().and_then(|f| serde_json::Number::from_f64(f)).map(Value::Number).unwrap_or(Value::Null),
            other => other.clone(),
        };
    }
    if let Some(b) = object.get("booleanValue") {
        return b.clone();
    }
    if object.contains_key("nullValue") {
        return Value::Null;
    }
    if let Some(array) = object.get("arrayValue") {
        return Value::Array(
            array["values"].as_array().map(|items| items.iter().map(from_firestore).collect()).unwrap_or_default(),
        );
    }
    if let Some(map) = object.get("mapValue") {
        let mut out = Map::new();
        if let Some(fields) = map["fields"].as_object() {
            for (key, item) in fields {
                out.insert(key.clone(), from_firestore(item));
            }
        }
        return Value::Object(out);
    }
    for key in ["timestampValue", "referenceValue", "bytesValue"] {
        if let Some(s) = object.get(key) {
            return s.clone();
        }
    }
    if let Some(geo) = object.get("geoPointValue") {
        return geo.clone();
    }
    Value::Null
}
