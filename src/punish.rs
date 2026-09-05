// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (c) 2026 Faiz

use anyhow::Result;
use serde_json::json;
use std::time::Instant;
use tracing::{debug, error, info};

pub async fn punish_executor(
    http_client: &wreq::Client,
    guild_id: &str,
    user_id: &str,
    action: &str,
    reason: &str,
) -> Result<bool> {
    let start = Instant::now();
    let action_lower = action.to_ascii_lowercase();

    debug!("initiating punishment: user={user_id} action={action_lower} guild={guild_id}");

    match action_lower.as_str() {
        "ban" => {
            let url = format!("https://discord.com/api/v9/guilds/{guild_id}/bans/{user_id}");
            let payload = json!({ "delete_message_seconds": 0 });

            let resp = http_client
                .put(&url)
                .header("Content-Type", "application/json")
                .header("X-Audit-Log-Reason", reason)
                .body(payload.to_string())
                .send()
                .await?;

            if resp.status().is_success() {
                let elapsed = start.elapsed().as_millis();
                info!("banned vanity attacker: user_id={user_id} [{elapsed}ms]");
                Ok(true)
            } else {
                let err = resp.text().await.unwrap_or_default();
                error!("failed to ban attacker {user_id}: {err}");
                Ok(false)
            }
        }
        "kick" => {
            let url = format!("https://discord.com/api/v9/guilds/{guild_id}/members/{user_id}");

            let resp = http_client
                .delete(&url)
                .header("X-Audit-Log-Reason", reason)
                .send()
                .await?;

            if resp.status().is_success() {
                let elapsed = start.elapsed().as_millis();
                info!("kicked vanity attacker: user_id={user_id} [{elapsed}ms]");
                Ok(true)
            } else {
                let err = resp.text().await.unwrap_or_default();
                error!("failed to kick attacker {user_id}: {err}");
                Ok(false)
            }
        }
        _ => {
            debug!("unrecognized punishment action: {action}");
            Ok(false)
        }
    }
}
