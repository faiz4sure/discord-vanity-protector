// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (c) 2026 Faiz

use anyhow::Result;
use regex::Regex;
use serde::{Deserialize, Serialize};
use tracing::debug;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DesktopBuild {
    pub client_version: String,
    pub native_build_number: u32,
    pub client_build_number: u32,
    pub electron_version: String,
    pub chrome_version: String,
    pub os_version: String,
    pub os_sdk_version: String,
}

impl Default for DesktopBuild {
    fn default() -> Self {
        Self {
            client_version: "1.0.9256".to_string(),
            native_build_number: 89799,
            client_build_number: 607562,
            electron_version: "42.9.0".to_string(),
            chrome_version: "148.0.7778.280".to_string(),
            os_version: "10.0.26100".to_string(),
            os_sdk_version: "26100".to_string(),
        }
    }
}

#[derive(Deserialize)]
struct ManifestResponse {
    modules: Option<ManifestModules>,
    metadata_version: Option<u32>,
}

#[derive(Deserialize)]
struct ManifestModules {
    discord_desktop_core: Option<ManifestModuleCore>,
}

#[derive(Deserialize)]
struct ManifestModuleCore {
    full: Option<ManifestFull>,
}

#[derive(Deserialize)]
struct ManifestFull {
    host_version: Vec<u32>,
}

#[derive(Deserialize)]
struct FallbackResponse {
    properties: FallbackProperties,
    metadata: Option<FallbackMetadata>,
}

#[derive(Deserialize)]
struct FallbackProperties {
    client_version: String,
    native_build_number: u32,
    client_build_number: u32,
    browser_version: String,
    os_version: String,
    os_sdk_version: String,
}

#[derive(Deserialize)]
struct FallbackMetadata {
    electron_version: Option<String>,
    native_chrome_version: Option<String>,
}

pub async fn fetch_desktop_build() -> DesktopBuild {
    // tier 1: query remote metadata provider first for all-in-one synchronized properties
    match tokio::time::timeout(std::time::Duration::from_secs(3), fetch_remote_metadata()).await {
        Ok(Ok(remote)) => {
            debug!(
                "resolved build metadata from remote provider: build={}",
                remote.client_build_number
            );
            return remote;
        }
        _ => {
            debug!("remote provider unavailable, falling back to discord official endpoints");
        }
    }

    let mut build = DesktopBuild::default();

    // tier 2 fallback: scrape live client build number from discord web app
    match tokio::time::timeout(std::time::Duration::from_secs(3), scrape_build_number()).await {
        Ok(Ok(num)) => {
            debug!("scraped client build number from login page: {num}");
            build.client_build_number = num;
        }
        _ => {
            debug!("web app build scrape skipped, using baseline build number");
        }
    }

    // tier 2 fallback: fetch official desktop release manifest from discord cdn
    match tokio::time::timeout(std::time::Duration::from_secs(3), fetch_official_manifest()).await {
        Ok(Ok((ver, native_num))) => {
            debug!("retrieved official discord manifest: version={ver} native={native_num}");
            build.client_version = ver;
            build.native_build_number = native_num;
        }
        _ => {
            debug!("official manifest fetch skipped, using baseline host version");
        }
    }

    build
}

async fn fetch_official_manifest() -> Result<(String, u32)> {
    let install_id = Uuid::new_v4().to_string();
    let url = format!(
        "https://updates.discord.com/distributions/app/manifests/latest?channel=stable&platform=win&arch=x64&install_id={install_id}"
    );

    let client = wreq::Client::builder()
        .timeout(std::time::Duration::from_secs(3))
        .build()?;
    let resp: ManifestResponse = client
        .get(&url)
        .header("User-Agent", "Discord-Updater/1")
        .send()
        .await?
        .json()
        .await?;

    let version_parts = resp
        .modules
        .and_then(|m| m.discord_desktop_core)
        .and_then(|c| c.full)
        .map(|f| f.host_version)
        .unwrap_or_else(|| vec![1, 0, 9256]);
    let client_version = version_parts
        .iter()
        .map(|p| p.to_string())
        .collect::<Vec<_>>()
        .join(".");

    let native_num = resp.metadata_version.unwrap_or(89799);
    Ok((client_version, native_num))
}

async fn scrape_build_number() -> Result<u32> {
    let client = wreq::Client::builder()
        .timeout(std::time::Duration::from_secs(3))
        .build()?;
    let html = client
        .get("https://discord.com/login")
        .header("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64)")
        .send()
        .await?
        .text()
        .await?;

    let re = Regex::new(r#""BUILD_NUMBER":\s*"(\d+)""#)?;
    let build_num: u32 = re
        .captures(&html)
        .and_then(|cap| cap.get(1))
        .and_then(|m| m.as_str().parse().ok())
        .unwrap_or(DesktopBuild::default().client_build_number);

    Ok(build_num)
}

async fn fetch_remote_metadata() -> Result<DesktopBuild> {
    let client = wreq::Client::builder()
        .timeout(std::time::Duration::from_secs(3))
        .build()?;
    let resp: FallbackResponse = client
        .post("https://cordapi.dolfi.es/api/v2/properties/windows")
        .header("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64)")
        .send()
        .await?
        .json()
        .await?;

    let chrome_version = resp
        .metadata
        .as_ref()
        .and_then(|m| m.native_chrome_version.clone())
        .unwrap_or_else(|| "148.0.7778.280".to_string());

    let electron_version = resp
        .metadata
        .as_ref()
        .and_then(|m| m.electron_version.clone())
        .unwrap_or(resp.properties.browser_version);

    Ok(DesktopBuild {
        client_version: resp.properties.client_version,
        native_build_number: resp.properties.native_build_number,
        client_build_number: resp.properties.client_build_number,
        electron_version,
        chrome_version,
        os_version: resp.properties.os_version,
        os_sdk_version: resp.properties.os_sdk_version,
    })
}
