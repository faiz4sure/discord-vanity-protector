// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (c) 2026 Faiz

use crate::totp::TotpKey;
use anyhow::Result;
use serde::Deserialize;
use serde_json::json;
use std::collections::HashMap;
use std::sync::{LazyLock, RwLock};
use std::time::{Duration, Instant};
use tracing::{debug, error, info, warn};

#[derive(Deserialize)]
struct MfaChallengeBody {
    code: Option<u32>,
    mfa: Option<MfaRequest>,
}

#[derive(Deserialize)]
struct MfaRequest {
    ticket: String,
    #[serde(default)]
    methods: Vec<MfaMethod>,
}

#[derive(Deserialize)]
struct MfaMethod {
    #[serde(rename = "type")]
    kind: String,
}

#[derive(Deserialize)]
struct MfaFinishResponse {
    token: String,
}

pub fn select_mfa_method(
    methods: &[&str],
    totp_key: Option<&TotpKey>,
    password: Option<&str>,
) -> Option<(&'static str, String)> {
    let offers_totp = methods.contains(&"totp");
    let offers_password = methods.contains(&"password");
    let pwd = password.filter(|p| !p.trim().is_empty());

    if let Some(key) = totp_key.filter(|_| offers_totp || methods.is_empty()) {
        Some(("totp", key.generate()))
    } else if let Some(p) = pwd.filter(|_| offers_password || !offers_totp) {
        Some(("password", p.to_string()))
    } else if let Some(key) = totp_key {
        Some(("totp", key.generate()))
    } else {
        pwd.map(|p| ("password", p.to_string()))
    }
}

struct CacheEntry {
    token: String,
    expires_at: Instant,
}

pub struct MfaCache {
    entries: RwLock<HashMap<String, CacheEntry>>,
    ttl: Duration,
}

impl MfaCache {
    pub fn new() -> Self {
        Self {
            entries: RwLock::new(HashMap::new()),
            ttl: Duration::from_secs(270),
        }
    }
}

impl Default for MfaCache {
    fn default() -> Self {
        Self::new()
    }
}

impl MfaCache {
    pub fn get(&self, guild_id: &str) -> Option<String> {
        let read = self.entries.read().ok()?;
        if let Some(entry) = read.get(guild_id).filter(|e| Instant::now() < e.expires_at) {
            return Some(entry.token.clone());
        }
        None
    }

    pub fn set(&self, guild_id: &str, token: &str) {
        if let Ok(mut write) = self.entries.write() {
            write.insert(
                guild_id.to_string(),
                CacheEntry {
                    token: token.to_string(),
                    expires_at: Instant::now() + self.ttl,
                },
            );
        }
    }

    pub fn invalidate(&self, guild_id: &str) {
        if let Ok(mut write) = self.entries.write() {
            write.remove(guild_id);
        }
    }
}

static MFA_CACHE: LazyLock<MfaCache> = LazyLock::new(MfaCache::new);

fn delay(resp: &wreq::Response) -> f64 {
    resp.headers()
        .get("Retry-After")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse::<f64>().ok())
        .unwrap_or(1.0)
        .min(5.0)
}

