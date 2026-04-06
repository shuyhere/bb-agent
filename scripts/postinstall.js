#!/usr/bin/env node

"use strict";

const { execSync } = require("child_process");
const fs = require("fs");
const path = require("path");
const os = require("os");
const https = require("https");

const VERSION = "0.0.1";
const REPO = "shuyhere/bb-agent";
const NATIVE_DIR = path.join(__dirname, "..", "native");

function getTarget() {
  const platform = os.platform();
  const arch = os.arch();

  const platformMap = {
    darwin: "apple-darwin",
    linux: "unknown-linux-gnu",
  };
  const archMap = {
    x64: "x86_64",
    arm64: "aarch64",
  };

  const rustPlatform = platformMap[platform];
  const rustArch = archMap[arch];

  if (!rustPlatform || !rustArch) {
    return null;
  }

  return `${rustArch}-${rustPlatform}`;
}

function downloadBinary(url, dest) {
  return new Promise((resolve, reject) => {
    const follow = (url, redirects = 0) => {
      if (redirects > 5) return reject(new Error("Too many redirects"));
      https
        .get(url, (res) => {
          if (res.statusCode >= 300 && res.statusCode < 400 && res.headers.location) {
            return follow(res.headers.location, redirects + 1);
          }
          if (res.statusCode !== 200) {
            return reject(new Error(`HTTP ${res.statusCode}`));
          }
          const file = fs.createWriteStream(dest);
          res.pipe(file);
          file.on("finish", () => {
            file.close();
            resolve();
          });
        })
        .on("error", reject);
    };
    follow(url);
  });
}

async function tryDownloadPrebuilt(target) {
  const assetName = `bb-${target}`;
  const url = `https://github.com/${REPO}/releases/download/v${VERSION}/${assetName}`;

  console.log(`Downloading BB-Agent v${VERSION} for ${target}...`);

  fs.mkdirSync(NATIVE_DIR, { recursive: true });
  const dest = path.join(NATIVE_DIR, "bb");

  try {
    await downloadBinary(url, dest);
    fs.chmodSync(dest, 0o755);
    console.log("BB-Agent binary installed successfully.");
    return true;
  } catch (err) {
    // Prebuilt not available for this platform
    return false;
  }
}

function tryCargoInstall() {
  // Check if cargo is available
  try {
    execSync("cargo --version", { stdio: "ignore" });
  } catch {
    return false;
  }

  console.log("Building BB-Agent from source with cargo (this may take a few minutes)...");
  const packageDir = path.join(__dirname, "..");

  try {
    fs.mkdirSync(NATIVE_DIR, { recursive: true });
    const binDest = path.join(NATIVE_DIR, "bb");
    execSync(
      `cargo build --release --manifest-path "${path.join(packageDir, "crates", "cli", "Cargo.toml")}"`,
      { stdio: "inherit", cwd: packageDir }
    );
    const built = path.join(packageDir, "target", "release", "bb");
    if (fs.existsSync(built)) {
      fs.copyFileSync(built, binDest);
      fs.chmodSync(binDest, 0o755);
      console.log("BB-Agent built and installed successfully.");
      return true;
    }
  } catch (err) {
    console.error("Cargo build failed:", err.message);
  }
  return false;
}

function checkExistingInstall() {
  // If bb is already in PATH (e.g. from cargo install), skip
  const envPath = process.env.PATH || "";
  const dirs = envPath.split(path.delimiter);
  for (const dir of dirs) {
    const full = path.join(dir, "bb");
    try {
      fs.accessSync(full, fs.constants.X_OK);
      const version = execSync(`"${full}" --version`, { encoding: "utf8" }).trim();
      console.log(`BB-Agent already installed: ${version} (${full})`);
      return true;
    } catch {
      // not found
    }
  }
  return false;
}

async function main() {
  // Skip in CI or if explicitly told to
  if (process.env.BB_SKIP_POSTINSTALL) {
    console.log("Skipping BB-Agent postinstall (BB_SKIP_POSTINSTALL set).");
    return;
  }

  // Check if already installed
  if (checkExistingInstall()) {
    return;
  }

  const target = getTarget();

  // Try prebuilt binary first
  if (target) {
    const downloaded = await tryDownloadPrebuilt(target);
    if (downloaded) return;
  }

  // Fall back to cargo build
  if (tryCargoInstall()) return;

  // Nothing worked
  console.error(
    "\n" +
    "================================================================\n" +
    " BB-Agent postinstall: could not install binary.\n" +
    "\n" +
    " No prebuilt binary found for your platform, and Rust/Cargo\n" +
    " is not installed.\n" +
    "\n" +
    " To install manually:\n" +
    "   1. Install Rust: curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh\n" +
    "   2. Clone and build:\n" +
    "      git clone https://github.com/shuyhere/bb-agent.git\n" +
    "      cd bb-agent\n" +
    "      cargo install --path crates/cli\n" +
    "   3. Run: bb\n" +
    "================================================================\n"
  );
}

main().catch((err) => {
  console.error("postinstall error:", err.message);
  // Don't fail npm install — the binary just won't be available
});
