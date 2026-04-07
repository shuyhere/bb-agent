#!/usr/bin/env node

"use strict";

const { execSync } = require("child_process");
const fs = require("fs");
const path = require("path");
const os = require("os");
const https = require("https");
const http = require("http");

const packageJson = require("../package.json");
const BINARY_RELEASE_TAG = `v${packageJson.version}`;
const REPO = "shuyhere/bb-agent";
const NATIVE_DIR = path.join(__dirname, "..", "native");
const DOWNLOAD_TIMEOUT_MS = 120_000;
const MAX_REDIRECTS = 8;
const MAX_DOWNLOAD_ATTEMPTS = 3;

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

function makeDownloadError(kind, message, statusCode) {
  const err = new Error(message);
  err.kind = kind;
  if (statusCode) err.statusCode = statusCode;
  return err;
}

function formatBytes(bytes) {
  if (!Number.isFinite(bytes) || bytes <= 0) return "0 B";
  const units = ["B", "KB", "MB", "GB"];
  let value = bytes;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  return `${value.toFixed(value >= 10 || unit === 0 ? 0 : 1)} ${units[unit]}`;
}

function requestBinary(url, dest, redirects = 0) {
  return new Promise((resolve, reject) => {
    if (redirects > MAX_REDIRECTS) {
      reject(makeDownloadError("redirect", "Too many redirects"));
      return;
    }

    const client = url.startsWith("https:") ? https : http;
    const req = client.get(
      url,
      {
        headers: {
          "User-Agent": `${packageJson.name}/${packageJson.version} (postinstall)`,
          Accept: "application/octet-stream,application/octet-stream; q=0.9,*/*;q=0.1",
        },
      },
      (res) => {
        const status = res.statusCode || 0;

        if (status >= 300 && status < 400 && res.headers.location) {
          res.resume();
          requestBinary(res.headers.location, dest, redirects + 1)
            .then(resolve)
            .catch(reject);
          return;
        }

        if (status === 404) {
          res.resume();
          reject(makeDownloadError("not-found", `HTTP 404 for ${url}`, 404));
          return;
        }

        if (status !== 200) {
          res.resume();
          reject(makeDownloadError("http", `HTTP ${status} for ${url}`, status));
          return;
        }

        const totalBytes = Number(res.headers["content-length"] || 0);
        let downloadedBytes = 0;
        let lastLoggedAt = Date.now();
        if (totalBytes > 0) {
          console.log(`Release asset size: ${formatBytes(totalBytes)}.`);
        } else {
          console.log("Release asset size: unknown (streaming download).");
        }

        const file = fs.createWriteStream(dest);
        let settled = false;

        const finish = (fn, value) => {
          if (settled) return;
          settled = true;
          clearTimeout(timeout);
          fn(value);
        };

        res.on("data", (chunk) => {
          downloadedBytes += chunk.length;
          const now = Date.now();
          if (now - lastLoggedAt >= 5000) {
            lastLoggedAt = now;
            if (totalBytes > 0) {
              const percent = Math.min(100, Math.round((downloadedBytes / totalBytes) * 100));
              console.log(
                `Download progress: ${percent}% (${formatBytes(downloadedBytes)} / ${formatBytes(totalBytes)})`
              );
            } else {
              console.log(`Downloaded ${formatBytes(downloadedBytes)} so far...`);
            }
          }
        });

        file.on("finish", () => {
          file.close((closeErr) => {
            if (closeErr) {
              finish(reject, makeDownloadError("write", closeErr.message));
            } else {
              if (totalBytes > 0) {
                console.log(
                  `Download complete: ${formatBytes(downloadedBytes)} / ${formatBytes(totalBytes)}.`
                );
              } else {
                console.log(`Download complete: ${formatBytes(downloadedBytes)}.`);
              }
              finish(resolve);
            }
          });
        });

        file.on("error", (err) => {
          try { file.close(() => {}); } catch {}
          try { fs.unlinkSync(dest); } catch {}
          finish(reject, makeDownloadError("write", err.message));
        });

        res.on("error", (err) => {
          try { file.close(() => {}); } catch {}
          try { fs.unlinkSync(dest); } catch {}
          finish(reject, makeDownloadError("network", err.message));
        });

        res.pipe(file);
      }
    );

    const timeout = setTimeout(() => {
      req.destroy(makeDownloadError("timeout", `Download timed out after ${DOWNLOAD_TIMEOUT_MS}ms`));
    }, DOWNLOAD_TIMEOUT_MS);

    req.on("error", (err) => {
      clearTimeout(timeout);
      reject(makeDownloadError(err.kind || "network", err.message));
    });
  });
}

