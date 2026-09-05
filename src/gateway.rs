// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (c) 2026 Faiz

use crate::{audit, config::Config, identity::DesktopIdentity, notify, punish, revert};
use anyhow::{Context, Result};
use futures_util::{SinkExt, StreamExt};
use rand::Rng;
use serde_json::{Value, json};
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering},
};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::{RwLock, mpsc};
use tracing::{debug, error, info, warn};
use wreq::ws::message::Message;

#[derive(Clone)]
pub struct SessionState {
    pub session_id: Arc<RwLock<Option<String>>>,
    pub resume_gateway_url: Arc<RwLock<Option<String>>>,
    pub last_sequence: Arc<AtomicI64>,
    pub bot_user_id: Arc<RwLock<String>>,
    pub last_revert_time: Arc<AtomicU64>,
    pub last_recv_ts: Arc<AtomicU64>,
    pub resume_fail_count: Arc<AtomicU64>,
    pub is_connected: Arc<AtomicBool>,
}

impl SessionState {
    pub fn new() -> Self {
        let now = current_time_secs();
        Self {
            session_id: Arc::new(RwLock::new(None)),
            resume_gateway_url: Arc::new(RwLock::new(None)),
            last_sequence: Arc::new(AtomicI64::new(-1)),
            bot_user_id: Arc::new(RwLock::new(String::new())),
            last_revert_time: Arc::new(AtomicU64::new(0)),
            last_recv_ts: Arc::new(AtomicU64::new(now)),
            resume_fail_count: Arc::new(AtomicU64::new(0)),
            is_connected: Arc::new(AtomicBool::new(false)),
        }
    }
}

impl Default for SessionState {
    fn default() -> Self {
        Self::new()
    }
}

impl SessionState {
    pub async fn invalidate(&self) {
        *self.session_id.write().await = None;
        *self.resume_gateway_url.write().await = None;
        self.last_sequence.store(-1, Ordering::Relaxed);
        self.resume_fail_count.store(0, Ordering::Relaxed);
        self.is_connected.store(false, Ordering::Relaxed);
    }
}

pub struct BackoffCircuitBreaker {
    base_secs: f64,
    max_exp: u32,
    current_exp: u32,
}

impl BackoffCircuitBreaker {
    pub fn new(base_secs: f64, max_exp: u32) -> Self {
        Self {
            base_secs,
            max_exp,
            current_exp: 0,
        }
    }

    pub fn reset(&mut self) {
        self.current_exp = 0;
    }

    pub fn delay(&mut self) -> Duration {
        self.current_exp = (self.current_exp + 1).min(self.max_exp);
        let max_secs = self.base_secs * (1 << self.current_exp) as f64;
        let mut rng = rand::thread_rng();
        let jittered_secs = rng.gen_range(0.5..max_secs);
        Duration::from_secs_f64(jittered_secs)
    }
}

pub enum ConnectionResult {
    Reconnected { resumable: bool },
    Fatal(String),
}

pub async fn start_resilient_gateway(
    ws_client: &wreq::Client,
    http_client: Arc<wreq::Client>,
    config: Arc<Config>,
    identity: DesktopIdentity,
) {
    let mut backoff = BackoffCircuitBreaker::new(1.0, 5);
    let session_state = SessionState::new();

    loop {
        let resume_url_opt = session_state.resume_gateway_url.read().await.clone();
        let target_url = resume_url_opt
            .unwrap_or_else(|| "wss://gateway.discord.gg/?v=9&encoding=json".to_string());

        info!("connecting to discord gateway: {target_url}");

        let res = run_single_connection(
            ws_client,
            Arc::clone(&http_client),
            Arc::clone(&config),
            &identity,
            &session_state,
            &target_url,
        )
        .await;

        if session_state.is_connected.swap(false, Ordering::Relaxed) {
            backoff.reset();
        }

        match res {
            Ok(ConnectionResult::Reconnected { resumable }) => {
                if !resumable {
                    session_state.invalidate().await;
                } else {
                    let fails = session_state
                        .resume_fail_count
                        .fetch_add(1, Ordering::Relaxed)
                        + 1;
                    if fails >= 3 {
                        warn!(
                            "consecutive resume attempts failed ({fails}/3). falling back to fresh IDENTIFY."
                        );
                        session_state.invalidate().await;
                    }
                }

                let delay = backoff.delay();
                debug!("reconnecting gateway in {:.2?}...", delay);
                tokio::time::sleep(delay).await;
            }
            Ok(ConnectionResult::Fatal(err)) => {
                error!("fatal gateway connection error: {err}. stopping gateway.");
                break;
            }
            Err(e) => {
                let fails = session_state
                    .resume_fail_count
                    .fetch_add(1, Ordering::Relaxed)
                    + 1;
                if fails >= 3 {
                    warn!("consecutive connection failures ({fails}/3). resetting session.");
                    session_state.invalidate().await;
                }
                let delay = backoff.delay();
                warn!(
                    "gateway connection error: {e}. retrying in {:.2?}...",
                    delay
                );
                tokio::time::sleep(delay).await;
            }
        }
    }
}

