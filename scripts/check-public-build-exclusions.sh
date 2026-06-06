#!/bin/bash

# check-public-build-exclusions.sh (EAA-11): prove the public Tandem engine build
# does not pull in enterprise-only or heavyweight crates.
#
# The public engine is the `tandem-ai` package (binary `tandem-engine`) built with
# default features. It must never depend on the enterprise server, the premium
# governance engine, or the heavyweight local-embedding stack (fastembed / ort).
# Those are opt-in behind the `enterprise-server` / `premium-governance` /
# `local-embeddings` features and must stay out of the default public build.
#
# On failure the offending dependency path is printed (via `cargo tree -i`) so the
# unexpected edge is easy to locate.

set -euo pipefail

PUBLIC_ENGINE_PACKAGE="tandem-ai"

# Crates that must NOT appear in the default public engine dependency tree.
FORBIDDEN_CRATES=(
  "tandem-enterprise-server"
  "tandem-governance-engine"
  "fastembed"
  "ort-sys"
)

echo "Checking that the public engine (${PUBLIC_ENGINE_PACKAGE}, default features) excludes enterprise/heavyweight crates..."

# Flat, de-duplicated list of normal (non-dev, non-build) dependencies of the
# public engine with default features. `--prefix none` yields one package per
# line as "<name> v<version> (<source>)".
tree_output="$(cargo tree --package "${PUBLIC_ENGINE_PACKAGE}" --edges normal --prefix none 2>/dev/null | sort -u)"

if [ -z "${tree_output}" ]; then
  echo "ERROR: could not resolve the dependency tree for ${PUBLIC_ENGINE_PACKAGE}." >&2
  exit 1
fi

violations=0
for crate in "${FORBIDDEN_CRATES[@]}"; do
  # Match the package name at the start of a line followed by a space, so e.g.
  # "ort-sys" does not match "ort-sys-something" and substrings never false-match.
  if printf '%s\n' "${tree_output}" | grep -qE "^${crate} "; then
    violations=$((violations + 1))
    echo "ERROR: '${crate}' is reachable from the default public build of ${PUBLIC_ENGINE_PACKAGE}." >&2
    echo "       Offending dependency path:" >&2
    # Invert the tree to show who pulls the crate in. Scoped to the public engine.
    cargo tree --package "${PUBLIC_ENGINE_PACKAGE}" --edges normal --invert "${crate}" 2>/dev/null \
      | sed 's/^/         /' >&2 || true
  fi
done

if [ "${violations}" -ne 0 ]; then
  echo "" >&2
  echo "Public build exclusion guard failed: ${violations} forbidden crate(s) leaked into the default public build." >&2
  echo "Keep these behind their opt-in features (enterprise-server / premium-governance / local-embeddings)." >&2
  exit 1
fi

echo "OK: public engine default build excludes ${FORBIDDEN_CRATES[*]}."