function verifyBinary(dest) {
  try {
    execSync(`"${dest}" --version`, { stdio: "pipe", timeout: 5000 });
    return { ok: true };
  } catch (err) {
    return {
      ok: false,
      message: err && err.message ? err.message : "binary verification failed",
    };
  }
}

async function tryDownloadPrebuilt(target) {
  const assetName = assetNameForTarget(target);
  const url = `https://github.com/${REPO}/releases/download/${BINARY_RELEASE_TAG}/${assetName}`;

  fs.mkdirSync(NATIVE_DIR, { recursive: true });
  const dest = nativeBinaryPath();

  let lastError = null;
  for (let attempt = 1; attempt <= MAX_DOWNLOAD_ATTEMPTS; attempt += 1) {
    try {
      console.log(
        `Downloading BB-Agent ${BINARY_RELEASE_TAG} for ${target} (attempt ${attempt}/${MAX_DOWNLOAD_ATTEMPTS})...`
      );
      console.log("This may take a little while on first install because npm downloads and verifies the native binary from the GitHub release.");
      try { fs.unlinkSync(dest); } catch {}
      await requestBinary(url, dest, 0);
      fs.chmodSync(dest, 0o755);

      const verified = verifyBinary(dest);
      if (!verified.ok) {
        try { fs.unlinkSync(dest); } catch {}
        return {
          ok: false,
          kind: "verify",
          message: `Downloaded binary could not run: ${verified.message}`,
        };
      }

      console.log("Verifying downloaded binary...");
      console.log("✓ BB-Agent binary installed successfully.");
      return { ok: true };
    } catch (err) {
      lastError = err;
      try { fs.unlinkSync(dest); } catch {}
      if (err.kind === "not-found") {
        return {
          ok: false,
          kind: "not-found",
          message: `No release asset named ${assetName} was found for ${BINARY_RELEASE_TAG}.`,
        };
      }
      if (attempt < MAX_DOWNLOAD_ATTEMPTS) {
        await new Promise((resolve) => setTimeout(resolve, 1000 * attempt));
      }
    }
  }

  return {
    ok: false,
    kind: (lastError && lastError.kind) || "download",
    message: (lastError && lastError.message) || "unknown download failure",
  };
}

function printFallbackHelp(platform, reason) {
  console.log("");
  if (reason && reason.kind === "not-found") {
    console.log(
      `BB-Agent ${packageJson.version}: matching prebuilt binary is not published for ${platform}.`
    );
  } else if (reason) {
    console.log(
      `BB-Agent ${packageJson.version}: failed to download the prebuilt binary for ${platform}.`
    );
    console.log(`Reason: ${reason.message}`);
  } else {
    console.log(
      `BB-Agent ${packageJson.version}: matching prebuilt binary not available yet for ${platform}.`
    );
  }
  console.log("");
  console.log("╔══════════════════════════════════════════════════════════════╗");
  console.log(
    "║  BB-Agent: npm could not install native binary for " +
      platform.padEnd(16) +
      "   ║"
  );
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

async function main() {
  if (process.env.BB_SKIP_POSTINSTALL) {
    return;
  }

  if (hasBundledNativeBinary()) {
    return;
  }

  const target = getTarget();
  const platform = `${os.platform()}-${os.arch()}`;

  if (target) {
    const result = await tryDownloadPrebuilt(target);
    if (result.ok) {
      return;
    }
    printFallbackHelp(platform, result);
    return;
  }

  printFallbackHelp(platform, {
    kind: "unsupported-platform",
    message: `Unsupported target mapping for ${platform}`,
  });
}

main()
  .catch((err) => {
    console.error("BB-Agent postinstall notice:", err && err.message ? err.message : String(err));
    console.log(
      "Install manually: git clone https://github.com/shuyhere/bb-agent.git && cd bb-agent && cargo install --path crates/cli"
    );
  })
  .finally(() => {
    process.exitCode = 0;
  });
