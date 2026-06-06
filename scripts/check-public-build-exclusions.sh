#!/bin/bash

# check-public-build-exclusions.sh (EAA-11): prove the public Tandem engine build
# does not pull in enterprise-only or heavyweight crates.
#
# The public engine is the `tandem-ai` package (binary `tandem-engine`). It must
# never depend on the enterprise server, the premium governance engine, or the
# heavyweight local-embedding stack (fastembed / ort). Those are opt-in behind the
# `enterprise-server` / `premium-governance` / `local-embeddings` features and
# must stay out of the public build.
#
# Cargo only activates a feature when it is passed via `--features`, so the guard
# must check the exact feature sets the public artifacts are built with — not just
# default features. The public release / desktop sidecar / engine-release builds
# all use `--features tandem-ai/browser` (see .github/workflows/release.yml,
# desktop-release.yml, engine-release.yml), so the browser feature is checked too.
# The enterprise-full build is intentionally excluded: it is the enterprise
# artifact and is allowed to contain these crates.
#
# On failure the offending dependency path is printed (via `cargo tree -i`) so the
# unexpected edge is easy to locate.

set -euo pipefail

PUBLIC_ENGINE_PACKAGE="tandem-ai"

# Crates that must NOT appear in any public engine dependency tree.
FORBIDDEN_CRATES=(
  "tandem-enterprise-server"
  "tandem-governance-engine"
  "fastembed"
  "ort-sys"
)

# Feature sets the public engine artifacts are actually built with. Each entry is
# the value passed to `--features` ("" means default features only). Keep this in
# sync with the public build steps in the release workflows.
PUBLIC_FEATURE_SETS=(
  ""
  "tandem-ai/browser"
)

# Check one feature set; appends to the global `violations` count.
check_feature_set() {
  local features="$1"
  local label="${features:-default}"

  local feature_args=()
  if [ -n "${features}" ]; then
    feature_args=(--features "${features}")
  fi

  echo "Checking public engine (${PUBLIC_ENGINE_PACKAGE}, features: ${label})..."

  # Flat, de-duplicated list of normal (non-dev, non-build) dependencies.
  # `--prefix none` yields one package per line as "<name> v<version> (<source>)".
  local tree_output
  tree_output="$(cargo tree --package "${PUBLIC_ENGINE_PACKAGE}" "${feature_args[@]}" --edges normal --prefix none 2>/dev/null | sort -u)"

  if [ -z "${tree_output}" ]; then
    echo "ERROR: could not resolve the dependency tree for ${PUBLIC_ENGINE_PACKAGE} (features: ${label})." >&2
    violations=$((violations + 1))
    return
  fi

  local crate
  for crate in "${FORBIDDEN_CRATES[@]}"; do
    # Match the package name at the start of a line followed by a space, so e.g.
    # "ort-sys" does not match "ort-sys-something" and substrings never false-match.
    if printf '%s\n' "${tree_output}" | grep -qE "^${crate} "; then
      violations=$((violations + 1))
      echo "ERROR: '${crate}' is reachable from the public build of ${PUBLIC_ENGINE_PACKAGE} (features: ${label})." >&2
      echo "       Offending dependency path:" >&2
      # Invert the tree to show who pulls the crate in. Scoped to the public engine.
      cargo tree --package "${PUBLIC_ENGINE_PACKAGE}" "${feature_args[@]}" --edges normal --invert "${crate}" 2>/dev/null \
        | sed 's/^/         /' >&2 || true
    fi
  done
}

violations=0
for feature_set in "${PUBLIC_FEATURE_SETS[@]}"; do
  check_feature_set "${feature_set}"
done

if [ "${violations}" -ne 0 ]; then
  echo "" >&2
  echo "Public build exclusion guard failed: ${violations} forbidden crate(s) leaked into a public build." >&2
  echo "Keep these behind their opt-in features (enterprise-server / premium-governance / local-embeddings)." >&2
  exit 1
fi

echo "OK: public engine builds exclude ${FORBIDDEN_CRATES[*]}."
