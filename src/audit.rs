// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (c) 2026 Faiz

use anyhow::Result;
use serde::Deserialize;
use tracing::{debug, error};

#[derive(Deserialize)]
struct AuditLogsResponse {
    audit_log_entries: Vec<AuditLogEntry>,
}

#[derive(Deserialize)]
struct AuditLogEntry {
    action_type: u32,
    user_id: Option<String>,
    changes: Option<Vec<AuditLogChange>>,
}

#[derive(Deserialize)]
struct AuditLogChange {
    key: String,
}

pub async fn fetch_latest_guild_update_executor(
    http_client: &wreq::Client,
    guild_id: &str,
) -> Result<Option<String>> {
    let url =
        format!("https://discord.com/api/v9/guilds/{guild_id}/audit-logs?action_type=1&limit=5");
    debug!("fetching audit logs for guild {guild_id}");

    let resp = http_client.get(&url).send().await?;

    let status = resp.status();
    if !status.is_success() {
        let err = resp.text().await.unwrap_or_default();
        error!("failed to fetch audit logs: status={status} body={err}");
        return Ok(None);
    }

    let data: AuditLogsResponse = match resp.json().await {
        Ok(d) => d,
        Err(e) => {
            debug!("failed to parse audit logs response: {e}");
            return Ok(None);
        }
    };

    for entry in data.audit_log_entries {
        if entry.action_type != 1 {
            continue;
        }

        let has_vanity_change = entry
            .changes
            .as_ref()
            .map(|changes| {
                changes.iter().any(|c| {
                    c.key == "vanity_url_code" || c.key == "vanity_url" || c.key == "vanityURLCode"
                })
            })
            .unwrap_or(false);

        if let Some(user_id) = entry.user_id.filter(|_| has_vanity_change) {
            debug!("resolved vanity update executor from audit logs: {user_id}");
            return Ok(Some(user_id));
        }
    }

    Ok(None)
}
