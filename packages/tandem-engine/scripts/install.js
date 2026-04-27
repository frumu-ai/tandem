const fs = require('fs');
const path = require('path');
const https = require('https');
const { execFileSync, execSync } = require('child_process');

// Configuration
const REPO = "frumu-ai/tandem";
const MIN_SIZE = 1024 * 1024; // 1MB

// Platform mapping
const PLATFORM_MAP = {
    'win32': { os: 'windows', ext: '.exe' },
    'darwin': { os: 'darwin', ext: '' },
    'linux': { os: 'linux', ext: '' }
};

const ARCH_MAP = {
    'x64': 'x64',
    'arm64': 'arm64'
};

function getArtifactInfo() {
    const platform = PLATFORM_MAP[process.platform];
    const arch = ARCH_MAP[process.arch];

    if (!platform || !arch) {
        throw new Error(`Unsupported platform: ${process.platform}-${process.arch}`);
    }

    let artifactName = `tandem-engine-${platform.os}-${arch}`;
    // Handle specific artifact naming conventions (zip vs tar.gz)
    if (platform.os === 'windows') {
        artifactName += '.zip';
    } else if (platform.os === 'darwin') {
        artifactName += '.zip';
    } else {
        artifactName += '.tar.gz';
    }

    return {
        artifactName,
        binaryName: `tandem-engine${platform.ext}`,
        isWindows: platform.os === 'windows'
    };
}

const { artifactName, binaryName, isWindows } = getArtifactInfo();
const binDir = path.join(__dirname, '..', 'bin', 'native');
const destPath = path.join(binDir, binaryName);

function buildHeaders(userAgent) {
    const headers = { 'User-Agent': userAgent };
    const token = process.env.GITHUB_TOKEN || process.env.GH_TOKEN;
    if (token) {
        headers.Authorization = `Bearer ${token}`;
    }
    return headers;
}

function warnAndExit(err) {
    const detail = err && err.message ? err.message : String(err);
    console.warn(`Warning: tandem-engine binary download skipped: ${detail}`);
    console.warn(
        "Install completed without a bundled binary. Runtime commands will require a later reinstall or a preinstalled tandem-engine binary."
    );
    process.exit(0);
}

if (!fs.existsSync(binDir)) {
    fs.mkdirSync(binDir, { recursive: true });
}

function parseVersion(raw) {
    const match = String(raw || '').match(/\b(\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.-]+)?)\b/);
    return match ? match[1] : "";
}

function installedBinaryVersion(binaryPath, execFile = execFileSync) {
    if (!fs.existsSync(binaryPath)) return "";
    try {
        const output = execFile(binaryPath, ['--version'], {
            encoding: 'utf8',
            stdio: ['ignore', 'pipe', 'ignore'],
            timeout: 5000,
        });
        return parseVersion(output);
    } catch {
        return "";
    }
}

function shouldDownloadBinary(binaryPath, packageVersion, readVersion = installedBinaryVersion) {
    if (!fs.existsSync(binaryPath)) {
        return { download: true, reason: "missing" };
    }

    const stats = fs.statSync(binaryPath);
    if (stats.size < MIN_SIZE) {
        return { download: true, reason: `too small (${stats.size} bytes)` };
    }

    const installedVersion = readVersion(binaryPath);
    if (!installedVersion) {
        return { download: true, reason: "version check failed" };
    }
    if (installedVersion !== packageVersion) {
        return {
            download: true,
            reason: `version mismatch (${installedVersion} != ${packageVersion})`,
        };
    }

    return { download: false, reason: `version ${installedVersion} already installed` };
}

// Helper to fetch JSON from GitHub API
function fetchJson(url) {
    return new Promise((resolve, reject) => {
        https.get(url, { headers: buildHeaders('tandem-engine-installer') }, (res) => {
            if (res.statusCode !== 200) {
                if (res.statusCode === 302 || res.statusCode === 301) {
                    return fetchJson(res.headers.location).then(resolve).catch(reject);
                }
                return reject(new Error(`GitHub API HTTP ${res.statusCode}`));
            }
            let data = '';
            res.on('data', chunk => data += chunk);
            res.on('end', () => {
                try { resolve(JSON.parse(data)); } catch (e) { reject(e); }
            });
        }).on('error', reject);
    });
}

