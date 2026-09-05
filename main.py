# SPDX-License-Identifier: AGPL-3.0-only
# Copyright (c) 2026 Faiz

import ctypes
import os
import platform
import sys
import time
import urllib.request

REPO_URL = "https://github.com/faiz4sure/discord-vanity-protector"

def get_asset_info():
    system = sys.platform
    arch = platform.machine().lower()

    if system == "win32":
        return "dvp-windows-x86_64.dll", "dvp.dll"
    elif system == "darwin":
        if "arm" in arch or "aarch64" in arch:
            return "libdvp-macos-aarch64.dylib", "libdvp.dylib"
        else:
            return "libdvp-macos-x86_64.dylib", "libdvp.dylib"
    else:  # linux / termux / android
        if "aarch64" in arch or "arm" in arch:
            return "libdvp-linux-aarch64.so", "libdvp.so"
        else:
            return "libdvp-linux-x86_64.so", "libdvp.so"

def download_library(dest_path, asset_name):
    url = f"{REPO_URL}/releases/latest/download/{asset_name}"
    print(f"[Python] Native binary not found locally. Downloading {asset_name} from GitHub Releases...")
    
    headers = {"User-Agent": "Mozilla/5.0"}
    req = urllib.request.Request(url, headers=headers)
    
    try:
        with urllib.request.urlopen(req) as resp, open(dest_path, "wb") as f:
            total = resp.headers.get("content-length")
            if total:
                total = int(total)
                downloaded = 0
                while chunk := resp.read(1024 * 64):
                    downloaded += len(chunk)
                    f.write(chunk)
                    percent = (downloaded / total) * 100
                    print(f"\r[Python] Downloading: {percent:.1f}% ({downloaded // (1024*1024)}MB / {total // (1024*1024)}MB)", end="")
                print()
            else:
                f.write(resp.read())
        print(f"[Python] Successfully downloaded and saved to: {dest_path}")
    except Exception as e:
        if os.path.exists(dest_path):
            os.remove(dest_path)
        raise RuntimeError(
            f"Failed to download pre-built binary from {url}: {e}\n"
            "You can also build manually using 'cargo build --release'."
        )

def find_or_download_library():
    base_dir = os.path.dirname(os.path.abspath(__file__))
    asset_name, local_filename = get_asset_info()

    search_dirs = [
        os.path.join(base_dir, "target", "release"),
        os.path.join(base_dir, "target", "debug"),
        base_dir,
    ]

    for sdir in search_dirs:
        for name in [local_filename, asset_name, "libdvp.so", "dvp.dll", "libdvp.dylib"]:
            path = os.path.join(sdir, name)
            if os.path.exists(path):
                return path

    dest_path = os.path.join(base_dir, local_filename)
    download_library(dest_path, asset_name)
    return dest_path

def main():
    lib_path = find_or_download_library()
    print(f"[Python] Loading DVP native engine from: {lib_path}")

    lib = ctypes.CDLL(lib_path)
    lib.dvp_start.argtypes = [ctypes.c_char_p]
    lib.dvp_start.restype = ctypes.c_int

    config_path = b"config.toml"
    result = lib.dvp_start(config_path)

    if result != 0:
        print(f"[Python] Failed to start DVP engine (error code: {result})")
        sys.exit(1)

    print("[Python] DVP engine running in background memory. Press Ctrl+C to stop.")
    try:
        while True:
            time.sleep(1)
    except KeyboardInterrupt:
        print("\n[Python] Shutting down...")

if __name__ == "__main__":
    main()
