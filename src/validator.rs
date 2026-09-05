// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (c) 2026 Faiz

use crate::config::Config;
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;
use tracing::{debug, error, info, warn};

pub fn start_token_validator(http_client: Arc<wreq::Client>, config: Arc<Config>) {
    let validator_cfg = match config.token_validator.as_ref() {
        Some(cfg) if cfg.enabled => cfg,
        _ => {
            debug!("token validator is disabled in config");
            return;
        }
    };

    let interval_hours = if validator_cfg.interval_hours > 0.0 {
        validator_cfg.interval_hours
    } else {
        2.0
    };

    let interval_secs = (interval_hours * 3600.0) as u64;
    info!("starting token health monitor (interval: {interval_hours:.1}h)");

    tokio::spawn(async move {
        // initial grace period after startup
        tokio::time::sleep(Duration::from_secs(10)).await;

        let mut ticker = tokio::time::interval(Duration::from_secs(interval_secs));

        loop {
            ticker.tick().await;
            validate_token(&http_client, &config).await;
        }
    });
}

async fn validate_token(http_client: &wreq::Client, config: &Config) {
    debug!("executing periodic token health validation");

    let url = "https://discord.com/api/v9/users/@me";
    let resp = match http_client.get(url).send().await {
        Ok(r) => r,
        Err(e) => {
            warn!("token health check network request failed: {e}");
            return;
        }
    };

    let status = resp.status().as_u16();

    if status == 200 {
        debug!("token health check passed: account is active and authenticated");
    } else if status == 401 {
        error!("CRITICAL: discord account token is invalid or has expired (status 401)");

        let webhook_url = config
            .token_validator
            .as_ref()
            .and_then(|v| v.webhook_url.as_deref())
            .filter(|s| !s.trim().is_empty())
            .or_else(|| {
                config
                    .logging
                    .as_ref()
                    .and_then(|l| l.webhook_url.as_deref())
            });

        if let Some(url) = webhook_url {
            send_token_alert(
                url,
                config,
                "Token is invalid or expired (401 Unauthorized)",
            )
            .await;
        }
    } else {
        debug!("token health check returned status {status}");
    }
}

async fn send_token_alert(webhook_url: &str, config: &Config, reason: &str) {
    let url = webhook_url.trim();
    if url.is_empty() {
        return;
    }

    let managers = config
        .security
        .as_ref()
        .map(|s| s.managers.as_slice())
        .unwrap_or(&[]);

    let mentions = if managers.is_empty() {
        String::new()
    } else {
        managers
            .iter()
            .map(|id| format!("<@{id}>"))
            .collect::<Vec<_>>()
            .join(" ")
    };

    let now_ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let description = format!(
        "### 🚨 Critical Token Health Alert\n\n\
        **Status:** Token Authentication Failed\n\
        **Reason:** {reason}\n\
        **Server ID:** `{}`\n\
        **Timestamp:** <t:{now_ts}:F>\n\n\
        ⚠️ **Action Required:** Please update the Discord token in `config.toml` immediately.",
        config.selfbot.server_id
    );

    let payload = json!({
        "content": if mentions.is_empty() { serde_json::Value::Null } else { json!(mentions) },
        "embeds": [{
            "title": "🚨 Discord Account Token Failure",
            "description": description,
            "color": 0xe74c3c, // red
            "footer": {
                "text": "DVP • Token Health Monitor"
            }
        }]
    });

    let client = match wreq::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            debug!("failed to create token alert webhook client: {e}");
            return;
        }
    };

    let _ = client
        .post(url)
        .header("Content-Type", "application/json")
        .body(payload.to_string())
        .send()
        .await;
}
