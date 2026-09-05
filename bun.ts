// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (c) 2026 Faiz

import { dlopen, FFIType } from "bun:ffi";
import { existsSync } from "fs";
import { join } from "path";

const REPO_URL = "https://github.com/faiz4sure/discord-vanity-protector";

function getAssetInfo(): { assetName: string; localFilename: string } {
  const platform = process.platform;
  const arch = process.arch;

  if (platform === "win32") {
    return { assetName: "dvp-windows-x86_64.dll", localFilename: "dvp.dll" };
  } else if (platform === "darwin") {
    if (arch === "arm64") {
      return { assetName: "libdvp-macos-aarch64.dylib", localFilename: "libdvp.dylib" };
    } else {
      return { assetName: "libdvp-macos-x86_64.dylib", localFilename: "libdvp.dylib" };
    }
  } else {
    if (arch === "arm64") {
      return { assetName: "libdvp-linux-aarch64.so", localFilename: "libdvp.so" };
    } else {
      return { assetName: "libdvp-linux-x86_64.so", localFilename: "libdvp.so" };
    }
  }
}

async function downloadLibrary(destPath: string, assetName: string): Promise<void> {
  const url = `${REPO_URL}/releases/latest/download/${assetName}`;
  console.log(`[Bun] Native binary not found locally. Downloading ${assetName} from GitHub Releases...`);

  try {
    const res = await fetch(url, { headers: { "User-Agent": "Mozilla/5.0" } });
    if (!res.ok) {
      throw new Error(`HTTP error ${res.status}: ${res.statusText}`);
    }
    const arrayBuffer = await res.arrayBuffer();
    await Bun.write(destPath, arrayBuffer);
    console.log(`[Bun] Successfully downloaded and saved to: ${destPath}`);
  } catch (err: any) {
    throw new Error(
      `Failed to download pre-built binary from ${url}: ${err.message}\n` +
      "You can also build manually using 'cargo build --release'."
    );
  }
}

async function findOrDownloadLibrary(): Promise<string> {
  const baseDir = import.meta.dir;
  const { assetName, localFilename } = getAssetInfo();

  const searchDirs = [
    join(baseDir, "target", "release"),
    join(baseDir, "target", "debug"),
    baseDir,
  ];

  for (const sdir of searchDirs) {
    for (const name of [localFilename, assetName, "libdvp.so", "dvp.dll", "libdvp.dylib"]) {
      const p = join(sdir, name);
      if (existsSync(p)) return p;
    }
  }

  const destPath = join(baseDir, localFilename);
  await downloadLibrary(destPath, assetName);
  return destPath;
}

async function main() {
  const libPath = await findOrDownloadLibrary();
  console.log(`[Bun] Loading DVP native engine from: ${libPath}`);

  const lib = dlopen(libPath, {
    dvp_start: {
      args: [FFIType.cstring],
      returns: FFIType.i32,
    },
  });

  const configBuffer = Buffer.from("config.toml\0");
  const res = lib.symbols.dvp_start(configBuffer);

  if (res !== 0) {
    console.error(`[Bun] Failed to start DVP engine (error code: ${res})`);
    process.exit(1);
  }

  console.log("[Bun] DVP engine running in background memory. Press Ctrl+C to stop.");

  process.on("SIGINT", () => {
    console.log("\n[Bun] Shutting down...");
    process.exit(0);
  });

  setInterval(() => {}, 1000);
}

main().catch((err) => {
  console.error(`[Bun] Fatal error:`, err);
  process.exit(1);
});
