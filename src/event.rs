// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (c) 2026 Faiz

use crate::{audit, config::Config, gateway::SessionState, notify, punish, revert};
use serde_json::Value;
use std::sync::Arc;
use std::time::Instant;
use tracing::{debug, info, warn};

struct Context {
    guild_id: String,
    server_name: Option<String>,
    target_vanity: String,
    new_vanity: Option<String>,
    executor: Option<String>,
    start: Instant,
    latency: u128,
}

// handle incoming guild update event
pub async fn update(
    payload: &Value,
    session: &SessionState,
    client: &Arc<wreq::Client>,
    config: &Arc<Config>,
) {
    let start = Instant::now();
    let d = match payload.get("d") {
        Some(d) => d,
        None => return,
    };

    let guild_id = d.get("id").and_then(|i| i.as_str()).unwrap_or_default();
    if guild_id != config.selfbot.server_id {
        return;
    }

    let server_name = d.get("name").and_then(|n| n.as_str()).map(str::to_string);
    let new_vanity = d.get("vanity_url_code").and_then(|v| v.as_str());
    let target = config
        .vanity
        .as_ref()
        .and_then(|v| v.code.as_deref())
        .unwrap_or_default();

    if target.is_empty() {
        debug!("guild update for {guild_id} ignored: vanity code not configured");
        return;
    }

    debug!("guild update event received for {guild_id}: vanity={new_vanity:?}");

    if new_vanity == Some(target) {
        return;
    }

    warn!(
        "vanity change detected via guild_update: expected='{target}', current='{}'",
        new_vanity.unwrap_or("none")
    );

    let mode = config
        .vanity
        .as_ref()
        .map(|v| v.mode.as_str())
        .unwrap_or("audit");

    let latency = start.elapsed().as_micros();

    match mode {
        "fast" => {
            let client = Arc::clone(client);
            let cfg = Arc::clone(config);
            let g_id = guild_id.to_string();
            let v_code = target.to_string();
            let new_v = new_vanity.map(str::to_string);
            let pwd = config.vanity.as_ref().and_then(|v| v.password.clone());

            tokio::spawn(async move {
                let ok = revert::revert_vanity(
                    &client,
                    &g_id,
                    &v_code,
                    pwd.as_deref(),
                    cfg.telemetry.as_ref(),
                )
                .await
                .unwrap_or(false);

                let total = start.elapsed().as_millis();
                alert(
                    &cfg,
                    notify::AttackNotification {
                        server_id: g_id,
                        server_name,
                        target_vanity: v_code,
                        new_vanity: new_v,
                        executor_id: None,
                        revert_success: ok,
                        punishment_action: None,
                        punishment_success: None,
                        engine_latency_us: latency,
                        total_elapsed_ms: total,
                        managers: Vec::new(),
                    },
                );
            });
        }
        "normal" => {
            let client = Arc::clone(client);
            let cfg = Arc::clone(config);
            let g_id = guild_id.to_string();
            let v_code = target.to_string();
            let new_v = new_vanity.map(str::to_string);
            let self_id = session.bot_user_id.read().await.clone();

            tokio::spawn(async move {
                let exec = audit::fetch_latest_guild_update_executor(&client, &g_id)
                    .await
                    .unwrap_or(None);

                let ctx = Context {
                    guild_id: g_id,
                    server_name,
                    target_vanity: v_code,
                    new_vanity: new_v,
                    executor: exec,
                    start,
                    latency,
                };

                act(&client, &cfg, ctx, &self_id).await;
            });
        }
        _ => {}
    }
}

// handle incoming guild audit log entry create event
pub async fn entry(
    payload: &Value,
    session: &SessionState,
    client: &Arc<wreq::Client>,
    config: &Arc<Config>,
) {
    let start = Instant::now();
    let d = match payload.get("d") {
        Some(d) => d,
        None => return,
    };

    let guild_id = d
        .get("guild_id")
        .and_then(|g| g.as_str())
        .unwrap_or_default();
    if guild_id != config.selfbot.server_id {
        return;
    }

    let action_type = d.get("action_type").and_then(|a| a.as_u64()).unwrap_or(0);
    debug!("audit log entry for {guild_id}: action_type={action_type}");

    // action type 1 is guild update
    if action_type != 1 {
        return;
    }

    let changes = d.get("changes").and_then(|c| c.as_array());
    let mut has_vanity = false;
    let mut new_vanity = None;

    if let Some(arr) = changes {
        for c in arr {
            let key = c.get("key").and_then(|k| k.as_str()).unwrap_or("");
            if key == "vanity_url_code" || key == "vanity_url" || key == "vanityURLCode" {
                has_vanity = true;
                new_vanity = c
                    .get("new_value")
                    .and_then(|v| v.as_str())
                    .map(str::to_string);
                break;
            }
        }
    }

    if !has_vanity {
        debug!("guild audit entry for {guild_id} does not contain vanity changes");
        return;
    }

    let executor = d
        .get("user_id")
        .and_then(|u| u.as_str())
        .unwrap_or_default();
    let self_id = session.bot_user_id.read().await.clone();

    if executor.is_empty() || executor == self_id {
        debug!("audit entry triggered by selfbot ({executor}), skipping");
        return;
    }

    let target = config
        .vanity
        .as_ref()
        .and_then(|v| v.code.as_deref())
        .unwrap_or_default();

    if target.is_empty() {
        debug!("vanity code not configured, skipping revert");
        return;
    }

    if new_vanity.as_deref() == Some(target) {
        debug!("audit entry vanity change matches target code ({target}), skipping");
        return;
    }

    let mode = config
        .vanity
        .as_ref()
        .map(|v| v.mode.as_str())
        .unwrap_or("audit");

    if mode != "audit" {
        return;
    }

    let latency = start.elapsed().as_micros();
    let client = Arc::clone(client);
    let cfg = Arc::clone(config);
    let g_id = guild_id.to_string();
    let v_code = target.to_string();
    let exec = Some(executor.to_string());

    tokio::spawn(async move {
        let ctx = Context {
            guild_id: g_id,
            server_name: None,
            target_vanity: v_code,
            new_vanity,
            executor: exec,
            start,
            latency,
        };

        act(&client, &cfg, ctx, &self_id).await;
    });
}

