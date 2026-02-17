#!/usr/bin/env node
/**
 * Extract release notes for a given tag/version.
 *
 * Sources (in priority order):
 * - docs/WHATS_NEW_vX.Y.Z.md (for focused release messaging)
 * - docs/RELEASE_NOTES.md (per-version sections)
 * - CHANGELOG.md (Keep a Changelog style)
 *
 * Usage:
 *   node scripts/extract-release-notes.js v0.1.4
 */

import fs from 'node:fs';
import path from 'node:path';
import { execSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

const input = process.argv[2];
if (!input) {
  console.error('Usage: node scripts/extract-release-notes.js <tag-or-version>');
  process.exit(1);
}

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
const repoRoot = path.resolve(__dirname, '..');

const raw = input.replace(/^refs\/tags\//, '');
const versionNumber = raw.replace(/^v/, '');
const baseVersion = versionNumber.replace(/[-+].*$/, '');
const currentTag = raw.startsWith('v') ? raw : `v${versionNumber}`;

const repo =
  process.env.GITHUB_REPOSITORY ||
  process.env.REPO ||
  // fallback for local runs
  'frumu-ai/tandem';

const whatsNewPath = path.join(repoRoot, 'docs', `WHATS_NEW_v${baseVersion}.md`);
const releaseNotesPath = path.join(repoRoot, 'docs', 'RELEASE_NOTES.md');
const changelogPath = path.join(repoRoot, 'CHANGELOG.md');

const intro = 'See the assets below to download the installer for your platform.';

const fromWhatsNew = safeRead(whatsNewPath);
const fromReleaseNotes = safeRead(releaseNotesPath);
const fromChangelog = safeRead(changelogPath);

const versionCandidates = Array.from(new Set([versionNumber, baseVersion]));

const extracted =
  (fromWhatsNew ? extractWholeMarkdown(fromWhatsNew) : null) ||
  (fromReleaseNotes
    ? extractFromReleaseNotesMd(fromReleaseNotes, versionCandidates)
    : null) ||
  (fromChangelog ? extractFromChangelog(fromChangelog, versionCandidates) : null) ||
  null;

const previousTag = getPreviousTag(currentTag);
const fullChangelogLine = previousTag
  ? `**Full Changelog**: https://github.com/${repo}/compare/${previousTag}...${currentTag}`
  : null;

const bodyParts = [
  intro,
  extracted?.trim()
    ? stripLeadingTopHeader(extracted.trim())
    : "## What's Changed\n\n- Bug fixes and improvements.",
  fullChangelogLine,
].filter(Boolean);

process.stdout.write(`${bodyParts.join('\n\n')}\n`);

function safeRead(filePath) {
  try {
    return fs.readFileSync(filePath, 'utf8');
  } catch {
    return null;
  }
}

function stripLeadingTopHeader(markdown) {
  // If section begins with a top-level header ("# ..."), drop it to avoid duplicating the GitHub Release title.
  const lines = markdown.split(/\r?\n/);
  if (!lines.length) return markdown;
  if (!/^#\s+/.test(lines[0])) return markdown;

  // Drop first line, and at most one subsequent blank line.
  let start = 1;
  if (lines[start] === '') start += 1;
  return lines.slice(start).join('\n').trim();
}

function extractWholeMarkdown(markdown) {
  return markdown.trim();
}

function extractFromReleaseNotesMd(markdown, versions) {
  for (const version of versions) {
    const section = extractVersionSectionFromReleaseNotes(markdown, version);
    if (section) return section;
  }
  return null;
}

function extractVersionSectionFromReleaseNotes(markdown, version) {
  // Match: "# Tandem v0.1.4 ..." (the docs file uses # for per-version sections)
  const startRe = new RegExp(`^#\\s+Tandem\\s+v${escapeRegExp(version)}\\b.*$`, 'mi');
  const startMatch = markdown.match(startRe);
  if (!startMatch || startMatch.index == null) return null;

  const startIdx = startMatch.index;
  const afterStart = markdown.slice(startIdx);

  // End at next "# Tandem vX.Y.Z" section, or EOF.
  const nextRe = /^#\s+Tandem\s+v\d+\.\d+\.\d+\b.*$/gim;
  nextRe.lastIndex = startMatch[0].length;
  const nextMatch = nextRe.exec(afterStart);
  const endIdx = nextMatch?.index != null ? nextMatch.index : afterStart.length;

  return afterStart.slice(0, endIdx).trim();
}

function extractFromChangelog(markdown, versions) {
  for (const version of versions) {
    // Prefer explicit version section.
    const versionRe = new RegExp(
      `^##\\s+\\[${escapeRegExp(version)}\\]([\\s\\S]*?)(?=^##\\s+\\[|\\Z)`,
      'im'
    );
    const match = markdown.match(versionRe);
    if (match) return "## What's Changed\n\n" + match[1].trim();
  }

  // Fallback to [Unreleased].
  const unreleasedRe = /^##\s+\[Unreleased\]([\s\S]*?)(?=^##\s+\[|\Z)/im;
  const unreleasedMatch = markdown.match(unreleasedRe);
  if (unreleasedMatch) return "## What's Changed\n\n" + unreleasedMatch[1].trim();

  return null;
}

function getPreviousTag(current) {
  try {
    // Requires checkout fetch-depth: 0 (or tags fetched) in CI.
    const tags = execSync('git tag --list "v*" --sort=-version:refname', {
      cwd: repoRoot,
      stdio: ['ignore', 'pipe', 'ignore'],
      encoding: 'utf8',
    })
      .split(/\r?\n/)
      .map((t) => t.trim())
      .filter(Boolean);

    const idx = tags.indexOf(current);
    if (idx === -1) return null;
    return idx < tags.length - 1 ? tags[idx + 1] : null;
  } catch {
    return null;
  }
}

function escapeRegExp(s) {
  return s.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
}
