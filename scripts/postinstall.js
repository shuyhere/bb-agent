#!/usr/bin/env node

"use strict";

const { execFileSync } = require("child_process");
const fs = require("fs");
const path = require("path");
const os = require("os");
const https = require("https");
const http = require("http");
const zlib = require("zlib");

const packageJson = require("../package.json");
const BINARY_RELEASE_TAG = `v${packageJson.version}`;
const REPO = "shuyhere/bb-agent";
const NATIVE_DIR = path.join(__dirname, "..", "native");
const DOWNLOAD_TIMEOUT_MS = 120_000;
const DOWNLOAD_PROGRESS_INTERVAL_MS = 1_000;
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

function assetCandidatesForTarget(target) {
  const assetName = assetNameForTarget(target);
  return [
    {
      assetName: `${assetName}.gz`,
      compressed: true,
    },
    {
      assetName,
      compressed: false,
    },
  ];
}

function logLine(message = "") {
  try {
    process.stderr.write(`${message}\n`);
  } catch (_) {}
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

function formatRate(bytesPerSecond) {
  if (!Number.isFinite(bytesPerSecond) || bytesPerSecond <= 0) return "0 B/s";
  return `${formatBytes(bytesPerSecond)}/s`;
}

function ensureParentDir(filePath) {
  fs.mkdirSync(path.dirname(filePath), { recursive: true });
}

function removeIfExists(filePath) {
  try {
    fs.unlinkSync(filePath);
  } catch (_) {}
}

function binaryVersion(binaryPath) {
  try {
    const out = execFileSync(binaryPath, ["--version"], {
      stdio: ["ignore", "pipe", "pipe"],
      timeout: 2500,
      encoding: "utf8",
    });
    return (out || "").trim();
  } catch (err) {
    return null;
  }
}

function binaryMatchesCurrentVersion(binaryPath) {
  const version = binaryVersion(binaryPath);
  if (!version) return false;
  return version.includes(packageJson.version);
}

function hasBundledNativeBinary() {
  const dest = nativeBinaryPath();
  if (!fs.existsSync(dest)) return false;
  if (!binaryMatchesCurrentVersion(dest)) return false;
  try {
    fs.accessSync(dest, fs.constants.X_OK);
    return true;
  } catch {
    return false;
  }
}

function cacheRootDir() {
  if (process.env.BB_INSTALL_CACHE_DIR && process.env.BB_INSTALL_CACHE_DIR.trim()) {
    return process.env.BB_INSTALL_CACHE_DIR;
  }

  const home = os.homedir();
  if (isWindows()) {
    return path.join(
      process.env.LOCALAPPDATA || process.env.APPDATA || path.join(home, "AppData", "Local"),
      "bb-agent"
    );
  }
  if (os.platform() === "darwin") {
    return path.join(home, "Library", "Caches", "bb-agent");
  }
  return path.join(process.env.XDG_CACHE_HOME || path.join(home, ".cache"), "bb-agent");
}

function cacheBinaryPath(target) {
  return path.join(cacheRootDir(), "prebuilt", packageJson.version, assetNameForTarget(target));
}

function cacheMetadataPath(target) {
  return `${cacheBinaryPath(target)}.json`;
}

function loadCacheMetadata(target) {
  try {
    return JSON.parse(fs.readFileSync(cacheMetadataPath(target), "utf8"));
  } catch (_) {
    return null;
  }
}

function storeCacheMetadata(target, binaryPath) {
  try {
    const stat = fs.statSync(binaryPath);
    ensureParentDir(cacheMetadataPath(target));
    fs.writeFileSync(
      cacheMetadataPath(target),
      JSON.stringify(
        {
          version: packageJson.version,
          target,
          assetName: assetNameForTarget(target),
          binaryName: nativeBinaryName(),
          size: stat.size,
          verifiedAt: new Date().toISOString(),
        },
        null,
        2
      )
    );
  } catch (_) {}
}

function copyBinary(src, dest) {
  ensureParentDir(dest);
  fs.copyFileSync(src, dest);
  if (!isWindows()) {
    fs.chmodSync(dest, 0o755);
  }
}

function installFromVerifiedCache(target) {
  const cached = cacheBinaryPath(target);
  const meta = loadCacheMetadata(target);
  if (!fs.existsSync(cached) || !meta) return false;
  if (meta.version !== packageJson.version || meta.target !== target) return false;

  let stat;
  try {
    stat = fs.statSync(cached);
  } catch (_) {
    return false;
  }
  if (!stat.isFile() || stat.size <= 0) return false;
  if (meta.size && stat.size !== meta.size) return false;

  logLine(`Using cached BB-Agent binary for ${target} (${formatBytes(stat.size)}).`);
  copyBinary(cached, nativeBinaryPath());
  return true;
}

function refreshCacheFromExistingBinary(target, sourcePath) {
  if (!binaryMatchesCurrentVersion(sourcePath)) return false;
  const cached = cacheBinaryPath(target);
  copyBinary(sourcePath, cached);
  storeCacheMetadata(target, cached);
  return true;
}

function maybeRepairCache(target) {
  const cached = cacheBinaryPath(target);
  if (!fs.existsSync(cached)) return false;

  const meta = loadCacheMetadata(target);
  if (meta && meta.version === packageJson.version && meta.target === target && meta.size) {
    try {
      const stat = fs.statSync(cached);
      if (stat.isFile() && stat.size === meta.size) {
        return false;
      }
    } catch (_) {
      return false;
    }
  }

  logLine(`Checking cached BB-Agent binary for ${target}...`);
  if (!binaryMatchesCurrentVersion(cached)) {
    removeIfExists(cached);
    removeIfExists(cacheMetadataPath(target));
    return false;
  }

  storeCacheMetadata(target, cached);
  logLine("Verified cached BB-Agent binary for reuse.");
  return true;
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
        const startedAt = Date.now();
        let downloadedBytes = 0;
        let lastLoggedAt = 0;

        if (totalBytes > 0) {
          logLine(`Release asset size: ${formatBytes(totalBytes)}.`);
        } else {
          logLine("Release asset size: unknown (streaming download).");
        }

        ensureParentDir(dest);
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
          if (now - lastLoggedAt >= DOWNLOAD_PROGRESS_INTERVAL_MS) {
            lastLoggedAt = now;
            const elapsedSeconds = Math.max((now - startedAt) / 1000, 0.001);
            const rate = downloadedBytes / elapsedSeconds;
            if (totalBytes > 0) {
              const percent = Math.min(100, Math.round((downloadedBytes / totalBytes) * 100));
              logLine(
                `Download progress: ${percent}% (${formatBytes(downloadedBytes)} / ${formatBytes(totalBytes)}, ${formatRate(rate)})`
              );
            } else {
              logLine(`Downloaded ${formatBytes(downloadedBytes)} so far (${formatRate(rate)})...`);
            }
          }
        });

        file.on("finish", () => {
          file.close((closeErr) => {
            if (closeErr) {
              finish(reject, makeDownloadError("write", closeErr.message));
            } else {
              const elapsedSeconds = Math.max((Date.now() - startedAt) / 1000, 0.001);
              const rate = downloadedBytes / elapsedSeconds;
              if (totalBytes > 0) {
                logLine(
                  `Download complete: ${formatBytes(downloadedBytes)} / ${formatBytes(totalBytes)} in ${elapsedSeconds.toFixed(1)}s (${formatRate(rate)}).`
                );
              } else {
                logLine(
                  `Download complete: ${formatBytes(downloadedBytes)} in ${elapsedSeconds.toFixed(1)}s (${formatRate(rate)}).`
                );
              }
              finish(resolve);
            }
          });
        });

        file.on("error", (err) => {
          try { file.close(() => {}); } catch (_) {}
          removeIfExists(dest);
          finish(reject, makeDownloadError("write", err.message));
        });

        res.on("error", (err) => {
          try { file.close(() => {}); } catch (_) {}
          removeIfExists(dest);
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

function verifyBinary(binaryPath) {
  logLine("Verifying downloaded binary...");
  const version = binaryVersion(binaryPath);
  if (!version) {
    return {
      ok: false,
      message: "binary verification failed",
    };
  }
  if (!version.includes(packageJson.version)) {
    return {
      ok: false,
      message: `expected version ${packageJson.version}, got '${version}'`,
    };
  }
  return { ok: true, version };
}

function expandCompressedBinary(src, dest) {
  logLine("Decompressing downloaded BB-Agent binary...");
  ensureParentDir(dest);
  const expanded = zlib.gunzipSync(fs.readFileSync(src));
  fs.writeFileSync(dest, expanded);
  removeIfExists(src);
}

async function tryDownloadPrebuilt(target) {
  const assetCandidates = assetCandidatesForTarget(target);

  fs.mkdirSync(NATIVE_DIR, { recursive: true });
  const dest = nativeBinaryPath();
  const tmpDest = `${dest}.tmp`;

  if (installFromVerifiedCache(target)) {
    logLine("✓ BB-Agent binary installed successfully from cache.");
    return { ok: true, source: "cache" };
  }

  if (maybeRepairCache(target) && installFromVerifiedCache(target)) {
    logLine("✓ BB-Agent binary installed successfully from cache.");
    return { ok: true, source: "cache" };
  }

  let lastError = null;
  for (let attempt = 1; attempt <= MAX_DOWNLOAD_ATTEMPTS; attempt += 1) {
    try {
      logLine(
        `Downloading BB-Agent ${BINARY_RELEASE_TAG} for ${target} (attempt ${attempt}/${MAX_DOWNLOAD_ATTEMPTS})...`
      );
      logLine("This may take a little while on first install because npm downloads the native binary from the GitHub release.");

      let missingCompressedAsset = false;
      for (const asset of assetCandidates) {
        const url = `https://github.com/${REPO}/releases/download/${BINARY_RELEASE_TAG}/${asset.assetName}`;
        const downloadDest = asset.compressed ? `${tmpDest}.gz` : tmpDest;

        removeIfExists(tmpDest);
        removeIfExists(`${tmpDest}.gz`);

        try {
          if (asset.compressed) {
            logLine(`Trying compressed release asset ${asset.assetName} first for a faster download.`);
          }
          await requestBinary(url, downloadDest, 0);
          if (asset.compressed) {
            expandCompressedBinary(downloadDest, tmpDest);
          }
          if (!isWindows()) {
            fs.chmodSync(tmpDest, 0o755);
          }

          const verified = verifyBinary(tmpDest);
          if (!verified.ok) {
            removeIfExists(tmpDest);
            return {
              ok: false,
              kind: "verify",
              message: `Downloaded binary could not run: ${verified.message}`,
            };
          }

          fs.renameSync(tmpDest, dest);
          refreshCacheFromExistingBinary(target, dest);
          logLine("Cached verified BB-Agent binary for future installs.");
          logLine("✓ BB-Agent binary installed successfully.");
          return { ok: true, source: "download" };
        } catch (err) {
          lastError = err;
          removeIfExists(tmpDest);
          removeIfExists(`${tmpDest}.gz`);
          if (err.kind === "not-found") {
            if (asset.compressed) {
              missingCompressedAsset = true;
              logLine(`Compressed asset ${asset.assetName} not found; falling back to the uncompressed release binary.`);
              continue;
            }
            return {
              ok: false,
              kind: "not-found",
              message: missingCompressedAsset
                ? `No release asset named ${asset.assetName} or ${assetCandidates[0].assetName} was found for ${BINARY_RELEASE_TAG}.`
                : `No release asset named ${asset.assetName} was found for ${BINARY_RELEASE_TAG}.`,
            };
          }
          throw err;
        }
      }
    } catch (err) {
      lastError = err;
      removeIfExists(tmpDest);
      removeIfExists(`${tmpDest}.gz`);
      if (attempt < MAX_DOWNLOAD_ATTEMPTS) {
        logLine(`Download failed (${err.message}). Retrying...`);
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
  logLine("");
  if (reason && reason.kind === "not-found") {
    logLine(`BB-Agent ${packageJson.version}: matching prebuilt binary is not published for ${platform}.`);
  } else if (reason) {
    logLine(`BB-Agent ${packageJson.version}: failed to download the prebuilt binary for ${platform}.`);
    logLine(`Reason: ${reason.message}`);
  } else {
    logLine(`BB-Agent ${packageJson.version}: matching prebuilt binary not available yet for ${platform}.`);
  }
  logLine("");
  logLine("╔══════════════════════════════════════════════════════════════╗");
  logLine(
    "║  BB-Agent: npm could not install native binary for " +
      platform.padEnd(16) +
      "   ║"
  );
  logLine("║                                                              ║");
  logLine("║  Install Rust (if needed):                                   ║");
  logLine("║    https://rustup.rs                                         ║");
  logLine("║    Then install with rustup for your platform                ║");
  logLine("║                                                              ║");
  logLine("║  Then build BB-Agent:                                        ║");
  logLine("║    git clone https://github.com/shuyhere/bb-agent.git        ║");
  logLine("║    cd bb-agent && cargo install --path crates/cli            ║");
  logLine("║                                                              ║");
  logLine("║  Then run:  bb                                               ║");
  logLine("╚══════════════════════════════════════════════════════════════╝");
  logLine("");
}

async function main() {
  if (process.env.BB_SKIP_POSTINSTALL) {
    return;
  }

  if (hasBundledNativeBinary()) {
    logLine(`BB-Agent ${packageJson.version} native binary already present; skipping download.`);
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
    logLine(`BB-Agent postinstall notice: ${err && err.message ? err.message : String(err)}`);
    logLine(
      "Install manually: git clone https://github.com/shuyhere/bb-agent.git && cd bb-agent && cargo install --path crates/cli"
    );
  })
  .finally(() => {
    process.exitCode = 0;
  });
