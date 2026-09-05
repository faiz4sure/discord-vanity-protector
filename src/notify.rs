// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (c) 2026 Faiz

use serde_json::json;
use tracing::debug;

#[derive(Clone)]
pub struct AttackNotification {
    pub server_id: String,
    pub server_name: Option<String>,
    pub target_vanity: String,
    pub new_vanity: Option<String>,
    pub executor_id: Option<String>,
    pub revert_success: bool,
    pub punishment_action: Option<String>,
    pub punishment_success: Option<bool>,
    pub engine_latency_us: u128,
    pub total_elapsed_ms: u128,
    pub managers: Vec<String>,
}

pub fn spawn_attack_alert(webhook_url: String, notification: AttackNotification) {
    if webhook_url.trim().is_empty() {
        return;
    }

    // execute detached in background to never block core gateway or revert loops
    tokio::spawn(async move {
        send_attack_alert(&webhook_url, notification).await;
    });
}

pub async fn send_attack_alert(webhook_url: &str, notification: AttackNotification) {
    let url = webhook_url.trim();
    if url.is_empty() {
        return;
    }

    let mentions = if notification.managers.is_empty() {
        String::new()
    } else {
        notification
            .managers
            .iter()
            .map(|id| format!("<@{id}>"))
            .collect::<Vec<_>>()
            .join(" ")
    };

    let server_display = notification
        .server_name
        .as_deref()
        .map(|n| format!("{n} (`{}`)", notification.server_id))
        .unwrap_or_else(|| format!("`{}`", notification.server_id));

    let old_v = &notification.target_vanity;
    let new_v = notification
        .new_vanity
        .as_deref()
        .unwrap_or("none (removed)");

    let revert_status = if notification.revert_success {
        format!("✅ Reverted to `{old_v}`")
    } else {
        "❌ Revert Failed".to_string()
    };

    let executor_display = match notification.executor_id.as_deref() {
        Some(id) => format!("<@{id}> (`{id}`)"),
        None => "*Unknown (Fast Mode / Audit Missing)*".to_string(),
    };

    let punishment_display = match (
        notification.punishment_action.as_deref(),
        notification.punishment_success,
    ) {
        (Some(action), Some(true)) => {
            format!(
                "🔨 **{}ned**",
                if action == "kick" { "Kick" } else { "Ban" }
            )
        }
        (Some(action), Some(false)) => {
            format!(
                "⚠️ **{} Attempt Failed**",
                if action == "kick" { "Kick" } else { "Ban" }
            )
        }
        _ => "🛡️ *None (Whitelisted / Manager / Fast Mode)*".to_string(),
    };

    let color = if notification.revert_success {
        0x2ecc71 // green
    } else {
        0xe74c3c // red
    };

    let engine_time_formatted = if notification.engine_latency_us < 1000 {
        format!("{}µs", notification.engine_latency_us)
    } else {
        format!("{:.2}ms", notification.engine_latency_us as f64 / 1000.0)
    };

    let description = format!(
        "### 🚨 Vanity Protection Alert\n\n\
        **Server:** {server_display}\n\
        **Vanity:** `{old_v}` ➔ `{new_v}`\n\
        **Revert Status:** {revert_status}\n\
        **Executor:** {executor_display}\n\
        **Punishment:** {punishment_display}\n\
        **Engine Latency:** `{engine_time_formatted}`\n\
        **Total Time:** `{}ms`",
        notification.total_elapsed_ms
    );

    let payload = json!({
        "content": if mentions.is_empty() { serde_json::Value::Null } else { json!(mentions) },
        "embeds": [{
            "title": "Vanity URL Security Event",
            "description": description,
            "color": color,
            "footer": {
                "text": "DVP • Vanity Chaos Logger"
            }
        }]
    });

    debug!("dispatching background webhook alert to discord");

    // standalone client without discord client origin or authorization headers
    let client = match wreq::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            debug!("failed to build webhook client: {e}");
            return;
        }
    };

    let resp = match client
        .post(url)
        .header("Content-Type", "application/json")
        .body(payload.to_string())
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => {
            debug!("webhook send request failed: {e}");
            return;
        }
    };

    if resp.status().is_success() {
        debug!("webhook alert dispatched successfully");
    } else {
        debug!("webhook returned non-success status: {}", resp.status());
    }
}
