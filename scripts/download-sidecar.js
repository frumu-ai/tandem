#!/usr/bin/env node
/**
 * Download OpenCode sidecar binary for the current platform
 * 
 * This script fetches the appropriate OpenCode binary from GitHub releases
 * and places it in src-tauri/binaries/ for bundling with the Tauri app.
 * 
 * Usage: node scripts/download-sidecar.js [--force]
 */

import { existsSync, mkdirSync, chmodSync, unlinkSync, statSync } from "fs";
import { join, dirname } from "path";
import { fileURLToPath } from "url";
import https from "https";
import { createWriteStream } from "fs";

const __filename = fileURLToPath(import.meta.url);
const __dirname = dirname(__filename);
const ROOT_DIR = join(__dirname, "..");
const BINARIES_DIR = join(ROOT_DIR, "src-tauri", "binaries");

// OpenCode release info
const OPENCODE_REPO = "anomalyco/opencode";
const GITHUB_API = "https://api.github.com";

// Platform mappings - maps Node.js platform/arch to OpenCode binary names
// Updated to match new naming convention: opencode-{os}-{arch}.{ext}
const PLATFORM_MAP = {
  darwin: {
    x64: { target: "x86_64-apple-darwin", asset: "opencode-darwin-x64.zip" },
    arm64: { target: "aarch64-apple-darwin", asset: "opencode-darwin-arm64.zip" },
  },
  linux: {
    x64: { target: "x86_64-unknown-linux-gnu", asset: "opencode-linux-x64.tar.gz" },
    arm64: { target: "aarch64-unknown-linux-gnu", asset: "opencode-linux-arm64.tar.gz" },
  },
  win32: {
    x64: { target: "x86_64-pc-windows-msvc", asset: "opencode-windows-x64.zip" },
  },
};

// Minimum binary size to consider valid (100KB)
const MIN_BINARY_SIZE = 100 * 1024;

function getPlatformInfo() {
  const platform = process.platform;
  const arch = process.arch;

  const platformTargets = PLATFORM_MAP[platform];
  if (!platformTargets) {
    throw new Error(`Unsupported platform: ${platform}`);
  }

  const info = platformTargets[arch];
  if (!info) {
    throw new Error(`Unsupported architecture ${arch} for platform ${platform}`);
  }

  return { platform, arch, ...info };
}

function getTauriBinaryName(target) {
  const isWindows = target.includes("windows");
  const ext = isWindows ? ".exe" : "";
  return `opencode-${target}${ext}`;
}

function httpsGet(url, options = {}) {
  return new Promise((resolve, reject) => {
    const reqOptions = {
      headers: {
        "User-Agent": "Tandem-Sidecar-Downloader",
        ...options.headers,
      },
    };

    https.get(url, reqOptions, (response) => {
      // Handle redirects
      if (response.statusCode === 302 || response.statusCode === 301) {
        const redirectUrl = response.headers.location;
        if (redirectUrl) {
          httpsGet(redirectUrl, options).then(resolve).catch(reject);
          return;
        }
      }

      if (response.statusCode !== 200) {
        reject(new Error(`HTTP ${response.statusCode}: ${url}`));
        return;
      }

      if (options.json) {
        let data = "";
        response.on("data", (chunk) => (data += chunk));
        response.on("end", () => {
          try {
            resolve(JSON.parse(data));
          } catch (e) {
            reject(new Error(`Failed to parse JSON: ${e.message}`));
          }
        });
      } else {
        resolve(response);
      }
    }).on("error", reject);
  });
}

async function downloadFile(url, destPath) {
  console.log(`   Downloading from: ${url}`);
  
  return new Promise((resolve, reject) => {
    const file = createWriteStream(destPath);

    const request = (downloadUrl) => {
      https.get(downloadUrl, {
        headers: { "User-Agent": "Tandem-Sidecar-Downloader" },
      }, (response) => {
        // Handle redirects
        if (response.statusCode === 302 || response.statusCode === 301) {
          const redirectUrl = response.headers.location;
          if (redirectUrl) {
            request(redirectUrl);
            return;
          }
        }

        if (response.statusCode !== 200) {
          file.close();
          unlinkSync(destPath);
          reject(new Error(`Failed to download: HTTP ${response.statusCode}`));
          return;
        }

        const totalSize = parseInt(response.headers["content-length"], 10);
        let downloadedSize = 0;

        response.on("data", (chunk) => {
          downloadedSize += chunk.length;
          if (totalSize) {
            const percent = Math.round((downloadedSize / totalSize) * 100);
            process.stdout.write(`\r   Progress: ${percent}% (${(downloadedSize / 1024 / 1024).toFixed(1)}MB)`);
          }
        });

        response.pipe(file);
        file.on("finish", () => {
          file.close();
          console.log("\n   Download complete!");
          resolve();
        });
      }).on("error", (err) => {
        file.close();
        unlinkSync(destPath);
        reject(err);
      });
    };

    request(url);
  });
}

async function extractArchive(archivePath, destDir, isWindows) {
  const { execSync } = await import("child_process");
  
  console.log(`   Extracting archive...`);
  
  if (isWindows) {
    // Use PowerShell to extract zip on Windows
    execSync(`powershell -Command "Expand-Archive -Path '${archivePath}' -DestinationPath '${destDir}' -Force"`, {
      stdio: "inherit",
    });
  } else {
    // Use tar for .tar.gz on Unix
    execSync(`tar -xzf "${archivePath}" -C "${destDir}"`, {
      stdio: "inherit",
    });
  }
  
  console.log(`   Extraction complete!`);
}

