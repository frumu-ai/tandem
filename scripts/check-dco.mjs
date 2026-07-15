#!/usr/bin/env node

// Enforces the Developer Certificate of Origin (DCO) sign-off policy
// described in CONTRIBUTING.md: every human-authored commit in the range
// under review must carry a Signed-off-by trailer matching the commit
// author. Only the allowlisted automation identities below are exempt.
//
// Merge commits are skipped, matching standard DCO tooling: GitHub's
// "Update branch" button and merge queue create merge commits without
// sign-off trailers, and the merged changes themselves are validated in
// the non-merge commits that introduced them.
//
// Range selection:
//   - In a pull request, GITHUB_BASE_REF is set and the range is
//     origin/<base>..HEAD.
//   - Otherwise (pushes, local runs) the range is origin/main..HEAD, and
//     an empty range passes.

import { spawnSync } from "node:child_process";
import process from "node:process";

const baseRef = process.env.GITHUB_BASE_REF || "main";

// Exact author emails of repository-operated automation. Do NOT loosen this
// to a pattern like /\[bot\]/ — author metadata is contributor-controlled,
// and a broad match would let anyone bypass the sign-off check by naming
// themselves a bot.
const EXEMPT_AUTHOR_EMAILS = new Set([
  "41898282+github-actions[bot]@users.noreply.github.com",
  "github-actions[bot]@users.noreply.github.com",
  "49699333+dependabot[bot]@users.noreply.github.com",
]);

function git(...args) {
  const result = spawnSync("git", args, { encoding: "utf8" });
  if (result.status !== 0) {
    console.error(`git ${args.join(" ")} failed:\n${result.stderr}`);
    process.exit(2);
  }
  return result.stdout;
}

// Field separator/record separator unlikely to appear in commit text.
const FS = "";
const RS = "";
const log = git(
  "log",
  "--no-merges",
  `--format=%H${FS}%an${FS}%ae${FS}%P${FS}%B${RS}`,
  `origin/${baseRef}..HEAD`
);

const problems = [];
let checked = 0;

for (const record of log.split(RS)) {
  const trimmed = record.replace(/^\n/, "");
  if (!trimmed) continue;
  const [hash, authorName, authorEmail, , body] = trimmed.split(FS);
  const shortHash = hash.slice(0, 10);

  // Allowlisted automation identities are exempt: DCO certifies third-party
  // provenance, and these commits are produced by repository-owned tooling.
  if (EXEMPT_AUTHOR_EMAILS.has(authorEmail.toLowerCase())) continue;

  checked += 1;
  const signoffs = [...body.matchAll(/^Signed-off-by:\s*(.+?)\s*<(.+?)>\s*$/gim)];
  if (signoffs.length === 0) {
    problems.push(
      `${shortHash} (${authorName} <${authorEmail}>) has no Signed-off-by trailer`
    );
    continue;
  }
  const matchesAuthor = signoffs.some(
    ([, , email]) => email.toLowerCase() === authorEmail.toLowerCase()
  );
  if (!matchesAuthor) {
    problems.push(
      `${shortHash} has Signed-off-by trailers, but none match the author <${authorEmail}>`
    );
  }
}

if (problems.length > 0) {
  console.error("DCO check failed. Each commit must be made with `git commit -s`,");
  console.error("certifying the Developer Certificate of Origin and the licensing");
  console.error("terms in CONTRIBUTING.md.\n");
  for (const problem of problems) console.error(`  - ${problem}`);
  process.exit(1);
}

console.log(`DCO check OK: ${checked} commit(s) signed off.`);
