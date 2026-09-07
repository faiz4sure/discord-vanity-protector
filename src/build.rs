// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (c) 2026 Faiz

use anyhow::Result;
use regex::Regex;
use serde::{Deserialize, Serialize};
use tracing::debug;

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
    client: Option<FallbackClientData>,
    properties: Option<FallbackProperties>,
    metadata: Option<FallbackMetadata>,
}

#[derive(Deserialize)]
struct FallbackClientData {
    build_number: u32,
    version: String,
    electron_version: Option<String>,
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
            debug!("failed to scrape live client build number, keeping default");
        }
    }

    // tier 3 fallback: resolve native host version from official discord manifest
    match tokio::time::timeout(std::time::Duration::from_secs(3), fetch_manifest_version()).await {
        Ok(Ok(manifest)) => {
            debug!("resolved manifest host version: {manifest}");
            build.client_version = manifest;
        }
        _ => {
            debug!("failed to fetch official manifest, using embedded default client version");
        }
    }

    build
}

async fn fetch_manifest_version() -> Result<String> {
    let client = wreq::Client::builder()
        .timeout(std::time::Duration::from_secs(3))
        .build()?;
    let resp: ManifestResponse = client
        .get("https://updates.discord.com/distributions/app/manifests/latest?channel=stable&platform=win&arch=x64")
        .header("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64)")
        .send()
        .await?
        .json()
        .await?;

    let host = resp
        .modules
        .and_then(|m| m.discord_desktop_core)
        .and_then(|c| c.full)
        .map(|f| f.host_version)
        .unwrap_or_else(|| vec![1, 0, 9256]);

    let version_str = host
        .iter()
        .map(|n| n.to_string())
        .collect::<Vec<_>>()
        .join(".");

    Ok(version_str)
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

    let default = DesktopBuild::default();

    if let Some(c) = resp.client {
        let electron = c.electron_version.unwrap_or(default.electron_version);
        return Ok(DesktopBuild {
            client_version: c.version,
            client_build_number: c.build_number,
            electron_version: electron,
            ..default
        });
    }

    if let Some(p) = resp.properties {
        let electron_version = resp
            .metadata
            .as_ref()
            .and_then(|m| m.electron_version.clone())
            .unwrap_or(p.browser_version);

        return Ok(DesktopBuild {
            client_version: p.client_version,
            native_build_number: p.native_build_number,
            client_build_number: p.client_build_number,
            electron_version,
            chrome_version: default.chrome_version,
            os_version: p.os_version,
            os_sdk_version: p.os_sdk_version,
        });
    }

    anyhow::bail!("remote metadata response contained no valid client or properties payload")
}
