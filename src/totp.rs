// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (c) 2026 Faiz

use anyhow::{Result, bail};
use std::{
    fmt,
    ptr::write_volatile,
    sync::atomic::{Ordering, compiler_fence},
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Clone)]
pub struct TotpKey {
    ipad: [u8; 64],
    opad: [u8; 64],
}

impl fmt::Debug for TotpKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("TotpKey([REDACTED])")
    }
}

impl Drop for TotpKey {
    fn drop(&mut self) {
        // scrub derived hmac pads so key material does not linger in memory
        for b in &mut self.ipad {
            unsafe { write_volatile(b, 0) };
        }
        for b in &mut self.opad {
            unsafe { write_volatile(b, 0) };
        }
        compiler_fence(Ordering::SeqCst);
    }
}

impl TotpKey {
    pub fn parse(secret_b32: &str) -> Result<Self> {
        let mut raw = decode_base32(secret_b32)?;
        // 16-char discord base32 secrets decode to 10 bytes (80 bits)
        if raw.len() < 10 {
            for b in &mut raw {
                unsafe { write_volatile(b, 0) };
            }
            bail!("totp secret too short: expected at least 16 base32 characters");
        }

        let mut k_block = [0u8; 64];
        if raw.len() > 64 {
            let hashed = sha1_variable(&raw);
            k_block[..20].copy_from_slice(&hashed);
        } else {
            k_block[..raw.len()].copy_from_slice(&raw);
        }

        for b in &mut raw {
            unsafe { write_volatile(b, 0) };
        }

        let mut ipad = [0x36u8; 64];
        let mut opad = [0x5cu8; 64];
        for i in 0..64 {
            ipad[i] ^= k_block[i];
            opad[i] ^= k_block[i];
            unsafe { write_volatile(&mut k_block[i], 0) };
        }
        compiler_fence(Ordering::SeqCst);

        Ok(Self { ipad, opad })
    }

    pub fn generate(&self) -> String {
        let now_secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        self.at_time(now_secs)
    }

    pub fn at_time(&self, unix_secs: u64) -> String {
        let counter = (unix_secs / 30).to_be_bytes();
        let mac = self.hmac_counter(&counter);
        let offset = (mac[19] & 0x0f) as usize;
        let binary = ((u32::from(mac[offset] & 0x7f)) << 24)
            | ((u32::from(mac[offset + 1])) << 16)
            | ((u32::from(mac[offset + 2])) << 8)
            | u32::from(mac[offset + 3]);

        let mut code = binary % 1_000_000;
        let mut digits = [b'0'; 6];
        for d in digits.iter_mut().rev() {
            *d = b'0' + (code % 10) as u8;
            code /= 10;
        }
        String::from_utf8_lossy(&digits).into_owned()
    }

    fn hmac_counter(&self, counter: &[u8; 8]) -> [u8; 20] {
        // 64-byte ipad + 8-byte counter = 72 bytes (576 bits), two 64-byte sha1 blocks
        let mut inner = [0u8; 128];
        inner[..64].copy_from_slice(&self.ipad);
        inner[64..72].copy_from_slice(counter);
        inner[72] = 0x80;
        inner[120..128].copy_from_slice(&576u64.to_be_bytes());
        let inner_hash = sha1_two_blocks(&inner);

        // 64-byte opad + 20-byte inner hash = 84 bytes (672 bits), two 64-byte sha1 blocks
        let mut outer = [0u8; 128];
        outer[..64].copy_from_slice(&self.opad);
        outer[64..84].copy_from_slice(&inner_hash);
        outer[84] = 0x80;
        outer[120..128].copy_from_slice(&672u64.to_be_bytes());
        sha1_two_blocks(&outer)
    }
}

fn decode_base32(input: &str) -> Result<Vec<u8>> {
    let mut out = Vec::with_capacity(input.len() * 5 / 8);
    let mut buffer: u32 = 0;
    let mut bits_left: u8 = 0;

    for ch in input.chars() {
        if ch.is_whitespace() || ch == '=' || ch == '-' {
            continue;
        }
        let val = match ch.to_ascii_uppercase() {
            'A'..='Z' => (ch.to_ascii_uppercase() as u8) - b'A',
            '2'..='7' => (ch as u8) - b'2' + 26,
            _ => bail!("invalid base32 character: {ch}"),
        };
        buffer = (buffer << 5) | u32::from(val);
        bits_left += 5;
        if bits_left >= 8 {
            bits_left -= 8;
            out.push(((buffer >> bits_left) & 0xff) as u8);
        }
    }
    Ok(out)
}

fn sha1_two_blocks(blocks: &[u8; 128]) -> [u8; 20] {
    let mut state = [
        0x67452301u32,
        0xEFCDAB89,
        0x98BADCFE,
        0x10325476,
        0xC3D2E1F0,
    ];
    for chunk in blocks.as_chunks::<64>().0 {
        compress_block(&mut state, chunk);
    }
    state_to_bytes(&state)
}

fn sha1_variable(data: &[u8]) -> [u8; 20] {
    let mut state = [
        0x67452301u32,
        0xEFCDAB89,
        0x98BADCFE,
        0x10325476,
        0xC3D2E1F0,
    ];
    let bit_len = (data.len() as u64) * 8;
    let mut padded = Vec::with_capacity((data.len() + 72) & !63);
    padded.extend_from_slice(data);
    padded.push(0x80);
    while (padded.len() % 64) != 56 {
        padded.push(0);
    }
    padded.extend_from_slice(&bit_len.to_be_bytes());

    for chunk in padded.as_chunks::<64>().0 {
        compress_block(&mut state, chunk);
    }
    state_to_bytes(&state)
}

#[inline(always)]
fn compress_block(state: &mut [u32; 5], chunk: &[u8; 64]) {
    let mut w = [0u32; 80];
    for (i, &word) in chunk.as_chunks::<4>().0.iter().enumerate() {
        w[i] = u32::from_be_bytes(word);
    }
    for i in 16..80 {
        w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
    }

    let [mut a, mut b, mut c, mut d, mut e] = *state;
    for (i, &wi) in w.iter().enumerate() {
        let (f, k) = match i {
            0..=19 => ((b & c) | ((!b) & d), 0x5A827999),
            20..=39 => (b ^ c ^ d, 0x6ED9EBA1),
            40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1BBCDC),
            _ => (b ^ c ^ d, 0xCA62C1D6),
        };
        let temp = a
            .rotate_left(5)
            .wrapping_add(f)
            .wrapping_add(e)
            .wrapping_add(k)
            .wrapping_add(wi);
        e = d;
        d = c;
        c = b.rotate_left(30);
        b = a;
        a = temp;
    }

    state[0] = state[0].wrapping_add(a);
    state[1] = state[1].wrapping_add(b);
    state[2] = state[2].wrapping_add(c);
    state[3] = state[3].wrapping_add(d);
    state[4] = state[4].wrapping_add(e);
}

#[inline(always)]
fn state_to_bytes(state: &[u32; 5]) -> [u8; 20] {
    let mut out = [0u8; 20];
    out[0..4].copy_from_slice(&state[0].to_be_bytes());
    out[4..8].copy_from_slice(&state[1].to_be_bytes());
    out[8..12].copy_from_slice(&state[2].to_be_bytes());
    out[12..16].copy_from_slice(&state[3].to_be_bytes());
    out[16..20].copy_from_slice(&state[4].to_be_bytes());
    out
}
