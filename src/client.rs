// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (c) 2026 Faiz

use crate::identity::DesktopIdentity;
use anyhow::Result;
use wreq::{
    Client,
    header::{HeaderMap, HeaderValue, OrigHeaderMap},
    tls::{AlpnProtocol, TlsOptions, TlsVersion},
};
use wreq_util::{Emulation as UtilEmulation, Platform, Profile};

macro_rules! join {
    ($sep:expr, $first:expr $(, $rest:expr)*) => {
        concat!($first $(, $sep, $rest)*)
    };
}

// client for discord rest http requests using verified chrome 148 profile
pub fn create_http_client(identity: &DesktopIdentity, token: Option<&str>) -> Result<Client> {
    let emulation = UtilEmulation::builder()
        .profile(Profile::Chrome148)
        .platform(Platform::Windows)
        .headers(false)
        .build();

    let mut headers = HeaderMap::new();
    headers.insert(
        "X-Super-Properties",
        HeaderValue::from_str(&identity.encoded_super_properties)?,
    );
    headers.insert("User-Agent", HeaderValue::from_str(&identity.user_agent)?);
    headers.insert("Origin", HeaderValue::from_static("https://discord.com"));
    headers.insert(
        "Referer",
        HeaderValue::from_static("https://discord.com/channels/@me"),
    );
    headers.insert("Accept", HeaderValue::from_static("*/*"));
    headers.insert(
        "Accept-Language",
        HeaderValue::from_static("en-US,en;q=0.9"),
    );
    headers.insert(
        "Accept-Encoding",
        HeaderValue::from_static("gzip, deflate, br, zstd"),
    );
    headers.insert("Sec-CH-UA", HeaderValue::from_str(&identity.sec_ch_ua)?);
    headers.insert("Sec-CH-UA-Mobile", HeaderValue::from_static("?0"));
    headers.insert(
        "Sec-CH-UA-Platform",
        HeaderValue::from_static("\"Windows\""),
    );
    headers.insert("Sec-Fetch-Dest", HeaderValue::from_static("empty"));
    headers.insert("Sec-Fetch-Mode", HeaderValue::from_static("cors"));
    headers.insert("Sec-Fetch-Site", HeaderValue::from_static("same-origin"));
    headers.insert("X-Discord-Locale", HeaderValue::from_static("en-US"));
    headers.insert("X-Discord-Timezone", HeaderValue::from_static("UTC"));

    if let Some(t) = token.filter(|t| !t.is_empty()) {
        headers.insert("Authorization", HeaderValue::from_str(t)?);
    }

    let mut orig_headers = OrigHeaderMap::new();
    orig_headers.insert("X-Super-Properties");
    orig_headers.insert("User-Agent");
    orig_headers.insert("Origin");
    orig_headers.insert("Referer");
    orig_headers.insert("Accept");
    orig_headers.insert("Accept-Language");
    orig_headers.insert("Accept-Encoding");
    orig_headers.insert("Sec-CH-UA");
    orig_headers.insert("Sec-CH-UA-Mobile");
    orig_headers.insert("Sec-CH-UA-Platform");
    orig_headers.insert("Sec-Fetch-Dest");
    orig_headers.insert("Sec-Fetch-Mode");
    orig_headers.insert("Sec-Fetch-Site");
    orig_headers.insert("X-Discord-Locale");
    orig_headers.insert("X-Discord-Timezone");
    if token.is_some() {
        orig_headers.insert("Authorization");
    }

    let client = Client::builder()
        .emulation(emulation)
        .cookie_store(true)
        .default_headers(headers)
        .orig_headers(orig_headers)
        .build()?;

    Ok(client)
}

// client specifically for gateway websocket (strictly http/1.1 without auth in handshake)
pub fn create_ws_client(identity: &DesktopIdentity) -> Result<Client> {
    // chrome 148 tls with post-quantum ml-kem curve and alpn restricted to http/1.1
    let tls = TlsOptions::builder()
        .enable_ocsp_stapling(true)
        .enable_ech_grease(true)
        .permute_extensions(true)
        .curves_list(join!(":", "X25519MLKEM768", "X25519", "P-256", "P-384"))
        .cipher_list(join!(
            ":",
            "TLS_AES_128_GCM_SHA256",
            "TLS_AES_256_GCM_SHA384",
            "TLS_CHACHA20_POLY1305_SHA256",
            "TLS_ECDHE_ECDSA_WITH_AES_128_GCM_SHA256",
            "TLS_ECDHE_RSA_WITH_AES_128_GCM_SHA256",
            "TLS_ECDHE_ECDSA_WITH_AES_256_GCM_SHA384",
            "TLS_ECDHE_RSA_WITH_AES_256_GCM_SHA384",
            "TLS_ECDHE_ECDSA_WITH_CHACHA20_POLY1305_SHA256",
            "TLS_ECDHE_RSA_WITH_CHACHA20_POLY1305_SHA256"
        ))
        .sigalgs_list(join!(
            ":",
            "ecdsa_secp256r1_sha256",
            "rsa_pss_rsae_sha256",
            "rsa_pkcs1_sha256",
            "ecdsa_secp384r1_sha384",
            "rsa_pss_rsae_sha384",
            "rsa_pkcs1_sha384",
            "rsa_pss_rsae_sha512",
            "rsa_pkcs1_sha512"
        ))
        .alpn_protocols([AlpnProtocol::HTTP1])
        .min_tls_version(TlsVersion::TLS_1_2)
        .max_tls_version(TlsVersion::TLS_1_3)
        .build();

    let mut headers = HeaderMap::new();
    headers.insert("Pragma", HeaderValue::from_static("no-cache"));
    headers.insert("Cache-Control", HeaderValue::from_static("no-cache"));
    headers.insert("User-Agent", HeaderValue::from_str(&identity.user_agent)?);
    headers.insert("Origin", HeaderValue::from_static("https://discord.com"));
    headers.insert(
        "Accept-Language",
        HeaderValue::from_static("en-US,en;q=0.9"),
    );
    headers.insert(
        "Accept-Encoding",
        HeaderValue::from_static("gzip, deflate, br, zstd"),
    );
    headers.insert(
        "Sec-WebSocket-Extensions",
        HeaderValue::from_static("permessage-deflate; client_max_window_bits"),
    );

    let mut orig_headers = OrigHeaderMap::new();
    orig_headers.insert("Pragma");
    orig_headers.insert("Cache-Control");
    orig_headers.insert("User-Agent");
    orig_headers.insert("Origin");
    orig_headers.insert("Accept-Language");
    orig_headers.insert("Accept-Encoding");
    orig_headers.insert("Sec-WebSocket-Extensions");

    let emulation = wreq::Emulation::builder()
        .tls_options(tls)
        .headers(headers)
        .orig_headers(orig_headers)
        .build(Default::default());

    let client = Client::builder().emulation(emulation).build()?;

    Ok(client)
}
