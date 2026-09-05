// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (c) 2026 Faiz

const koffi = require("koffi");
const fs = require("fs");
const path = require("path");
const https = require("https");

const REPO_URL = "https://github.com/faiz4sure/discord-vanity-protector";

function getAssetInfo() {
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

function downloadFile(url, destPath) {
  return new Promise((resolve, reject) => {
    https.get(url, { headers: { "User-Agent": "Mozilla/5.0" } }, (res) => {
      if (res.statusCode >= 300 && res.statusCode < 400 && res.headers.location) {
        return downloadFile(res.headers.location, destPath).then(resolve).catch(reject);
      }
      if (res.statusCode !== 200) {
        return reject(new Error(`Server returned status code ${res.statusCode}`));
      }

      const fileStream = fs.createWriteStream(destPath);
      res.pipe(fileStream);

      fileStream.on("finish", () => {
        fileStream.close(() => resolve());
      });

      fileStream.on("error", (err) => {
        fs.unlink(destPath, () => reject(err));
      });
    }).on("error", reject);
  });
}

async function findOrDownloadLibrary() {
  const baseDir = __dirname;
  const { assetName, localFilename } = getAssetInfo();

  const searchDirs = [
    path.join(baseDir, "target", "release"),
    path.join(baseDir, "target", "debug"),
    baseDir,
  ];

  for (const sdir of searchDirs) {
    for (const name of [localFilename, assetName, "libdvp.so", "dvp.dll", "libdvp.dylib"]) {
      const p = path.join(sdir, name);
      if (fs.existsSync(p)) return p;
    }
  }

  const destPath = path.join(baseDir, localFilename);
  const downloadUrl = `${REPO_URL}/releases/latest/download/${assetName}`;
  console.log(`[Node.js] Native binary not found locally. Downloading ${assetName} from GitHub Releases...`);

  try {
    await downloadFile(downloadUrl, destPath);
    console.log(`[Node.js] Successfully downloaded and saved to: ${destPath}`);
    return destPath;
  } catch (err) {
    if (fs.existsSync(destPath)) fs.unlinkSync(destPath);
    throw new Error(
      `Failed to download pre-built binary from ${downloadUrl}: ${err.message}\n` +
      "You can also build manually using 'cargo build --release'."
    );
  }
}

async function main() {
  const libPath = await findOrDownloadLibrary();
  console.log(`[Node.js] Loading DVP native engine from: ${libPath}`);

  const lib = koffi.load(libPath);
  const dvp_start = lib.func("int dvp_start(const char *config_path)");

  const res = dvp_start("config.toml");

  if (res !== 0) {
    console.error(`[Node.js] Failed to start DVP engine (error code: ${res})`);
    process.exit(1);
  }

  console.log("[Node.js] DVP engine running in background memory. Press Ctrl+C to stop.");

  process.on("SIGINT", () => {
    console.log("\n[Node.js] Shutting down...");
    process.exit(0);
  });

  setInterval(() => {}, 1000);
}

main().catch((err) => {
  console.error("[Node.js] Fatal error:", err);
  process.exit(1);
});
