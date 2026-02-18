const fs = require('fs');
const path = require('path');
const https = require('https');
const { execSync } = require('child_process');

const REPO = "frumu-ai/tandem";
const MIN_SIZE = 1024 * 1024;

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

    let artifactName = `tandem-tui-${platform.os}-${arch}`;
    if (platform.os === 'windows') {
        artifactName += '.zip';
    } else if (platform.os === 'darwin') {
        artifactName += '.zip';
    } else {
        artifactName += '.tar.gz';
    }

    return {
        artifactName,
        binaryName: `tandem-tui${platform.ext}`,
        isWindows: platform.os === 'windows'
    };
}

const { artifactName, binaryName, isWindows } = getArtifactInfo();
const binDir = path.join(__dirname, '..', 'bin');
const destPath = path.join(binDir, binaryName);

if (!fs.existsSync(binDir)) {
    fs.mkdirSync(binDir, { recursive: true });
}

if (fs.existsSync(destPath)) {
    console.log("Binary already present.");
    process.exit(0);
}

function fetchJson(url) {
    return new Promise((resolve, reject) => {
        https.get(url, { headers: { 'User-Agent': 'tandem-tui-installer' } }, (res) => {
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

async function download() {
    console.log(`Checking releases for ${REPO}...`);
    const releases = await fetchJson(`https://api.github.com/repos/${REPO}/releases`);

    const packageVersion = require('../package.json').version;
    const targetTag = `v${packageVersion}`;

    console.log(`Filtering releases for ${REPO} (Target: ${targetTag})...`);
    let release = releases.find(r => r.tag_name === targetTag);

    if (!release) {
        console.warn(`Warning: No release found for tag ${targetTag}. Checking for latest compatible assets...`);
        release = releases.find(r => r.assets.some(a => a.name === artifactName));
    }

    if (!release) {
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
            https.get(url, { headers: { 'User-Agent': 'tandem-tui-installer' } }, (res) => {
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
                fs.unlink(path.join(binDir, artifactName), () => { });
                reject(err);
            });
        };
        request(downloadUrl);
    });
}

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

    fs.unlinkSync(archivePath);

    if (fs.existsSync(destPath)) {
        console.log("Verified binary extracted.");
        if (!isWindows) fs.chmodSync(destPath, 0o755);
    } else {
        console.error("Binary not found at expected path:", destPath);
        console.log("Files in bin:", fs.readdirSync(binDir));
        process.exit(1);
    }
}

download().then(extract).catch(err => {
    console.error("Install failed:", err);
    process.exit(1);
});
