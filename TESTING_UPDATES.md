# Testing Tauri Auto-Updates

## Prerequisites

1. GitHub secret configured:
   - `TAURI_SIGNING_PRIVATE_KEY` - Your private key from `~/.tauri/tandem.key`

   **Note**: No password secret needed since the key was generated with `--ci` flag (passwordless)

## Test Scenario: Version 0.1.0 → 0.2.0

### Step 0: Prepare Release Notes (Optional)

Before creating a release, you can update `CHANGELOG.md`:

```markdown
## [0.2.0] - 2026-01-XX

### Added

- New feature X
- New feature Y

### Fixed

- Bug Z
```

Or let GitHub auto-generate notes from commits (current setup).

### Step 1: Create Initial Release (v0.1.0)

```powershell
# Ensure version is 0.1.0 in tauri.conf.json
git add .
git commit -m "Release v0.1.0"
git tag v0.1.0
git push origin main --tags
```

Wait for GitHub Actions to:

- Build installers for all platforms
- Sign them
- Create `latest.json`
- Publish the release

### Step 2: Install v0.1.0 on Your Machine

1. Go to: https://github.com/frumu-ai/tandem/releases/tag/v0.1.0
2. Download the installer for your platform:
   - Windows: `Tandem_0.1.0_x64-setup.nsis.zip` (extract and run)
   - macOS: `Tandem_0.1.0_x64.dmg`
   - Linux: `tandem_0.1.0_amd64.AppImage`
3. Install and run the app

### Step 3: Create a New Release (v0.2.0)

```powershell
# Update version in tauri.conf.json to 0.2.0
# Make a visible change (e.g., update About page text)

git add .
git commit -m "Release v0.2.0"
git tag v0.2.0
git push origin main --tags
```

Wait for GitHub Actions to complete.

### Step 4: Test the Update

1. Open your **installed** v0.1.0 app (not dev mode!)
2. Navigate to About section
3. Click "Check for updates"
4. Should show: "Update available: v0.2.0"
5. Click "Install update"
6. App should download, install, and relaunch with v0.2.0

## Testing in Development Mode

**Important**: Auto-updates **do not work** in dev mode (`pnpm tauri dev`). You must:

1. Build a production installer
2. Install it
3. Run the installed app

## Troubleshooting

### Update Check Fails

- Check that `latest.json` exists at the endpoint
- Verify the GitHub release is **published** (not draft)
- Check browser console for error messages

### Signature Verification Fails

- Ensure the private key secret matches the public key in tauri.conf.json
- Verify GitHub secrets are set correctly
- Check that `createUpdaterArtifacts: true` is in bundle config

### Latest.json Not Found (404)

- GitHub Actions may have failed - check the workflow logs
- Release might still be a draft - publish it manually
- Ensure the release has the `latest.json` file attached

## Simulating Updates Locally (Advanced)

You can test without creating real releases:

1. Build two versions locally with different version numbers
2. Host `latest.json` and the second installer on a local server
3. Update the endpoint in tauri.conf.json temporarily to point to `http://localhost:3000/latest.json`

## What Gets Updated

- **The Tandem app itself** - Yes ✅
- **OpenCode sidecar** - No, that's managed separately via the sidecar downloader

The OpenCode binary updates independently when:

- User clicks "Update Now" in the sidecar downloader UI
- Or when the app detects a new version on launch
