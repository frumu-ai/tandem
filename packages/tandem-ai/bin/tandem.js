#!/usr/bin/env node

const fs = require('fs');
const path = require('path');
const https = require('https');
const { spawn, execSync } = require('child_process');
const os = require('os');

// Configuration
const TANDEM_REPO = "frumu-ai/tandem";
const CACHE_DIR = path.join(os.homedir(), '.tandem', 'bin');
const TUI_BINARY_NAME = process.platform === 'win32' ? 'tandem-tui.exe' : 'tandem-tui';
const ENGINE_BINARY_NAME = process.platform === 'win32' ? 'tandem-engine.exe' : 'tandem-engine';
const ENGINE_PACKAGE = "@frumu-ltd/tandem-engine";

// Ensure cache directory exists
if (!fs.existsSync(CACHE_DIR)) {
    fs.mkdirSync(CACHE_DIR, { recursive: true });
}

// Helper to download file
async function downloadFile(url, destPath) {
    return new Promise((resolve, reject) => {
        const file = fs.createWriteStream(destPath);
        const request = (downloadUrl) => {
            https.get(downloadUrl, { headers: { 'User-Agent': 'tandem-cli' } }, (response) => {
                if (response.statusCode === 302 || response.statusCode === 301) {
                    return request(response.headers.location);
                }
                if (response.statusCode !== 200) {
                    fs.unlink(destPath, () => { });
                    return reject(new Error(`Failed to download: HTTP ${response.statusCode}`));
                }
                response.pipe(file);
                file.on('finish', () => {
                    file.close();
                    resolve();
                });
            }).on('error', (err) => {
                fs.unlink(destPath, () => { });
                reject(err);
            });
        };
        request(url);
    });
}

// Helper to fetch latest release info
async function fetchLatestRelease() {
    return new Promise((resolve, reject) => {
        https.get(`https://api.github.com/repos/${TANDEM_REPO}/releases`, { headers: { 'User-Agent': 'tandem-cli' } }, (res) => {
            let data = '';
            res.on('data', chunk => data += chunk);
            res.on('end', () => {
                if (res.statusCode !== 200) return reject(new Error(`GitHub API Error: ${res.statusCode}`));
                try {
                    const releases = JSON.parse(data);
                    resolve(releases[0]); // Get latest, including prerelease if it's the very latest
                } catch (e) { reject(e); }
            });
        });
    });
}

// Platform detection
function getPlatformInfo() {
    const platform = process.platform;
    const arch = process.arch;

    // Mapping logic (simplified for brevity, match existing script logic)
    let target = '';
    let ext = platform === 'win32' ? '.zip' : '.tar.gz';

    if (platform === 'win32') target = 'windows-x64';
    else if (platform === 'darwin') target = arch === 'arm64' ? 'darwin-arm64' : 'darwin-x64';
    else if (platform === 'linux') target = arch === 'arm64' ? 'linux-arm64' : 'linux-x64';
    else throw new Error(`Unsupported platform: ${platform}-${arch}`);

    return { target, ext };
}

async function ensureBinary(binaryName, artifactPrefix) {
    const binPath = path.join(CACHE_DIR, binaryName);

    if (fs.existsSync(binPath)) {
        // Optionally check version/update, but for now just existence
        return binPath;
    }

    console.log(`Binary ${binaryName} not found. Downloading...`);
    const release = await fetchLatestRelease();
    if (!release) throw new Error("No release found");

    const { target, ext } = getPlatformInfo();
    // Expect naming convention: artifactPrefix-target.ext  (e.g. tandem-tui-windows-x64.zip)
    // Or tandem-engine-windows-x64.zip

    const assetName = `${artifactPrefix}-${target}${ext}`;
    const asset = release.assets.find(a => a.name === assetName);

    if (!asset) {
        throw new Error(`Asset ${assetName} not found in release ${release.tag_name}`);
    }

    const archivePath = path.join(CACHE_DIR, assetName);
    await downloadFile(asset.browser_download_url, archivePath);

    console.log("Extracting...");
    // Simple extraction for now
    try {
        if (path.extname(archivePath) === '.zip') {
            // Use powershell on windows, unzip on unix
            if (process.platform === 'win32') {
                execSync(`powershell -Command "Expand-Archive -Path '${archivePath}' -DestinationPath '${CACHE_DIR}' -Force"`);
            } else {
                execSync(`unzip -o "${archivePath}" -d "${CACHE_DIR}"`);
            }
        } else {
            execSync(`tar -xzf "${archivePath}" -C "${CACHE_DIR}"`);
        }
    } catch (e) {
        console.error("Extraction failed", e);
        throw e;
    } finally {
        fs.unlinkSync(archivePath);
    }

    if (!fs.existsSync(binPath)) throw new Error(`Extraction failed to produce ${binaryName}`);
    if (process.platform !== 'win32') fs.chmodSync(binPath, 0o755);

    return binPath;
}

