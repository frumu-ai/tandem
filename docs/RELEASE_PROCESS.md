# Release Process

This document outlines the steps to create and publish a new release of Tandem.

## Overview

Tandem uses **Git tags** to trigger automated builds and releases. When you push a tag matching the pattern `v*.*.*` (e.g., `v0.1.10`), GitHub Actions automatically:

- Builds the application for all platforms (Windows, macOS, Linux)
- Creates a GitHub Release with the built artifacts
- Publishes the release notes

## Prerequisites

Before creating a release, ensure:

- [ ] All changes are committed and pushed to `main`
- [ ] `CHANGELOG.md` is updated with the new version
- [ ] `docs/RELEASE_NOTES.md` is updated with detailed release notes
- [ ] **Version numbers are updated in ALL three files** (critical for auto-updater):
  - `src-tauri/tauri.conf.json` - **REQUIRED** (this is what the app reports as its version)
  - `package.json` - **REQUIRED**
  - `src-tauri/Cargo.toml` - **REQUIRED**

> [!CAUTION]
> **DO NOT create a release tag without updating all three version numbers first!** The auto-updater will fail if version numbers are mismatched. Always verify with:
>
> ```bash
> grep '"version"' src-tauri/tauri.conf.json
> grep '"version"' package.json
> grep '^version' src-tauri/Cargo.toml
> ```

## Release Steps

### 1. Create a Git Tag

Create an annotated tag with the version number:

```bash
git tag -a v0.1.10 -m "Release v0.1.10: Skills Management"
```

**Format:**

- Tag name: `v<MAJOR>.<MINOR>.<PATCH>` (e.g., `v0.1.10`)
- Message: Brief description of the release (e.g., "Release v0.1.10: Skills Management")

### 2. Push the Tag

Push the tag to trigger the automated build:

```bash
git push origin v0.1.10
```

Or push all tags at once:

```bash
git push --tags
```

### 3. Monitor the Build

1. Go to the [GitHub Actions page](https://github.com/frumu-ai/tandem/actions)
2. Look for the workflow run triggered by your tag
3. Wait for the build to complete (usually 10-20 minutes)
4. Check for any build failures

### 4. Verify the Release

Once the build completes:

1. Go to the [Releases page](https://github.com/frumu-ai/tandem/releases)
2. Verify the new release is published
3. Check that all platform binaries are attached
4. Review the auto-generated release notes

## Quick Reference

```bash
# Create and push a release tag (all in one go)
git tag -a v0.1.10 -m "Release v0.1.10: Skills Management"
git push origin v0.1.10

# List all tags
git tag -l

# Delete a local tag (if you made a mistake)
git tag -d v0.1.10

# Delete a remote tag (if you need to redo)
git push origin --delete v0.1.10
```

## Versioning Guidelines

We follow [Semantic Versioning](https://semver.org/):

- **MAJOR** (`1.0.0`): Breaking changes, major rewrites
- **MINOR** (`0.1.0`): New features, non-breaking changes
- **PATCH** (`0.0.1`): Bug fixes, minor improvements

### Current Phase (Pre-1.0)

Since we're in the `0.x.x` phase:

- Increment **MINOR** for new features (e.g., `0.1.0` → `0.2.0`)
- Increment **PATCH** for bug fixes (e.g., `0.1.0` → `0.1.1`)

## Troubleshooting

### Tag Already Exists

If you get an error that the tag already exists:

```bash
# Delete the local tag
git tag -d v0.1.10

# Delete the remote tag (if pushed)
git push origin --delete v0.1.10

# Recreate the tag
git tag -a v0.1.10 -m "Release v0.1.10: Skills Management"
git push origin v0.1.10
```

### Build Fails

If the automated build fails:

1. Check the GitHub Actions logs for errors
2. Fix any issues in the code
3. Delete and recreate the tag (see above)
4. Push the tag again

### Wrong Commit Tagged

If you tagged the wrong commit:

```bash
# Delete the tag
git tag -d v0.1.10
git push origin --delete v0.1.10

# Checkout the correct commit
git checkout <correct-commit-hash>

# Create the tag on the correct commit
git tag -a v0.1.10 -m "Release v0.1.10: Skills Management"
git push origin v0.1.10
```

## Post-Release

After a successful release:

- [ ] Announce the release (Discord, Twitter, etc.)
- [ ] Update any documentation that references version numbers
- [ ] Start a new section in `CHANGELOG.md` for the next version
- [ ] Close any related GitHub issues/milestones

## Example Workflow

Here's a complete example for releasing v0.1.10:

```bash
# 1. Ensure you're on main with latest changes
git checkout main
git pull

# 2. Verify all changes are committed
git status

# 3. Create the tag
git tag -a v0.1.10 -m "Release v0.1.10: Skills Management"

# 4. Push the tag
git push origin v0.1.10

# 5. Monitor the build at:
# https://github.com/frumu-ai/tandem/actions

# 6. Once complete, verify at:
# https://github.com/frumu-ai/tandem/releases
```

---

**Need help?** Check the [GitHub Actions documentation](https://docs.github.com/en/actions) or review previous releases for reference.
