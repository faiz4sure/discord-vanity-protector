// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (c) 2026 Faiz

use anyhow::Result;
use dvp::config::Config;
use dvp::{build, client, gateway, identity, validator};
use mimalloc::MiMalloc;
use std::sync::Arc;
use tracing::{error, info};
use tracing_subscriber::{EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};

#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

#[tokio::main]
async fn main() -> Result<()> {
    let cfg = match Config::load("config.toml") {
        Ok(c) => Arc::new(c),
        Err(e) => {
            eprintln!("config error: {e}");
            return Ok(());
        }
    };

    let panic_cfg = Arc::clone(&cfg);
    std::panic::set_hook(Box::new(move |panic_info| {
        let err_msg = panic_info.to_string();
        error!("critical unhandled panic intercepted: {err_msg}");
        let tlm_enabled = panic_cfg
            .telemetry
            .as_ref()
            .map(|t| t.enabled)
            .unwrap_or(true);
        let tlm_endpoint = panic_cfg
            .telemetry
            .as_ref()
            .and_then(|t| t.endpoint.as_deref());
        dvp::telemetry::capture(
            tlm_endpoint,
            tlm_enabled,
            "CRITICAL_PANIC",
            None,
            &err_msg,
            None,
            None,
        );
    }));

    let log_level = cfg
        .logging
        .as_ref()
        .and_then(|l| l.level.as_deref())
        .unwrap_or("info");

    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(format!("{log_level},wreq=warn")));

    tracing_subscriber::registry()
        .with(filter)
        .with(tracing_subscriber::fmt::layer().with_target(false))
        .init();

    info!("starting dvp discord client");

    if cfg.selfbot.token.trim().is_empty() {
        info!("token is empty in config.toml, please provide token to connect");
        return Ok(());
    }

    let vanity_code = cfg
        .vanity
        .as_ref()
        .and_then(|v| v.code.as_deref())
        .unwrap_or("none");
    let vanity_mode = cfg
        .vanity
        .as_ref()
        .map(|v| v.mode.as_str())
        .unwrap_or("audit");
    let punishment = cfg
        .security
        .as_ref()
        .map(|s| s.punishment.as_str())
        .unwrap_or("ban");

    info!(
        "vanity protection initialized: server={} vanity='{}' mode={} punishment={}",
        if cfg.selfbot.server_id.is_empty() {
            "none"
        } else {
            &cfg.selfbot.server_id
        },
        vanity_code,
        vanity_mode,
        punishment
    );

    info!("fetching discord desktop build metadata");
    let build_info = build::fetch_desktop_build().await;
    info!(
        "desktop metadata resolved: client_version={} chrome={} electron={} build={}",
        build_info.client_version,
        build_info.chrome_version,
        build_info.electron_version,
        build_info.client_build_number
    );

    let identity = identity::create_desktop_identity(&build_info);
    info!("windows desktop identity generated");

    let http_client = Arc::new(client::create_http_client(
        &identity,
        Some(&cfg.selfbot.token),
    )?);
    let ws_client = client::create_ws_client(&identity)?;
    info!("clients initialized with chromium 148 / windows profile");

    validator::start_token_validator(Arc::clone(&http_client), Arc::clone(&cfg));

    tokio::select! {
        _ = gateway::start_resilient_gateway(&ws_client, http_client, cfg, identity) => {},
        _ = wait_for_shutdown_signal() => {
            info!("gracefully shutting down dvp client...");
        }
    }

    Ok(())
}

#[cfg(unix)]
async fn wait_for_shutdown_signal() {
    use tokio::signal::unix::{SignalKind, signal};

    let mut sigint = signal(SignalKind::interrupt()).expect("failed to register SIGINT listener");
    let mut sigterm = signal(SignalKind::terminate()).expect("failed to register SIGTERM listener");
    let mut sighup = signal(SignalKind::hangup()).expect("failed to register SIGHUP listener");

    tokio::select! {
        _ = sigint.recv() => info!("shutdown signal received: SIGINT (Ctrl+C)"),
        _ = sigterm.recv() => info!("shutdown signal received: SIGTERM (kill / systemd stop)"),
        _ = sighup.recv() => info!("shutdown signal received: SIGHUP (terminal closed)"),
    }
}

#[cfg(not(unix))]
async fn wait_for_shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
    info!("shutdown signal received: Ctrl+C");
}