async function fetchReleases() {
  console.log(`📡 Fetching releases from ${OPENCODE_REPO}...`);
  
  const url = `${GITHUB_API}/repos/${OPENCODE_REPO}/releases`;
  const releases = await httpsGet(url, { json: true });
  
  if (!Array.isArray(releases) || releases.length === 0) {
    throw new Error("No releases found");
  }
  
  return releases;
}

function findBestRelease(releases, assetName) {
  // Find the most recent release that has the asset we need
  for (const release of releases) {
    if (release.draft || release.prerelease) continue;
    
    const asset = release.assets?.find((a) => a.name === assetName);
    if (asset) {
      return { release, asset };
    }
  }
  
  // If no stable release has it, try prereleases
  for (const release of releases) {
    if (release.draft) continue;
    
    const asset = release.assets?.find((a) => a.name === assetName);
    if (asset) {
      console.log(`   ⚠️  Using prerelease: ${release.tag_name}`);
      return { release, asset };
    }
  }
  
  return null;
}

async function main() {
  const forceDownload = process.argv.includes("--force");
  
  console.log("🔧 Tandem Sidecar Download Script");
  console.log("================================\n");

  // Ensure binaries directory exists
  if (!existsSync(BINARIES_DIR)) {
    mkdirSync(BINARIES_DIR, { recursive: true });
    console.log(`📁 Created binaries directory: ${BINARIES_DIR}`);
  }

  const { platform, arch, target, asset: assetName } = getPlatformInfo();
  const tauriBinaryName = getTauriBinaryName(target);
  const destPath = join(BINARIES_DIR, tauriBinaryName);
  const isWindows = platform === "win32";

  console.log(`📦 Platform: ${platform}`);
  console.log(`📦 Architecture: ${arch}`);
  console.log(`📦 Target: ${target}`);
  console.log(`📦 Asset: ${assetName}`);
  console.log(`📦 Output: ${tauriBinaryName}\n`);

  // Check if valid binary already exists
  if (existsSync(destPath) && !forceDownload) {
    const stats = statSync(destPath);
    if (stats.size >= MIN_BINARY_SIZE) {
      console.log(`✅ Valid binary already exists at ${destPath}`);
      console.log(`   Size: ${(stats.size / 1024 / 1024).toFixed(1)}MB`);
      console.log("   Use --force to re-download.\n");
      return;
    } else {
      console.log(`⚠️  Existing binary is too small (${stats.size} bytes), re-downloading...`);
      unlinkSync(destPath);
    }
  }

  // Fetch releases from GitHub
  let releases;
  try {
    releases = await fetchReleases();
    console.log(`   Found ${releases.length} releases\n`);
  } catch (err) {
    console.error(`❌ Failed to fetch releases: ${err.message}`);
    console.log("\n📝 Manual download instructions:");
    console.log(`   1. Go to https://github.com/${OPENCODE_REPO}/releases`);
    console.log(`   2. Download ${assetName}`);
    console.log(`   3. Extract and copy the binary to ${destPath}`);
    process.exit(1);
  }

  // Find the best release with our asset
  const result = findBestRelease(releases, assetName);
  
  if (!result) {
    console.error(`❌ No release found with asset: ${assetName}`);
    console.log("\n   Available assets in latest release:");
    const latestRelease = releases.find((r) => !r.draft) || releases[0];
    latestRelease.assets?.forEach((a) => console.log(`   - ${a.name}`));
    console.log("\n📝 Manual download instructions:");
    console.log(`   1. Go to https://github.com/${OPENCODE_REPO}/releases`);
    console.log(`   2. Download the appropriate binary for your platform`);
    console.log(`   3. Rename and copy to ${destPath}`);
    process.exit(1);
  }

  const { release, asset } = result;
  console.log(`📥 Downloading OpenCode ${release.tag_name}...`);
  console.log(`   Asset: ${asset.name} (${(asset.size / 1024 / 1024).toFixed(1)}MB)`);

  // Download the archive
  const archivePath = join(BINARIES_DIR, assetName);
  try {
    await downloadFile(asset.browser_download_url, archivePath);
  } catch (err) {
    console.error(`\n❌ Download failed: ${err.message}`);
    process.exit(1);
  }

  // Extract the archive
  try {
    await extractArchive(archivePath, BINARIES_DIR, isWindows);
  } catch (err) {
    console.error(`❌ Extraction failed: ${err.message}`);
    unlinkSync(archivePath);
    process.exit(1);
  }

  // Clean up archive
  unlinkSync(archivePath);

  // Find and rename the extracted binary
  const extractedBinaryName = isWindows ? "opencode.exe" : "opencode";
  const extractedPath = join(BINARIES_DIR, extractedBinaryName);
  
  if (existsSync(extractedPath)) {
    const { renameSync } = await import("fs");
    
    // Rename to Tauri-expected name with platform suffix
    if (extractedPath !== destPath) {
      if (existsSync(destPath)) {
        unlinkSync(destPath);
      }
      renameSync(extractedPath, destPath);
    }
    
    // Set executable permissions on Unix
    if (!isWindows) {
      chmodSync(destPath, 0o755);
    }
    
    const finalStats = statSync(destPath);
    console.log(`\n✅ OpenCode binary installed successfully!`);
    console.log(`   Path: ${destPath}`);
    console.log(`   Size: ${(finalStats.size / 1024 / 1024).toFixed(1)}MB`);
    console.log(`   Version: ${release.tag_name}\n`);
  } else {
    // List what was extracted to help debug
    const { readdirSync } = await import("fs");
    const files = readdirSync(BINARIES_DIR);
    console.error(`❌ Expected binary not found after extraction`);
    console.log(`   Expected: ${extractedBinaryName}`);
    console.log(`   Found files: ${files.join(", ")}`);
    process.exit(1);
  }
}

main().catch((err) => {
  console.error("❌ Error:", err.message);
  process.exit(1);
});
