// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (c) 2026 Faiz

use crate::totp::TotpKey;
use anyhow::{Context, Result, bail};
use serde::Deserialize;
use std::fs;

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub selfbot: SelfbotConfig,
    pub vanity: Option<VanityConfig>,
    pub security: Option<SecurityConfig>,
    pub logging: Option<LoggingConfig>,
    pub token_validator: Option<TokenValidatorConfig>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SelfbotConfig {
    pub token: String,
    pub server_id: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct VanityConfig {
    pub code: Option<String>,
    #[serde(default = "default_vanity_mode")]
    pub mode: String,
    pub password: Option<String>,
    pub totp_secret: Option<String>,
    #[serde(skip)]
    pub totp_key: Option<TotpKey>,
}

fn default_vanity_mode() -> String {
    "audit".to_string()
}

#[derive(Debug, Clone, Deserialize)]
pub struct SecurityConfig {
    #[serde(default)]
    pub managers: Vec<String>,
    #[serde(default)]
    pub whitelisted: Vec<String>,
    #[serde(default = "default_punishment")]
    pub punishment: String,
}

fn default_punishment() -> String {
    "ban".to_string()
}

#[derive(Debug, Clone, Deserialize)]
pub struct LoggingConfig {
    pub level: Option<String>,
    pub webhook_url: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TokenValidatorConfig {
    #[serde(default = "default_token_validator_enabled")]
    pub enabled: bool,
    #[serde(default = "default_interval_hours")]
    pub interval_hours: f64,
    pub webhook_url: Option<String>,
}

fn default_token_validator_enabled() -> bool {
    true
}

fn default_interval_hours() -> f64 {
    2.0
}

impl Config {
    pub fn parse_str(content: &str) -> Result<Self> {
        let mut config: Config =
            toml::from_str(content).with_context(|| "failed to parse config.toml")?;

        if let Some(sec) = config.security.as_ref().filter(|s| s.managers.len() > 2) {
            bail!(
                "configuration error: managers cannot exceed 2 user ids (found {})",
                sec.managers.len()
            );
        }

        if let Some(v) = config.vanity.as_mut() {
            match v.mode.as_str() {
                "fast" | "normal" | "audit" => {}
                mode => {
                    bail!(
                        "configuration error: invalid mode '{mode}', expected fast, normal, or audit"
                    )
                }
            }

            if let Some(secret) = v
                .totp_secret
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
            {
                v.totp_key = Some(
                    TotpKey::parse(secret)
                        .with_context(|| "configuration error: invalid totp_secret")?,
                );
            }
        }

        Ok(config)
    }

    pub fn load(path: &str) -> Result<Self> {
        let content = fs::read_to_string(path)
            .with_context(|| format!("failed to read config file at {path}"))?;
        Self::parse_str(&content)
    }

    pub fn is_manager(&self, user_id: &str) -> bool {
        self.security
            .as_ref()
            .map(|s| s.managers.iter().any(|m| m == user_id))
            .unwrap_or(false)
    }

    pub fn is_whitelisted(&self, user_id: &str) -> bool {
        self.security
            .as_ref()
            .map(|s| s.whitelisted.iter().any(|w| w == user_id))
            .unwrap_or(false)
    }
}