// execute revert and punishment checks
async fn act(client: &wreq::Client, cfg: &Config, ctx: Context, self_id: &str) {
    let pwd = cfg.vanity.as_ref().and_then(|v| v.password.clone());

    let exec = match ctx.executor {
        Some(ref id) if !id.is_empty() => id.as_str(),
        _ => {
            let rev_ok = revert::revert_vanity(
                client,
                &ctx.guild_id,
                &ctx.target_vanity,
                pwd.as_deref(),
                cfg.telemetry.as_ref(),
            )
            .await
            .unwrap_or(false);

            let total = ctx.start.elapsed().as_millis();
            alert(
                cfg,
                notify::AttackNotification {
                    server_id: ctx.guild_id,
                    server_name: ctx.server_name,
                    target_vanity: ctx.target_vanity,
                    new_vanity: ctx.new_vanity,
                    executor_id: None,
                    revert_success: rev_ok,
                    punishment_action: None,
                    punishment_success: None,
                    engine_latency_us: ctx.latency,
                    total_elapsed_ms: total,
                    managers: Vec::new(),
                },
            );
            return;
        }
    };

    if exec == self_id {
        return;
    }

    if cfg.is_manager(exec) {
        info!("vanity changed by manager {exec}, skipping");
        return;
    }

    if cfg.is_whitelisted(exec) {
        warn!("vanity changed by whitelisted user {exec}, reverting without punishment");
        let rev_ok = revert::revert_vanity(
            client,
            &ctx.guild_id,
            &ctx.target_vanity,
            pwd.as_deref(),
            cfg.telemetry.as_ref(),
        )
        .await
        .unwrap_or(false);

        let total = ctx.start.elapsed().as_millis();
        alert(
            cfg,
            notify::AttackNotification {
                server_id: ctx.guild_id,
                server_name: ctx.server_name,
                target_vanity: ctx.target_vanity,
                new_vanity: ctx.new_vanity,
                executor_id: Some(exec.to_string()),
                revert_success: rev_ok,
                punishment_action: None,
                punishment_success: None,
                engine_latency_us: ctx.latency,
                total_elapsed_ms: total,
                managers: Vec::new(),
            },
        );
        return;
    }

    warn!("unauthorized vanity change by attacker {exec}, reverting and punishing");
    let punish_action = cfg
        .security
        .as_ref()
        .map(|s| s.punishment.as_str())
        .unwrap_or("ban");

    let (rev_res, pun_res) = tokio::join!(
        revert::revert_vanity(
            client,
            &ctx.guild_id,
            &ctx.target_vanity,
            pwd.as_deref(),
            cfg.telemetry.as_ref(),
        ),
        punish::punish_executor(
            client,
            &ctx.guild_id,
            exec,
            punish_action,
            "unauthorized vanity url change",
        )
    );

    let total = ctx.start.elapsed().as_millis();
    alert(
        cfg,
        notify::AttackNotification {
            server_id: ctx.guild_id,
            server_name: ctx.server_name,
            target_vanity: ctx.target_vanity,
            new_vanity: ctx.new_vanity,
            executor_id: Some(exec.to_string()),
            revert_success: rev_res.unwrap_or(false),
            punishment_action: Some(punish_action.to_string()),
            punishment_success: Some(pun_res.unwrap_or(false)),
            engine_latency_us: ctx.latency,
            total_elapsed_ms: total,
            managers: Vec::new(),
        },
    );
}

// dispatch webhook notification if configured
fn alert(cfg: &Config, mut note: notify::AttackNotification) {
    let url = match cfg.logging.as_ref().and_then(|l| l.webhook_url.as_deref()) {
        Some(u) => u,
        None => return,
    };

    note.managers = cfg
        .security
        .as_ref()
        .map(|s| s.managers.clone())
        .unwrap_or_default();

    notify::spawn_attack_alert(url.to_string(), note);
}