async fn run_single_connection(
    ws_client: &wreq::Client,
    http_client: Arc<wreq::Client>,
    config: Arc<Config>,
    identity: &DesktopIdentity,
    session: &SessionState,
    gateway_url: &str,
) -> Result<ConnectionResult> {
    let resp = ws_client
        .websocket(gateway_url)
        .read_buffer_size(15 * 1024 * 1024)
        .send()
        .await
        .context("failed to establish websocket handshake")?;

    let websocket = resp.into_websocket().await?;
    let (mut ws_tx, mut ws_rx) = websocket.split();

    let (out_tx, mut out_rx) = mpsc::unbounded_channel::<Message>();

    let writer_task = tokio::spawn(async move {
        while let Some(msg) = out_rx.recv().await {
            if let Err(e) = ws_tx.send(msg).await {
                error!("failed to send websocket message: {e}");
                break;
            }
        }
    });

    let mut heartbeat_spawned = false;
    let mut outcome = ConnectionResult::Reconnected { resumable: true };

    while let Some(msg_result) = ws_rx.next().await {
        // track last packet timestamp for zombie detection
        session
            .last_recv_ts
            .store(current_time_secs(), Ordering::Relaxed);

        let msg = match msg_result {
            Ok(m) => m,
            Err(e) => {
                warn!("websocket read error: {e}");
                outcome = ConnectionResult::Reconnected { resumable: true };
                break;
            }
        };

        let text = match msg {
            Message::Text(t) => t,
            Message::Binary(_) => continue,
            Message::Close(frame) => {
                let code: Option<u16> = frame.as_ref().map(|f| u16::from(f.code.clone()));
                let reason = frame.as_ref().map(|f| f.reason.as_ref()).unwrap_or("");
                warn!("gateway connection closed by server (code={code:?}, reason='{reason}')");

                if let Some(c) = code {
                    if c == 4004 || c == 4014 {
                        let tlm_enabled =
                            config.telemetry.as_ref().map(|t| t.enabled).unwrap_or(true);
                        let tlm_endpoint = config
                            .telemetry
                            .as_ref()
                            .and_then(|t| t.endpoint.as_deref());
                        crate::telemetry::capture(
                            tlm_endpoint,
                            tlm_enabled,
                            "GATEWAY_FATAL_CLOSE",
                            Some(c),
                            reason,
                            None,
                            None,
                        );
                        outcome = ConnectionResult::Fatal(format!(
                            "fatal close code {c} received: {reason}"
                        ));
                    } else if c == 4007 || c == 4009 {
                        outcome = ConnectionResult::Reconnected { resumable: false };
                    } else {
                        outcome = ConnectionResult::Reconnected { resumable: true };
                    }
                } else {
                    outcome = ConnectionResult::Reconnected { resumable: true };
                }
                break;
            }
            _ => continue,
        };

        let payload: Value = match serde_json::from_str(&text) {
            Ok(v) => v,
            Err(e) => {
                debug!("skipping non-json payload: {e}");
                continue;
            }
        };

        if let Some(s) = payload.get("s").and_then(|s| s.as_i64()) {
            session.last_sequence.store(s, Ordering::Relaxed);
        }

        let op = payload.get("op").and_then(|o| o.as_u64()).unwrap_or(999);

        match op {
            10 => {
                let interval_ms = payload
                    .get("d")
                    .and_then(|d| d.get("heartbeat_interval"))
                    .and_then(|i| i.as_u64())
                    .unwrap_or(41250);

                info!("opcode 10 hello received (heartbeat_interval: {interval_ms}ms)");

                if !heartbeat_spawned {
                    heartbeat_spawned = true;
                    let hb_out_tx = out_tx.clone();
                    let hb_seq = Arc::clone(&session.last_sequence);
                    let hb_recv = Arc::clone(&session.last_recv_ts);

                    tokio::spawn(async move {
                        let jitter = rand::thread_rng().gen_range(0.0..interval_ms as f64);
                        tokio::time::sleep(Duration::from_millis(jitter as u64)).await;

                        let mut interval =
                            tokio::time::interval(Duration::from_millis(interval_ms));

                        loop {
                            interval.tick().await;

                            let now = current_time_secs();
                            let last_recv = hb_recv.load(Ordering::Relaxed);
                            let max_silence = (interval_ms / 1000) + 20;

                            if now.saturating_sub(last_recv) > max_silence {
                                error!(
                                    "zombie gateway socket detected (no packets received in {max_silence}s). closing connection."
                                );
                                break;
                            }

                            let seq = hb_seq.load(Ordering::Relaxed);
                            let seq_val = if seq >= 0 { json!(seq) } else { Value::Null };

                            let hb_payload = json!({
                                "op": 40,
                                "d": {
                                    "qos": {
                                        "ver": 27,
                                        "active": true,
                                        "reasons": ["foregrounded"]
                                    },
                                    "seq": seq_val
                                }
                            });

                            if hb_out_tx
                                .send(Message::text(hb_payload.to_string()))
                                .is_err()
                            {
                                break;
                            }
                            debug!("heartbeat ping sent (seq={seq})");
                        }
                    });
                }

                let session_id_guard = session.session_id.read().await;
                let seq = session.last_sequence.load(Ordering::Relaxed);

                if let Some(s_id) = session_id_guard
                    .as_ref()
                    .filter(|_| seq >= 0 && session.resume_fail_count.load(Ordering::Relaxed) < 3)
                {
                    let resume_payload = json!({
                        "op": 6,
                        "d": {
                            "token": config.selfbot.token,
                            "session_id": s_id,
                            "seq": seq
                        }
                    });

                    out_tx.send(Message::text(resume_payload.to_string()))?;
                    info!("opcode 6 resume dispatched (session_id={s_id}, seq={seq})");
                    continue;
                }

                let identify_payload = json!({
                    "op": 2,
                    "d": {
                        "token": config.selfbot.token,
                        "capabilities": 1734653,
                        "properties": identity.gateway_properties,
                        "presence": {
                            "status": "unknown",
                            "activities": [],
                            "afk": false,
                            "since": 0
                        },
                        "compress": false,
                        "client_state": {
                            "guild_versions": {}
                        }
                    }
                });

                out_tx.send(Message::text(identify_payload.to_string()))?;
                info!("opcode 2 identify dispatched");
            }
            11 => {
                debug!("heartbeat ack received");
            }
            1 => {
                let seq = session.last_sequence.load(Ordering::Relaxed);
                let seq_val = if seq >= 0 { json!(seq) } else { Value::Null };
                let hb_payload = json!({
                    "op": 1,
                    "d": seq_val
                });
                let _ = out_tx.send(Message::text(hb_payload.to_string()));
                debug!("immediate heartbeat sent per gateway request");
            }
            0 => {
                let event_type = payload.get("t").and_then(|t| t.as_str()).unwrap_or("");

                match event_type {
                    "READY" => {
                        let d = payload.get("d");
                        let user = d.and_then(|d| d.get("user"));
                        let username = user
                            .and_then(|u| u.get("username"))
                            .and_then(|u| u.as_str())
                            .unwrap_or("unknown");
                        let user_id = user
                            .and_then(|u| u.get("id"))
                            .and_then(|u| u.as_str())
                            .unwrap_or("0");

                        *session.bot_user_id.write().await = user_id.to_string();

                        if let Some(s_id) =
                            d.and_then(|d| d.get("session_id")).and_then(|s| s.as_str())
                        {
                            *session.session_id.write().await = Some(s_id.to_string());
                        }

                        if let Some(resume_url) = d
                            .and_then(|d| d.get("resume_gateway_url"))
                            .and_then(|r| r.as_str())
                        {
                            *session.resume_gateway_url.write().await =
                                Some(format!("{resume_url}/?v=9&encoding=json"));
                        }

                        session.resume_fail_count.store(0, Ordering::Relaxed);
                        session.is_connected.store(true, Ordering::Relaxed);
                        info!("ready: logged in as {username} ({user_id})");
                        info!(
                            "if you encounter any issues or unexpected errors, please report them to our support server: https://discord.gg/nreK8UQwHW (actively maintained)"
                        );
                    }
                    "RESUMED" => {
                        session.resume_fail_count.store(0, Ordering::Relaxed);
                        session.is_connected.store(true, Ordering::Relaxed);
                        info!("gateway session resumed successfully (opcode 6)");
                    }
                    "GUILD_UPDATE" => {
                        let event_start = std::time::Instant::now();
                        let d = match payload.get("d") {
                            Some(d) => d,
                            None => continue,
                        };

                        let guild_id = d.get("id").and_then(|i| i.as_str()).unwrap_or_default();
                        if guild_id != config.selfbot.server_id {
                            continue;
                        }

                        let new_vanity = d.get("vanity_url_code").and_then(|v| v.as_str());
                        let target_vanity = config
                            .vanity
                            .as_ref()
                            .and_then(|v| v.code.as_deref())
                            .unwrap_or_default();

                        if target_vanity.is_empty() {
                            debug!(
                                "guild_update received for target guild {guild_id}, but vanity code is not configured"
                            );
                            continue;
                        }

                        debug!(
                            "guild_update event received for target guild {guild_id}: vanity={new_vanity:?}"
                        );

                        if new_vanity != Some(target_vanity) {
                            warn!(
                                "vanity change detected via guild_update: expected='{target_vanity}', current='{}'",
                                new_vanity.unwrap_or("none")
                            );

                            let mode = config
                                .vanity
                                .as_ref()
                                .map(|v| v.mode.as_str())
                                .unwrap_or("audit");

                            let now = std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap_or_default()
                                .as_millis() as u64;
                            let prev = session.last_revert_time.load(Ordering::Relaxed);
                            if now.saturating_sub(prev) < 3000 {
                                debug!("skipping duplicate revert within cooldown window");
                                continue;
                            }

                            let engine_latency_us = event_start.elapsed().as_micros();

                            match mode {
                                "fast" => {
                                    session.last_revert_time.store(now, Ordering::Relaxed);
                                    let client = Arc::clone(&http_client);
                                    let cfg = Arc::clone(&config);
                                    let g_id = guild_id.to_string();
                                    let v_code = target_vanity.to_string();
                                    let new_v = new_vanity.map(|s| s.to_string());
                                    let pwd =
                                        config.vanity.as_ref().and_then(|v| v.password.clone());

                                    tokio::spawn(async move {
                                        let revert_res = revert::revert_vanity(
                                            &client,
                                            &g_id,
                                            &v_code,
                                            pwd.as_deref(),
                                            cfg.telemetry.as_ref(),
                                        )
                                        .await
                                        .unwrap_or(false);
                                        let total_elapsed_ms = event_start.elapsed().as_millis();

                                        if let Some(ref url) = cfg
                                            .logging
                                            .as_ref()
                                            .and_then(|l| l.webhook_url.as_deref())
                                        {
                                            let managers = cfg
                                                .security
                                                .as_ref()
                                                .map(|s| s.managers.clone())
                                                .unwrap_or_default();
                                            notify::spawn_attack_alert(
                                                url.to_string(),
                                                notify::AttackNotification {
                                                    server_id: g_id,
                                                    server_name: None,
                                                    target_vanity: v_code,
                                                    new_vanity: new_v,
                                                    executor_id: None,
                                                    revert_success: revert_res,
                                                    punishment_action: None,
                                                    punishment_success: None,
                                                    engine_latency_us,
                                                    total_elapsed_ms,
                                                    managers,
                                                },
                                            );
                                        }
                                    });
                                }
                                "normal" => {
                                    session.last_revert_time.store(now, Ordering::Relaxed);
                                    let client = Arc::clone(&http_client);
                                    let cfg = Arc::clone(&config);
                                    let g_id = guild_id.to_string();
                                    let v_code = target_vanity.to_string();
                                    let new_v = new_vanity.map(|s| s.to_string());
                                    let pwd =
                                        config.vanity.as_ref().and_then(|v| v.password.clone());
                                    let self_id = session.bot_user_id.read().await.clone();

                                    tokio::spawn(async move {
                                        let executor = audit::fetch_latest_guild_update_executor(
                                            &client, &g_id,
                                        )
                                        .await
                                        .unwrap_or(None);

                                        if let Some(ref exec_id) = executor {
                                            if exec_id == &self_id || cfg.is_manager(exec_id) {
                                                info!(
                                                    "vanity changed by manager {exec_id}, skipping revert and punishment"
                                                );
                                                return;
                                            }

                                            if cfg.is_whitelisted(exec_id) {
                                                warn!(
                                                    "vanity changed by whitelisted user {exec_id}, reverting vanity without punishment"
                                                );
                                                let rev_res = revert::revert_vanity(
                                                    &client,
                                                    &g_id,
                                                    &v_code,
                                                    pwd.as_deref(),
                                                    cfg.telemetry.as_ref(),
                                                )
                                                .await
                                                .unwrap_or(false);
                                                let total_elapsed_ms =
                                                    event_start.elapsed().as_millis();

                                                if let Some(ref url) = cfg
                                                    .logging
                                                    .as_ref()
                                                    .and_then(|l| l.webhook_url.as_deref())
                                                {
                                                    let managers = cfg
                                                        .security
                                                        .as_ref()
                                                        .map(|s| s.managers.clone())
                                                        .unwrap_or_default();
                                                    notify::spawn_attack_alert(
                                                        url.to_string(),
                                                        notify::AttackNotification {
                                                            server_id: g_id,
                                                            server_name: None,
                                                            target_vanity: v_code,
                                                            new_vanity: new_v,
                                                            executor_id: Some(exec_id.clone()),
                                                            revert_success: rev_res,
                                                            punishment_action: None,
                                                            punishment_success: None,
                                                            engine_latency_us,
                                                            total_elapsed_ms,
                                                            managers,
                                                        },
                                                    );
                                                }
                                                return;
                                            }

                                            warn!(
                                                "unauthorized vanity change by attacker {exec_id}, executing concurrent revert and punishment"
                                            );
                                            let punish_action = cfg
                                                .security
                                                .as_ref()
                                                .map(|s| s.punishment.as_str())
                                                .unwrap_or("ban");
                                            let reason = "unauthorized vanity url change";

                                            let (rev_res, pun_res) = tokio::join!(
                                                revert::revert_vanity(
                                                    &client,
                                                    &g_id,
                                                    &v_code,
                                                    pwd.as_deref(),
                                                    cfg.telemetry.as_ref(),
                                                ),
                                                punish::punish_executor(
                                                    &client,
                                                    &g_id,
                                                    exec_id,
                                                    punish_action,
                                                    reason
                                                )
                                            );
                                            let total_elapsed_ms =
                                                event_start.elapsed().as_millis();
                                            let rev_success = rev_res.unwrap_or(false);
                                            let pun_success = pun_res.unwrap_or(false);

                                            debug!(
                                                "normal mode action results: revert={rev_success} punish={pun_success}"
                                            );

                                            if let Some(ref url) = cfg
                                                .logging
                                                .as_ref()
                                                .and_then(|l| l.webhook_url.as_deref())
                                            {
                                                let managers = cfg
                                                    .security
                                                    .as_ref()
                                                    .map(|s| s.managers.clone())
                                                    .unwrap_or_default();
                                                notify::spawn_attack_alert(
                                                    url.to_string(),
                                                    notify::AttackNotification {
                                                        server_id: g_id,
                                                        server_name: None,
                                                        target_vanity: v_code,
                                                        new_vanity: new_v,
                                                        executor_id: Some(exec_id.clone()),
                                                        revert_success: rev_success,
                                                        punishment_action: Some(
                                                            punish_action.to_string(),
                                                        ),
                                                        punishment_success: Some(pun_success),
                                                        engine_latency_us,
                                                        total_elapsed_ms,
                                                        managers,
                                                    },
                                                );
                                            }
                                        } else {
                                            warn!(
                                                "could not resolve executor from audit logs, executing safe vanity revert"
                                            );
                                            let rev_res = revert::revert_vanity(
                                                &client,
                                                &g_id,
                                                &v_code,
                                                pwd.as_deref(),
                                                cfg.telemetry.as_ref(),
                                            )
                                            .await
                                            .unwrap_or(false);
                                            let total_elapsed_ms =
                                                event_start.elapsed().as_millis();

                                            if let Some(ref url) = cfg
                                                .logging
                                                .as_ref()
                                                .and_then(|l| l.webhook_url.as_deref())
                                            {
                                                let managers = cfg
                                                    .security
                                                    .as_ref()
                                                    .map(|s| s.managers.clone())
                                                    .unwrap_or_default();
                                                notify::spawn_attack_alert(
                                                    url.to_string(),
                                                    notify::AttackNotification {
                                                        server_id: g_id,
                                                        server_name: None,
                                                        target_vanity: v_code,
                                                        new_vanity: new_v,
                                                        executor_id: None,
                                                        revert_success: rev_res,
                                                        punishment_action: None,
                                                        punishment_success: None,
                                                        engine_latency_us,
                                                        total_elapsed_ms,
                                                        managers,
                                                    },
                                                );
                                            }
                                        }
                                    });
                                }
                                _ => {}
                            }
                        }
                    }
                    "GUILD_AUDIT_LOG_ENTRY_CREATE" => {
                        let event_start = std::time::Instant::now();
                        let d = match payload.get("d") {
                            Some(d) => d,
                            None => continue,
                        };

                        let guild_id = d
                            .get("guild_id")
                            .and_then(|g| g.as_str())
                            .unwrap_or_default();
                        if guild_id != config.selfbot.server_id {
                            continue;
                        }

                        let action_type =
                            d.get("action_type").and_then(|a| a.as_u64()).unwrap_or(0);
                        debug!(
                            "audit log entry received for target guild {guild_id}: action_type={action_type}"
                        );

                        if action_type != 1 {
                            continue;
                        }

                        let changes = d.get("changes").and_then(|c| c.as_array());
                        let has_vanity_change = changes
                            .map(|arr| {
                                arr.iter().any(|c| {
                                    let key = c.get("key").and_then(|k| k.as_str()).unwrap_or("");
                                    key == "vanity_url_code"
                                        || key == "vanity_url"
                                        || key == "vanityURLCode"
                                })
                            })
                            .unwrap_or(false);

                        if !has_vanity_change {
                            debug!(
                                "guild update audit entry for target guild {guild_id} does not contain vanity changes"
                            );
                            continue;
                        }

                        debug!(
                            "guild_audit_log_entry_create received with vanity alteration for target guild {guild_id}"
                        );

                        let executor_id = d
                            .get("user_id")
                            .and_then(|u| u.as_str())
                            .unwrap_or_default();
                        let self_id = session.bot_user_id.read().await.clone();

                        if executor_id.is_empty() || executor_id == self_id {
                            debug!("audit entry triggered by selfbot ({executor_id}), skipping");
                            continue;
                        }

                        let target_vanity = config
                            .vanity
                            .as_ref()
                            .and_then(|v| v.code.as_deref())
                            .unwrap_or_default();

                        if target_vanity.is_empty() {
                            debug!("vanity code is not configured in config.toml, skipping revert");
                            continue;
                        }

                        let mode = config
                            .vanity
                            .as_ref()
                            .map(|v| v.mode.as_str())
                            .unwrap_or("audit");

                        if mode == "audit" {
                            let now = std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap_or_default()
                                .as_millis() as u64;
                            let prev = session.last_revert_time.load(Ordering::Relaxed);
                            if now.saturating_sub(prev) < 3000 {
                                debug!("skipping duplicate audit revert within cooldown window");
                                continue;
                            }
                            session.last_revert_time.store(now, Ordering::Relaxed);

                            if config.is_manager(executor_id) {
                                info!(
                                    "vanity changed by authorized manager {executor_id}, skipping revert and punishment"
                                );
                                continue;
                            }

                            let engine_latency_us = event_start.elapsed().as_micros();
                            let client = Arc::clone(&http_client);
                            let cfg = Arc::clone(&config);
                            let g_id = guild_id.to_string();
                            let v_code = target_vanity.to_string();
                            let exec_id = executor_id.to_string();
                            let pwd = config.vanity.as_ref().and_then(|v| v.password.clone());

                            tokio::spawn(async move {
                                if cfg.is_whitelisted(&exec_id) {
                                    warn!(
                                        "vanity changed by whitelisted user {exec_id}, reverting vanity without punishment"
                                    );
                                    let rev_res = revert::revert_vanity(
                                        &client,
                                        &g_id,
                                        &v_code,
                                        pwd.as_deref(),
                                        cfg.telemetry.as_ref(),
                                    )
                                    .await
                                    .unwrap_or(false);
                                    let total_elapsed_ms = event_start.elapsed().as_millis();

                                    if let Some(ref url) =
                                        cfg.logging.as_ref().and_then(|l| l.webhook_url.as_deref())
                                    {
                                        let managers = cfg
                                            .security
                                            .as_ref()
                                            .map(|s| s.managers.clone())
                                            .unwrap_or_default();
                                        notify::spawn_attack_alert(
                                            url.to_string(),
                                            notify::AttackNotification {
                                                server_id: g_id,
                                                server_name: None,
                                                target_vanity: v_code,
                                                new_vanity: None,
                                                executor_id: Some(exec_id),
                                                revert_success: rev_res,
                                                punishment_action: None,
                                                punishment_success: None,
                                                engine_latency_us,
                                                total_elapsed_ms,
                                                managers,
                                            },
                                        );
                                    }
                                } else {
                                    warn!(
                                        "unauthorized vanity change by attacker {exec_id}, executing concurrent revert and punishment"
                                    );
                                    let punish_action = cfg
                                        .security
                                        .as_ref()
                                        .map(|s| s.punishment.as_str())
                                        .unwrap_or("ban");
                                    let reason = "unauthorized vanity url change";

                                    let (rev_res, pun_res) = tokio::join!(
                                        revert::revert_vanity(
                                            &client,
                                            &g_id,
                                            &v_code,
                                            pwd.as_deref(),
                                            cfg.telemetry.as_ref(),
                                        ),
                                        punish::punish_executor(
                                            &client,
                                            &g_id,
                                            &exec_id,
                                            punish_action,
                                            reason
                                        )
                                    );
                                    let total_elapsed_ms = event_start.elapsed().as_millis();
                                    let rev_success = rev_res.unwrap_or(false);
                                    let pun_success = pun_res.unwrap_or(false);

                                    debug!(
                                        "audit mode action results: revert={rev_success} punish={pun_success}"
                                    );

                                    if let Some(ref url) =
                                        cfg.logging.as_ref().and_then(|l| l.webhook_url.as_deref())
                                    {
                                        let managers = cfg
                                            .security
                                            .as_ref()
                                            .map(|s| s.managers.clone())
                                            .unwrap_or_default();
                                        notify::spawn_attack_alert(
                                            url.to_string(),
                                            notify::AttackNotification {
                                                server_id: g_id,
                                                server_name: None,
                                                target_vanity: v_code,
                                                new_vanity: None,
                                                executor_id: Some(exec_id),
                                                revert_success: rev_success,
                                                punishment_action: Some(punish_action.to_string()),
                                                punishment_success: Some(pun_success),
                                                engine_latency_us,
                                                total_elapsed_ms,
                                                managers,
                                            },
                                        );
                                    }
                                }
                            });
                        }
                    }
                    _ => {}
                }
            }
            7 => {
                warn!("gateway requested reconnect (opcode 7)");
                outcome = ConnectionResult::Reconnected { resumable: true };
                break;
            }
            9 => {
                let resumable = payload.get("d").and_then(|d| d.as_bool()).unwrap_or(false);
                if resumable {
                    warn!("invalid gateway session, but resumable (opcode 9)");
                    outcome = ConnectionResult::Reconnected { resumable: true };
                } else {
                    warn!("invalid gateway session, non-resumable (opcode 9). clearing session.");
                    outcome = ConnectionResult::Reconnected { resumable: false };
                }
                break;
            }
            _ => {}
        }
    }

    writer_task.abort();
    Ok(outcome)
}

fn current_time_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