// Update check
async function checkForPackageUpdate() {
    try {
        const pkg = require('../package.json');
        // Simple fetch to registry
        return new Promise((resolve) => {
            const req = https.get(`https://registry.npmjs.org/${pkg.name}/latest`, { headers: { 'User-Agent': 'node' } }, (res) => {
                let data = '';
                res.on('data', c => data += c);
                res.on('end', () => {
                    if (res.statusCode === 200) {
                        try {
                            const info = JSON.parse(data);
                            if (info.version !== pkg.version) {
                                console.log(`\n📦 Update available: ${info.version} (current: ${pkg.version})`);
                                console.log(`   Run: npm install -g ${pkg.name}`);
                                console.log(`   Or:  pnpm add -g ${pkg.name}\n`);
                            }
                        } catch (e) { }
                    }
                    resolve();
                });
            });
            req.on('error', () => resolve()); // Fail silently
            req.setTimeout(1000, () => { req.destroy(); resolve(); }); // Don't block startup too long
        });
    } catch (e) {
        // ignore
    }
}

// Main logic
async function main() {
    try {
        // Start update check in background (await it later or let it print whenever if we don't want to block)
        // Checking strict block might slow down CLI. Let's await it with timeout effectively handled above.
        await checkForPackageUpdate();

        // 1. Ensure Engine
        // Check if engine is in path, or managed by @frumu-ltd/tandem-engine, or in cache.
        // We will favor our managed cache for "tandem-ai" CLI user.
        // But if global `npm install -g @frumu-ltd/tandem-engine` was used, maybe use that?
        // Let's stick to self-contained cache for simplicity as requested.

        console.log("Checking Tandem Engine...");
        const enginePath = await ensureBinary(ENGINE_BINARY_NAME, 'tandem-engine');

        // Check if engine is running? Or just spawn it?
        // The TUI likely expects to connect to it. TUI might spawn it itself if it was built that way?
        // Current TUI implementation assumes it connects to localhost:3000.
        // We should spawn the engine in background if not running.

        // Check port 3000? Or just try to spawn and handle "address in use"?
        // Let's spawn child process detached.

        // Actually, TUI expects engine to be running.
        // Let's try to start it.

        // 2. Ensure TUI
        console.log("Checking Tandem TUI...");
        const tuiPath = await ensureBinary(TUI_BINARY_NAME, 'tandem-tui');

        // 3. Launch
        console.log("Launching...");

        // Start engine (if not running - complex to detect reliably without port check, 
        // but we can try to start and ignore error if port bound, or let user manage it).
        // Better: Start engine as a child process of this CLI, pipe output?
        // Or start detached so it survives?
        // "tandem desktop app will require downloading the tandem-engine... old workflow was using opencode".

        // For TUI, usually you want the engine running.
        // We can spawn it. use `spawn`.

        // Note: If engine is already running (e.g. from Desktop app), we might conflict or just connect.
        // For beta simplicity: Just run the TUI. The TUI currently panics if no engine? 
        // Or TUI has a way to start it?
        // "tandem tui cli... engine also likely needs to live in git repo... tandem tui should also download tandem-engine if not installed"

        // I will SPAWN the engine first.
        const engine = spawn(enginePath, [], {
            detached: false, // For now, keep attached so closing TUI closes engine? Or detached?
            stdio: 'ignore' // or 'inherit' for debug?
        });

        engine.on('error', (err) => console.log("Engine start error (might be already running):", err.message));

        // Give it a moment? or TUI retries connection?
        await new Promise(r => setTimeout(r, 1000));

        // Run TUI
        // We replace current process or spawn and wait?
        // Spawn and wait.
        const tui = spawn(tuiPath, process.argv.slice(2), {
            stdio: 'inherit'
        });

        tui.on('close', (code) => {
            engine.kill(); // Kill engine when TUI exits
            process.exit(code);
        });

    } catch (err) {
        console.error("Error:", err.message);
        process.exit(1);
    }
}

main();
