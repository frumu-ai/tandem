#!/bin/bash

# check-build-modes.sh (EAA-09 / TAN-34): prove the three Tandem engine build
# modes compose exactly as designed.
#
# The engine is the `tandem-ai` package (binary `tandem-engine`). The enterprise
# server split defines three build modes, distinguished purely by the engine
# features passed to cargo:
#
#   * public          (no features, optionally `browser`)
#         Lean, single-tenant dev/CI composition. Must NOT pull the enterprise
#         server, the premium governance engine, or the heavyweight local
#         embedding stack (fastembed / ort). Not shipped as a release artifact
#         since TAN-632, but kept compiling as the minimal composition.
#   * enterprise      (`enterprise`) — THE RELEASE DEFAULT (TAN-632)
#         Every released artifact: enterprise routes (connectors, source
#         bindings, cross-tenant grants), premium governance, and Google
#         Drive — everything except the heavyweight local embedding stack
#         (fastembed / ort).
#   * enterprise-full (`enterprise-full`)
#         The hosted Linux asset: everything in `enterprise` plus local
#         embeddings. Must pull all of the heavyweight crates.
#
# Cargo only activates a feature when it is passed via `--features`, so each mode
# is checked with the exact feature set its artifacts are built with. The check
# uses `cargo tree` (dependency resolution only, no compilation) so it is cheap
# and deterministic. On failure the offending dependency path is printed via
# `cargo tree -i` so the unexpected edge is easy to locate.
#
# This guard supersedes the public-only exclusion check (check-public-build-
# exclusions.sh) by validating all three modes; it delegates the public mode to
# that script so the EAA-11 guard remains the single source of truth for the
# public exclusion set.

set -euo pipefail

ENGINE_PACKAGE="tandem-ai"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# Crates whose presence/absence distinguishes the build modes.
ENTERPRISE_SERVER="tandem-enterprise-server"
GOVERNANCE_ENGINE="tandem-governance-engine"
FASTEMBED="fastembed"
ORT="ort-sys"

violations=0

# Flat, de-duplicated list of normal (non-dev, non-build) dependencies for the
# engine built with the given feature set. `--prefix none` yields one package
# per line as "<name> v<version> (<source>)".
mode_tree() {
  local features="$1"
  local feature_args=(--no-default-features)
  if [ -n "${features}" ]; then
    feature_args+=(--features "${features}")
  fi
  cargo tree --package "${ENGINE_PACKAGE}" "${feature_args[@]}" --edges normal --prefix none 2>/dev/null | sort -u
}

# Match a package name at the start of a line followed by a space, so e.g.
# "ort-sys" never matches "ort-sys-something" and substrings never false-match.
tree_contains() {
  printf '%s\n' "$1" | grep -qE "^$2 "
}

print_inverted_path() {
  local features="$1" crate="$2"
  local feature_args=(--no-default-features)
  if [ -n "${features}" ]; then
    feature_args+=(--features "${features}")
  fi
  cargo tree --package "${ENGINE_PACKAGE}" "${feature_args[@]}" --edges normal --invert "${crate}" 2>/dev/null \
    | sed 's/^/         /' >&2 || true
}

# Assert that the given mode's dependency tree includes every crate in
# `must_include` and excludes every crate in `must_exclude` (space-separated).
assert_mode() {
  local label="$1" features="$2" must_include="$3" must_exclude="$4"

  echo "Checking ${label} (features: ${features:-<default>})..."

  local tree
  tree="$(mode_tree "${features}")"
  if [ -z "${tree}" ]; then
    echo "ERROR: could not resolve the dependency tree for ${label}." >&2
    violations=$((violations + 1))
    return
  fi

  local crate
  for crate in ${must_include}; do
    if ! tree_contains "${tree}" "${crate}"; then
      violations=$((violations + 1))
      echo "ERROR: ${label} must INCLUDE '${crate}', but it is absent from the build." >&2
    fi
  done
  for crate in ${must_exclude}; do
    if tree_contains "${tree}" "${crate}"; then
      violations=$((violations + 1))
      echo "ERROR: ${label} must EXCLUDE '${crate}', but it is reachable." >&2
      echo "       Offending dependency path:" >&2
      print_inverted_path "${features}" "${crate}"
    fi
  done
}

# --- public mode: delegate to the EAA-11 exclusion guard ------------------------
echo "== public build mode =="
if ! bash "${SCRIPT_DIR}/check-public-build-exclusions.sh"; then
  violations=$((violations + 1))
fi
echo

# --- enterprise: the standard release artifact (TAN-632) ------------------------
# Every released binary ships enterprise routes + premium governance; only the
# heavyweight local-embedding stack stays out (hosted Linux asset carries it
# via enterprise-full).
echo "== enterprise (release default) build mode =="
assert_mode "enterprise" "enterprise" \
  "${ENTERPRISE_SERVER} ${GOVERNANCE_ENGINE}" \
  "${FASTEMBED} ${ORT}"
echo

# --- enterprise-full: the complete enterprise stack -----------------------------
echo "== enterprise-full build mode =="
assert_mode "enterprise-full" "enterprise-full" \
  "${ENTERPRISE_SERVER} ${GOVERNANCE_ENGINE} ${FASTEMBED} ${ORT}" \
  ""
echo

if [ "${violations}" -ne 0 ]; then
  echo "Build-mode validation failed: ${violations} issue(s) found." >&2
  echo "The public/enterprise/enterprise-full feature composition no longer matches the enterprise server split." >&2
  exit 1
fi

echo "OK: public, enterprise (release default), and enterprise-full build modes compose as designed."
