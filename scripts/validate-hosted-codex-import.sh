#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
Usage: scripts/validate-hosted-codex-import.sh [--dry-run] [--keep-workdir]

Creates an isolated temp CODEX_HOME, imports a sample Codex auth.json through
the real tandem-core helper, and confirms the credential can be loaded back.
EOF
}

dry_run=0
keep_workdir=0

while [[ $# -gt 0 ]]; do
  case "$1" in
    --dry-run)
      dry_run=1
      shift
      ;;
    --keep-workdir)
      keep_workdir=1
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "unknown argument: $1" >&2
      usage >&2
      exit 1
      ;;
  esac
done

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
workdir="$(mktemp -d "${TMPDIR:-/tmp}/tandem-hosted-codex-import.XXXXXX")"
sample_auth_json="$workdir/sample-auth.json"
temp_home="$workdir/home"
temp_codex_home="$workdir/codex"
cargo_dir="$workdir/harness"

cleanup() {
  local exit_code=$?
  if [[ $exit_code -eq 0 && $keep_workdir -eq 0 ]]; then
    rm -rf "$workdir"
  else
    echo "workdir preserved at: $workdir" >&2
  fi
  exit "$exit_code"
}
trap cleanup EXIT

mkdir -p "$temp_home" "$temp_codex_home" "$cargo_dir/src"

sample_access_token="$(
  python3 - <<'PY'
import base64
import json

def b64url(obj):
    raw = json.dumps(obj, separators=(",", ":")).encode("utf-8")
    return base64.urlsafe_b64encode(raw).decode("ascii").rstrip("=")

header = {"alg": "none", "typ": "JWT"}
payload = {
    "exp": 2000000000,
    "sub": "google-oauth2|validator",
    "https://api.openai.com/profile": {
        "email": "validator@example.com",
        "email_verified": True,
    },
    "https://api.openai.com/auth": {
        "chatgpt_account_id": "acct_validator",
        "chatgpt_account_user_id": "user_validator",
        "chatgpt_user_id": "user_validator",
    },
}
print(f"{b64url(header)}.{b64url(payload)}.")
PY
)"

cat >"$sample_auth_json" <<EOF
{
  "auth_mode": "chatgpt",
  "tokens": {
    "access_token": "$sample_access_token",
    "refresh_token": "rt_validator_sample_0123456789",
    "account_id": "acct_validator"
  },
  "last_refresh": 2000000000000
}
EOF

cat >"$cargo_dir/Cargo.toml" <<EOF
[package]
name = "codex-import-validation"
version = "0.1.0"
edition = "2021"

[dependencies]
anyhow = "1"
serde_json = "1"
tandem-core = { path = "$repo_root/crates/tandem-core" }
EOF

cat >"$cargo_dir/src/main.rs" <<'EOF'
use anyhow::{Context, Result};
use serde_json::Value;
use std::env;
use std::path::PathBuf;

fn main() -> Result<()> {
    let validation_home = env::var("VALIDATION_HOME").context("VALIDATION_HOME not set")?;
    let codex_home = env::var("CODEX_HOME").context("CODEX_HOME not set")?;
    let sample_auth_json = env::var("SAMPLE_AUTH_JSON_PATH")
        .context("SAMPLE_AUTH_JSON_PATH not set")?;

    env::set_var("HOME", &validation_home);
    env::set_var("CODEX_HOME", &codex_home);

    let sample_raw = std::fs::read_to_string(&sample_auth_json)
        .with_context(|| format!("failed to read sample auth json at {sample_auth_json}"))?;
    let sample_value: Value = serde_json::from_str(&sample_raw)
        .context("sample Codex auth.json is not valid JSON")?;

    let written_path = tandem_core::write_openai_codex_cli_auth_json(&sample_value)
        .context("failed to write Codex auth.json into temp CODEX_HOME")?;
    let expected_auth_path = PathBuf::from(&codex_home).join("auth.json");
    anyhow::ensure!(
        written_path == expected_auth_path,
        "Codex auth.json was written to {:?}, expected {:?}",
        written_path,
        expected_auth_path
    );
    anyhow::ensure!(
        expected_auth_path.exists(),
        "expected Codex auth.json to exist at {:?}",
        expected_auth_path
    );

    let loaded = tandem_core::load_openai_codex_cli_oauth_credential()
        .context("failed to reload Codex credential from temp CODEX_HOME")?;
    anyhow::ensure!(
        loaded.provider_id == "openai-codex",
        "unexpected provider_id {:?}",
        loaded.provider_id
    );
    anyhow::ensure!(
        loaded.email.as_deref() == Some("validator@example.com"),
        "unexpected loaded email {:?}",
        loaded.email
    );
    anyhow::ensure!(
        loaded.managed_by == "codex-cli",
        "unexpected managed_by {:?}",
        loaded.managed_by
    );

    let mut imported = loaded.clone();
    imported.managed_by = "codex-upload".to_string();
    tandem_core::set_provider_oauth_credential("openai-codex", imported.clone())
        .context("failed to persist provider credential")?;

    let stored = tandem_core::load_provider_oauth_credential("openai-codex")
        .context("failed to load provider credential after persistence")?;
    anyhow::ensure!(
        stored.managed_by == "codex-upload",
        "unexpected persisted managed_by {:?}",
        stored.managed_by
    );
    anyhow::ensure!(
        stored.email.as_deref() == Some("validator@example.com"),
        "unexpected persisted email {:?}",
        stored.email
    );
    anyhow::ensure!(
        stored.expires_at_ms > 0,
        "persisted credential expires_at_ms was not populated"
    );

    let status = if stored.expires_at_ms > current_ms() {
        "connected"
    } else {
        "reauth_required"
    };

    println!("[ok] wrote sample auth.json to {expected_auth_path:?}");
    println!(
        "[ok] loaded Codex credential: provider_id={} email={:?} managed_by={}",
        loaded.provider_id, loaded.email, loaded.managed_by
    );
    println!(
        "[ok] persisted provider credential: status={} email={:?} managed_by={}",
        status, stored.email, stored.managed_by
    );

    Ok(())
}

fn current_ms() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}
EOF

if [[ $dry_run -eq 1 ]]; then
  cat <<EOF
[dry-run] workdir: $workdir
[dry-run] sample auth.json: $sample_auth_json
[dry-run] temp HOME: $temp_home
[dry-run] temp CODEX_HOME: $temp_codex_home
[dry-run] harness: $cargo_dir
EOF
  exit 0
fi

VALIDATION_HOME="$temp_home" \
CODEX_HOME="$temp_codex_home" \
SAMPLE_AUTH_JSON_PATH="$sample_auth_json" \
cargo run --quiet --manifest-path "$cargo_dir/Cargo.toml"
