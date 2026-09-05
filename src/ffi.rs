// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (c) 2026 Faiz

use crate::{build, client, config::Config, gateway, identity, validator};
use std::ffi::CStr;
use std::os::raw::c_char;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tracing::{error, info, warn};
use tracing_subscriber::{EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};

static RUNNING: AtomicBool = AtomicBool::new(false);

#[unsafe(no_mangle)]
/// starts the DVP engine in a background tokio runtime thread pool
///
/// # Safety
///
/// `config_path_ptr` must either be `std::ptr::null()` or a valid, null-terminated C string
/// pointer containing valid UTF-8 characters.
pub unsafe extern "C" fn dvp_start(config_path_ptr: *const c_char) -> i32 {
    if RUNNING.swap(true, Ordering::SeqCst) {
        warn!("dvp engine is already running in this process");
        return 1;
    }

    let config_path = if config_path_ptr.is_null() {
        "config.toml"
    } else {
        match unsafe { CStr::from_ptr(config_path_ptr) }.to_str() {
            Ok(s) => s,
            Err(_) => return -1,
        }
    };

    let path_str = config_path.to_string();

    std::thread::spawn(move || {
        let runtime = match tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
        {
            Ok(rt) => rt,
            Err(e) => {
                eprintln!("failed to build tokio runtime: {e}");
                RUNNING.store(false, Ordering::SeqCst);
                return;
            }
        };

        runtime.block_on(async move {
            if let Err(e) = run_engine(&path_str).await {
                error!("dvp engine execution failed: {e}");
            }
        });

        RUNNING.store(false, Ordering::SeqCst);
    });

    0
}

async fn run_engine(config_path: &str) -> anyhow::Result<()> {
    let cfg = match Config::load(config_path) {
        Ok(c) => Arc::new(c),
        Err(e) => {
            eprintln!("config error: {e}");
            return Ok(());
        }
    };

    let panic_cfg = Arc::clone(&cfg);
    std::panic::set_hook(Box::new(move |panic_info| {
        let err_msg = panic_info.to_string();
        error!("critical unhandled panic intercepted in ffi: {err_msg}");
        let tlm_enabled = panic_cfg
            .telemetry
            .as_ref()
            .map(|t| t.enabled)
            .unwrap_or(true);
        let tlm_endpoint = panic_cfg
            .telemetry
            .as_ref()
            .and_then(|t| t.endpoint.as_deref());
        crate::telemetry::capture(
            tlm_endpoint,
            tlm_enabled,
            "CRITICAL_PANIC_FFI",
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

    let _ = tracing_subscriber::registry()
        .with(filter)
        .with(tracing_subscriber::fmt::layer().with_target(false))
        .try_init();

    info!("starting dvp discord client via C FFI engine");

    if cfg.selfbot.token.trim().is_empty() {
        info!("token is empty in config, please provide token to connect");
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

    gateway::start_resilient_gateway(&ws_client, http_client, cfg, identity).await;

    Ok(())
}
