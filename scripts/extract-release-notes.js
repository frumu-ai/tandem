#!/usr/bin/env node
/**
 * Extract release notes from CHANGELOG.md for a given version
 * Usage: node scripts/extract-release-notes.js v0.1.0
 */

const fs = require('fs');
const path = require('path');

const version = process.argv[2];
if (!version) {
  console.error('Usage: node extract-release-notes.js <version>');
  process.exit(1);
}

// Remove 'v' prefix if present
const versionNumber = version.replace(/^v/, '');

const changelogPath = path.join(__dirname, '..', 'CHANGELOG.md');
const changelog = fs.readFileSync(changelogPath, 'utf8');

// Find the section for this version
const versionRegex = new RegExp(`## \\[${versionNumber}\\]([\\s\\S]*?)(?=## \\[|$)`, 'i');
const match = changelog.match(versionRegex);

if (!match) {
  // If not found, try to extract from [Unreleased]
  const unreleasedRegex = /## \[Unreleased\]([\s\S]*?)(?=## \[|$)/i;
  const unreleasedMatch = changelog.match(unreleasedRegex);
  
  if (unreleasedMatch) {
    console.log(`# Release ${version}\n`);
    console.log(unreleasedMatch[1].trim());
    console.log('\n---\n');
    console.log('**Full Changelog**: See commits since last release');
  } else {
    console.log(`# Release ${version}\n`);
    console.log('See the assets below to download the installer for your platform.\n');
    console.log('## What\'s Changed\n');
    console.log('This release includes bug fixes and improvements.\n');
  }
} else {
  console.log(`# Release ${version}\n`);
  console.log(match[1].trim());
  console.log('\n---\n');
  console.log(`**Full Changelog**: https://github.com/frumu-ai/tandem/compare/v${getPreviousVersion(changelog, versionNumber)}...v${versionNumber}`);
}

function getPreviousVersion(changelog, currentVersion) {
  const versions = [];
  const versionRegex = /## \[(\d+\.\d+\.\d+)\]/g;
  let match;
  
  while ((match = versionRegex.exec(changelog)) !== null) {
    versions.push(match[1]);
  }
  
  const currentIndex = versions.indexOf(currentVersion);
  return currentIndex < versions.length - 1 ? versions[currentIndex + 1] : currentVersion;
}
