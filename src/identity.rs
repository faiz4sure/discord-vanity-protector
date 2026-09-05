// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (c) 2026 Faiz

use crate::build::DesktopBuild;
use base64::Engine;
use serde_json::{Value, json};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct DesktopIdentity {
    pub user_agent: String,
    #[allow(dead_code)]
    pub super_properties: Value,
    pub encoded_super_properties: String,
    pub gateway_properties: Value,
}

pub fn create_desktop_identity(build: &DesktopBuild) -> DesktopIdentity {
    let client_launch_id = Uuid::new_v4().to_string();
    let client_heartbeat_session_id = Uuid::new_v4().to_string();
    let launch_signature = generate_launch_signature();

    let user_agent = format!(
        "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) discord/{} Chrome/{} Electron/{} Safari/537.36",
        build.client_version, build.chrome_version, build.electron_version
    );

    let super_properties = json!({
        "os": "Windows",
        "browser": "Discord Client",
        "release_channel": "stable",
        "client_version": build.client_version,
        "os_version": build.os_version,
        "os_arch": "x64",
        "app_arch": "x64",
        "system_locale": "en-US",
        "has_client_mods": false,
        "client_launch_id": client_launch_id,
        "browser_user_agent": user_agent,
        "browser_version": build.electron_version,
        "os_sdk_version": build.os_sdk_version,
        "client_build_number": build.client_build_number,
        "native_build_number": build.native_build_number,
        "client_event_source": null,
        "launch_signature": launch_signature,
        "client_heartbeat_session_id": client_heartbeat_session_id,
        "client_app_state": "focused"
    });

    let encoded_super_properties =
        base64::engine::general_purpose::STANDARD.encode(super_properties.to_string().as_bytes());

    let mut gateway_properties = super_properties.clone();
    if let Some(obj) = gateway_properties.as_object_mut() {
        obj.insert("is_fast_connect".to_string(), json!(false));
        obj.insert("gateway_connect_reasons".to_string(), json!("AppSkeleton"));
    }

    DesktopIdentity {
        user_agent,
        super_properties,
        encoded_super_properties,
        gateway_properties,
    }
}

// clear 12 mod detection bit positions for clean client signature
fn generate_launch_signature() -> String {
    let mod_bits: u128 = (1 << 119)
        | (1 << 108)
        | (1 << 100)
        | (1 << 91)
        | (1 << 84)
        | (1 << 75)
        | (1 << 61)
        | (1 << 55)
        | (1 << 48)
        | (1 << 38)
        | (1 << 24)
        | (1 << 11);

    let random_val = Uuid::new_v4().as_u128();
    let clean_val = random_val & !mod_bits;
    Uuid::from_u128(clean_val).to_string()
}