pub async fn revert_vanity(
    http_client: &wreq::Client,
    guild_id: &str,
    target_code: &str,
    totp_key: Option<&TotpKey>,
    password: Option<&str>,
) -> Result<bool> {
    let start = Instant::now();
    let url = format!("https://discord.com/api/v9/guilds/{guild_id}/vanity-url");
    let payload = json!({ "code": target_code });

    debug!("dispatching vanity revert request for guild {guild_id}: code={target_code}");

    // fast path: check if cached mfa token is valid
    if let Some(cached_token) = MFA_CACHE.get(guild_id) {
        debug!("reusing cached 270s mfa token for guild {guild_id}");
        let mut cached_resp = http_client
            .patch(&url)
            .header("Content-Type", "application/json")
            .header("X-Discord-MFA-Authorization", &cached_token)
            .body(payload.to_string())
            .send()
            .await?;

        let mut cached_status = cached_resp.status();
        if cached_status.as_u16() == 429 {
            let wait = delay(&cached_resp);
            warn!("rate limited (429) on cached mfa revert, retrying in {wait:.1}s");
            tokio::time::sleep(Duration::from_secs_f64(wait)).await;

            cached_resp = http_client
                .patch(&url)
                .header("Content-Type", "application/json")
                .header("X-Discord-MFA-Authorization", &cached_token)
                .body(payload.to_string())
                .send()
                .await?;
            cached_status = cached_resp.status();
        }

        if cached_status.is_success() {
            let elapsed = start.elapsed().as_millis();
            info!("vanity successfully reverted to '{target_code}' using cached mfa [{elapsed}ms]");
            return Ok(true);
        } else if cached_status.as_u16() != 429 {
            debug!("cached mfa token invalid or expired, clearing from cache");
            MFA_CACHE.invalidate(guild_id);
        }
    }

    let mut resp = http_client
        .patch(&url)
        .header("Content-Type", "application/json")
        .body(payload.to_string())
        .send()
        .await?;

    let mut status = resp.status();
    if status.as_u16() == 429 {
        let wait = delay(&resp);
        warn!("rate limited (429) on vanity revert, retrying in {wait:.1}s");
        tokio::time::sleep(Duration::from_secs_f64(wait)).await;

        resp = http_client
            .patch(&url)
            .header("Content-Type", "application/json")
            .body(payload.to_string())
            .send()
            .await?;
        status = resp.status();
    }

    if status.is_success() {
        let elapsed = start.elapsed().as_millis();
        info!("vanity successfully reverted to '{target_code}' [{elapsed}ms]");
        return Ok(true);
    }

    if status.as_u16() == 401 {
        let body_text = resp.text().await.unwrap_or_default();
        let parsed: MfaChallengeBody = match serde_json::from_str(&body_text) {
            Ok(p) => p,
            Err(_) => {
                error!("vanity revert 401 unauthorized without valid mfa payload");
                return Ok(false);
            }
        };

        if parsed.code != Some(60003) {
            error!(
                "vanity revert 401 unauthorized is not an mfa challenge (code={:?})",
                parsed.code
            );
            return Ok(false);
        }

        let mfa = match parsed.mfa {
            Some(m) if !m.ticket.is_empty() => m,
            _ => {
                error!("vanity revert 401 unauthorized without mfa ticket");
                return Ok(false);
            }
        };

        let method_kinds: Vec<&str> = mfa.methods.iter().map(|m| m.kind.as_str()).collect();
        let (mfa_type, mfa_data) = match select_mfa_method(&method_kinds, totp_key, password) {
            Some(pair) => pair,
            None => {
                warn!(
                    "mfa required (methods={method_kinds:?}), but neither totp_secret nor password is configured in config.toml"
                );
                return Ok(false);
            }
        };

        debug!("mfa ticket received, authenticating with {mfa_type}");

        let mfa_payload = json!({
            "ticket": mfa.ticket,
            "mfa_type": mfa_type,
            "data": mfa_data
        });

        let mfa_resp = http_client
            .post("https://discord.com/api/v9/mfa/finish")
            .header("Content-Type", "application/json")
            .body(mfa_payload.to_string())
            .send()
            .await?;

        let mfa_status = mfa_resp.status();
        if !mfa_status.is_success() {
            let err_body = mfa_resp.text().await.unwrap_or_default();
            error!("failed to finish mfa authorization: status={mfa_status} body={err_body}");
            if mfa_type == "password" && err_body.contains("60008") {
                warn!(
                    "discord rejected password mfa with code 60008; switch to totp_secret in [vanity] (see docs/2FA.md)"
                );
            }
            return Ok(false);
        }

        let mfa_body = match mfa_resp.json::<MfaFinishResponse>().await {
            Ok(b) if !b.token.is_empty() => b,
            _ => {
                error!("mfa finish response did not contain token");
                return Ok(false);
            }
        };
        let mfa_token = mfa_body.token.as_str();

        // cache acquired mfa token for 270 seconds
        MFA_CACHE.set(guild_id, mfa_token);
        debug!("mfa token cached for 270s, retrying vanity revert");

        let mut retry_resp = http_client
            .patch(&url)
            .header("Content-Type", "application/json")
            .header("X-Discord-MFA-Authorization", mfa_token)
            .body(payload.to_string())
            .send()
            .await?;

        let mut retry_status = retry_resp.status();
        if retry_status.as_u16() == 429 {
            let wait = delay(&retry_resp);
            warn!("rate limited (429) on mfa vanity revert, retrying in {wait:.1}s");
            tokio::time::sleep(Duration::from_secs_f64(wait)).await;

            retry_resp = http_client
                .patch(&url)
                .header("Content-Type", "application/json")
                .header("X-Discord-MFA-Authorization", mfa_token)
                .body(payload.to_string())
                .send()
                .await?;
            retry_status = retry_resp.status();
        }

        if retry_status.is_success() {
            let elapsed = start.elapsed().as_millis();
            info!("vanity successfully reverted to '{target_code}' with mfa [{elapsed}ms]");
            return Ok(true);
        } else {
            let err = retry_resp.text().await.unwrap_or_default();
            error!("vanity revert with mfa failed: status={retry_status} body={err}");
            return Ok(false);
        }
    }

    let err_text = resp.text().await.unwrap_or_default();
    error!("vanity revert failed: status={status} body={err_text}");
    Ok(false)
}
