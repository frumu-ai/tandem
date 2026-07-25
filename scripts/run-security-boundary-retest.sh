#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

node --test scripts/verify-security-retest-matrix.test.mjs
node scripts/verify-security-retest-matrix.mjs --self-test
node scripts/verify-security-retest-matrix.mjs

security_filter="$(node scripts/verify-security-retest-matrix.mjs --print-nextest-filter)"
cargo nextest run --workspace --exclude tandem \
  --features tandem-ai/browser,tandem-server/premium-governance \
  --profile ci \
  -E "$security_filter"
