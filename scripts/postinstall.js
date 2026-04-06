#!/usr/bin/env node

"use strict";

const { execSync } = require("child_process");
const fs = require("fs");
const path = require("path");
const os = require("os");
const https = require("https");

const packageJson = require("../package.json");
const BINARY_RELEASE_TAG = `v${packageJson.version}`;
const REPO = "shuyhere/bb-agent";
const PACKAGE_ROOT = path.resolve(__dirname, "..");
const NATIVE_DIR = path.join(__dirname, "..", "native");
const DOWNLOAD_TIMEOUT_MS = 15_000;

function isWindows() {
  return os.platform() === "win32";
}

function nativeBinaryName() {
  return isWindows() ? "bb.exe" : "bb";
}

function nativeBinaryPath() {
  return path.join(NATIVE_DIR, nativeBinaryName());
}

function getTarget() {
  const platform = os.platform();
  const arch = os.arch();

  const platformMap = {
    darwin: "apple-darwin",
    linux: "unknown-linux-gnu",
    win32: "pc-windows-msvc",
  };
  const archMap = { x64: "x86_64", arm64: "aarch64" };

  const p = platformMap[platform];
  const a = archMap[arch];
  if (!p || !a) return null;
  return `${a}-${p}`;
}

function downloadBinary(url, dest, timeoutMs) {
  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error("Download timed out")), timeoutMs);

    const follow = (url, redirects = 0) => {
      if (redirects > 5) { clearTimeout(timer); return reject(new Error("Too many redirects")); }

      const mod = url.startsWith("https") ? https : require("http");
      const req = mod.get(url, (res) => {
        if (res.statusCode >= 300 && res.statusCode < 400 && res.headers.location) {
          return follow(res.headers.location, redirects + 1);
        }
        if (res.statusCode !== 200) {
          clearTimeout(timer);
          return reject(new Error(`HTTP ${res.statusCode}`));
        }
        const file = fs.createWriteStream(dest);
        res.pipe(file);
        file.on("finish", () => { clearTimeout(timer); file.close(); resolve(); });
        file.on("error", (e) => { clearTimeout(timer); reject(e); });
      });
      req.on("error", (e) => { clearTimeout(timer); reject(e); });
      req.on("timeout", () => { req.destroy(); clearTimeout(timer); reject(new Error("Request timed out")); });
    };
    follow(url);
  });
}

function assetNameForTarget(target) {
  return isWindows() ? `bb-${target}.exe` : `bb-${target}`;
}

function hasBundledNativeBinary() {
  const dest = nativeBinaryPath();
  if (!fs.existsSync(dest)) return false;
  try {
    execSync(`"${dest}" --version`, { stdio: "pipe", timeout: 5000 });
    return true;
  } catch {
    return false;
  }
}

async function tryDownloadPrebuilt(target) {
  const assetName = assetNameForTarget(target);
  const url = `https://github.com/${REPO}/releases/download/${BINARY_RELEASE_TAG}/${assetName}`;

  fs.mkdirSync(NATIVE_DIR, { recursive: true });
  const dest = nativeBinaryPath();

  try {
    console.log(`Downloading BB-Agent ${BINARY_RELEASE_TAG} for ${target}...`);
    await downloadBinary(url, dest, DOWNLOAD_TIMEOUT_MS);
    fs.chmodSync(dest, 0o755);

    // Verify the binary is executable
    try {
      execSync(`"${dest}" --version`, { stdio: "pipe", timeout: 5000 });
    } catch {
      // Binary may not run on this platform (e.g. wrong arch) — remove it
      fs.unlinkSync(dest);
      return false;
    }

    console.log("✓ BB-Agent binary installed successfully.");
    return true;
  } catch (err) {
    // Clean up partial download
    try { fs.unlinkSync(dest); } catch {}
    return false;
  }
}


async function main() {
  if (process.env.BB_SKIP_POSTINSTALL) {
    return;
  }

  if (hasBundledNativeBinary()) return;

  const target = getTarget();

  // Try prebuilt binary
  if (target) {
    const ok = await tryDownloadPrebuilt(target);
    if (ok) return;
  }

  // No prebuilt available — print instructions instead of trying cargo build
  // (cargo build takes 5+ minutes and would appear to hang)
  const platform = `${os.platform()}-${os.arch()}`;
  console.log("");
  console.log(`BB-Agent ${packageJson.version}: matching prebuilt binary not available yet for ${platform}.`);
  console.log("");
  console.log("╔══════════════════════════════════════════════════════════════╗");
  console.log("║  BB-Agent: no prebuilt binary for " + platform.padEnd(19) + "       ║");
  console.log("║                                                              ║");
  console.log("║  Install Rust (if needed):                                   ║");
  console.log("║    https://rustup.rs                                         ║");
  console.log("║    Then install with rustup for your platform                ║");
  console.log("║                                                              ║");
  console.log("║  Then build BB-Agent:                                        ║");
  console.log("║    git clone https://github.com/shuyhere/bb-agent.git        ║");
  console.log("║    cd bb-agent && cargo install --path crates/cli            ║");
  console.log("║                                                              ║");
  console.log("║  Then run:  bb                                               ║");
  console.log("╚══════════════════════════════════════════════════════════════╝");
  console.log("");
}

main().catch((err) => {
  // Never fail npm install — just print instructions
  console.error("BB-Agent postinstall notice:", err.message);
  console.log("Install manually: git clone https://github.com/shuyhere/bb-agent.git && cd bb-agent && cargo install --path crates/cli");
});
