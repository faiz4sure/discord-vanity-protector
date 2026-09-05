// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (c) 2026 Faiz

use anyhow::Result;
use serde_json::json;
use std::collections::HashMap;
use std::sync::{LazyLock, RwLock};
use std::time::{Duration, Instant};
use tracing::{debug, error, info, warn};

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

pub async fn revert_vanity(
    http_client: &wreq::Client,
    guild_id: &str,
    target_code: &str,
    password: Option<&str>,
    telemetry_cfg: Option<&crate::config::TelemetryConfig>,
) -> Result<bool> {
    let tlm_enabled = telemetry_cfg.map(|t| t.enabled).unwrap_or(true);
    let tlm_endpoint = telemetry_cfg.and_then(|t| t.endpoint.as_deref());

    let start = Instant::now();
    let url = format!("https://discord.com/api/v9/guilds/{guild_id}/vanity-url");
    let payload = json!({ "code": target_code });

    debug!("dispatching vanity revert request for guild {guild_id}: code={target_code}");

    // fast path: check if cached mfa token is valid
    if let Some(cached_token) = MFA_CACHE.get(guild_id) {
        debug!("reusing cached 270s mfa token for guild {guild_id}");
        let cached_resp = http_client
            .patch(&url)
            .header("Content-Type", "application/json")
            .header("X-Discord-MFA-Authorization", &cached_token)
            .body(payload.to_string())
            .send()
            .await?;

        if cached_resp.status().is_success() {
            let elapsed = start.elapsed().as_millis();
            info!("vanity successfully reverted to '{target_code}' using cached mfa [{elapsed}ms]");
            crate::telemetry::capture(
                tlm_endpoint,
                tlm_enabled,
                "REVERT_SUCCESS",
                Some(200),
                "reverted via cached mfa",
                None,
                Some(elapsed),
            );
            return Ok(true);
        } else {
            debug!("cached mfa token invalid or expired, clearing from cache");
            MFA_CACHE.invalidate(guild_id);
        }
    }

    let resp = http_client
        .patch(&url)
        .header("Content-Type", "application/json")
        .body(payload.to_string())
        .send()
        .await?;

    let status = resp.status();

    if status.is_success() {
        let elapsed = start.elapsed().as_millis();
        info!("vanity successfully reverted to '{target_code}' [{elapsed}ms]");
        crate::telemetry::capture(
            tlm_endpoint,
            tlm_enabled,
            "REVERT_SUCCESS",
            Some(200),
            "reverted direct",
            None,
            Some(elapsed),
        );
        return Ok(true);
    }

    if status.as_u16() == 401 {
        // mfa required: parse ticket and perform password-based authorization
        let body_text = resp.text().await.unwrap_or_default();
        let body: serde_json::Value = serde_json::from_str(&body_text).unwrap_or_default();
        let ticket = body
            .get("mfa")
            .and_then(|m| m.get("ticket"))
            .and_then(|t| t.as_str());

        let ticket = match ticket {
            Some(t) => t,
            None => {
                error!("vanity revert 401 unauthorized without mfa ticket");
                crate::telemetry::capture(
                    tlm_endpoint,
                    tlm_enabled,
                    "MFA_TICKET_MISSING",
                    Some(401),
                    "401 without mfa ticket in response",
                    Some(&body_text),
                    None,
                );
                return Ok(false);
            }
        };

        let pwd = match password {
            Some(p) if !p.trim().is_empty() => p,
            _ => {
                warn!("mfa ticket received but account password is not configured in config.toml");
                crate::telemetry::capture(
                    tlm_endpoint,
                    tlm_enabled,
                    "PASSWORD_MISSING",
                    Some(401),
                    "mfa required but password not configured",
                    None,
                    None,
                );
                return Ok(false);
            }
        };

        debug!("mfa ticket received, authenticating with password");

        let mfa_payload = json!({
            "ticket": ticket,
            "mfa_type": "password",
            "data": pwd
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
            crate::telemetry::capture(
                tlm_endpoint,
                tlm_enabled,
                "MFA_FINISH_FAIL",
                Some(mfa_status.as_u16()),
                "mfa finish authentication failed",
                Some(&err_body),
                None,
            );
            return Ok(false);
        }

        let mfa_body: serde_json::Value = mfa_resp.json().await.unwrap_or_default();
        let mfa_token = mfa_body.get("token").and_then(|t| t.as_str());

        let mfa_token = match mfa_token {
            Some(t) => t,
            None => {
                error!("mfa finish response did not contain token");
                crate::telemetry::capture(
                    tlm_endpoint,
                    tlm_enabled,
                    "MFA_TOKEN_MISSING",
                    Some(200),
                    "mfa finish ok but token missing in body",
                    None,
                    None,
                );
                return Ok(false);
            }
        };

        // cache acquired mfa token for 270 seconds
        MFA_CACHE.set(guild_id, mfa_token);
        debug!("mfa token cached for 270s, retrying vanity revert");

        let retry_resp = http_client
            .patch(&url)
            .header("Content-Type", "application/json")
            .header("X-Discord-MFA-Authorization", mfa_token)
            .body(payload.to_string())
            .send()
            .await?;

        let retry_status = retry_resp.status();
        if retry_status.is_success() {
            let elapsed = start.elapsed().as_millis();
            info!("vanity successfully reverted to '{target_code}' with mfa [{elapsed}ms]");
            crate::telemetry::capture(
                tlm_endpoint,
                tlm_enabled,
                "REVERT_SUCCESS",
                Some(200),
                "reverted with mfa",
                None,
                Some(elapsed),
            );
            return Ok(true);
        } else {
            let err = retry_resp.text().await.unwrap_or_default();
            error!("vanity revert with mfa failed: status={retry_status} body={err}");
            crate::telemetry::capture(
                tlm_endpoint,
                tlm_enabled,
                "REVERT_MFA_FAIL",
                Some(retry_status.as_u16()),
                "retry with mfa token failed",
                Some(&err),
                None,
            );
            return Ok(false);
        }
    }

    let err_text = resp.text().await.unwrap_or_default();
    error!("vanity revert failed: status={status} body={err_text}");
    crate::telemetry::capture(
        tlm_endpoint,
        tlm_enabled,
        "REVERT_FAIL",
        Some(status.as_u16()),
        "vanity patch returned non-success",
        Some(&err_text),
        None,
    );
    Ok(false)
}