// Simplified download
async function download() {
    console.log(`Checking releases for ${REPO}...`);
    const releases = await fetchJson(`https://api.github.com/repos/${REPO}/releases`);

    // Get the version from package.json
    const packageVersion = require('../package.json').version;
    const targetTag = `v${packageVersion}`;

    console.log(`Filtering releases for ${REPO} (Target: ${targetTag})...`);
    // const releases = await fetchJson... <--- REMOVED DUPLICATE

    // 1. Try to find the exact release for this package version
    let release = releases.find(r => r.tag_name === targetTag);

    if (!release) {
        console.warn(`Warning: No release found for tag ${targetTag}. Checking for latest compatible assets...`);
        // 2. Fallback: Find LATEST release that contains our asset (useful for nightly/beta where tags might differ)
        release = releases.find(r => r.assets.some(a => a.name === artifactName));
    }

    if (!release) {
        // Fallback: Check prereleases explicitly if strict filtering was on (it wasn't here)
        // If not found, maybe name changed?
        console.error(`Status: No release found with asset ${artifactName}`);
        console.error("Available assets in latest:", releases[0]?.assets?.map(a => a.name));
        process.exit(1);
    }

    const asset = release.assets.find(a => a.name === artifactName);
    console.log(`Downloading ${asset.name} from ${release.tag_name}...`);

    const file = fs.createWriteStream(path.join(binDir, artifactName));

    return new Promise((resolve, reject) => {
        const downloadUrl = asset.browser_download_url;

        const request = (url) => {
            https.get(url, { headers: buildHeaders('tandem-installer') }, (res) => {
                if (res.statusCode === 302 || res.statusCode === 301) {
                    return request(res.headers.location);
                }
                if (res.statusCode !== 200) return reject(new Error(`Download failed: HTTP ${res.statusCode}`));
                res.pipe(file);
                file.on('finish', () => {
                    file.close();
                    resolve(path.join(binDir, artifactName));
                });
            }).on('error', err => {
                fs.unlink(path.join(binDir, artifactName), () => { }); // cleanup
                reject(err);
            });
        };
        request(downloadUrl);
    });
}

// Extract
async function extract(archivePath) {
    console.log("Extracting...");
    if (isWindows) {
        execSync(`powershell -Command "Expand-Archive -Path '${archivePath}' -DestinationPath '${binDir}' -Force"`);
    } else {
        if (artifactName.endsWith('.zip')) {
            execSync(`unzip -o "${archivePath}" -d "${binDir}"`);
        } else {
            execSync(`tar -xzf "${archivePath}" -C "${binDir}"`);
        }
    }

    // Cleanup archive
    fs.unlinkSync(archivePath);

    // Locate binary (it might be inside a folder?)
    // Our release workflow:
    // Windows: dist/tandem-engine.exe -> zipped -> dist/tandem-engine.exe
    // Linux: dist/tandem-engine -> tar -> dist/tandem-engine
    // So on extraction, it might extract 'dist/tandem-engine' or just 'tandem-engine'.
    // We should check.

    // If extraction creates a folder (common behavior), we need to find it.
    // Assuming root extraction for now based on GHA inspect.
    // 'dist' folder? Yes. The GHA zips "dist/*". So it extracts "tandem-engine.exe" directly if zip didn't preserve root?
    // "Compress-Archive -Path "dist/*" ... " -> This usually puts files at root of zip.

    if (fs.existsSync(destPath)) {
        console.log("Verified binary extracted.");
        if (!isWindows) fs.chmodSync(destPath, 0o755);
    } else {
        console.error("Binary not found at expected path:", destPath);
        // List files
        console.log("Files in bin:", fs.readdirSync(binDir));
        process.exit(1);
    }
}

async function main() {
    const packageVersion = require('../package.json').version;
    const decision = shouldDownloadBinary(destPath, packageVersion);
    if (!decision.download) {
        console.log(`Binary already present (${decision.reason}).`);
        return;
    }
    if (decision.reason !== "missing") {
        console.log(`Existing binary will be replaced: ${decision.reason}.`);
    }
    const archivePath = await download();
    await extract(archivePath);
}

if (require.main === module) {
    main().catch(warnAndExit);
}

module.exports = {
    installedBinaryVersion,
    parseVersion,
    shouldDownloadBinary,
};
