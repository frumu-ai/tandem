#!/usr/bin/env bash
set -euo pipefail

kind="${1:-}"
archive="${2:-}"

if [[ -z "$kind" || -z "$archive" || ! -f "$archive" ]]; then
  echo "usage: $0 <ts-sdk|control-panel|python-sdk|python-sdist> <archive>" >&2
  exit 2
fi

listing="$(mktemp)"
trap 'rm -f "$listing"' EXIT

require_entry() {
  local entry="$1"
  if ! grep -Fxq "$entry" "$listing"; then
    echo "release archive is missing required entry: $entry" >&2
    exit 1
  fi
}

reject_unsafe_entries() {
  if grep -Eq '(^/|(^|/)\.\.(/|$))' "$listing"; then
    echo "release archive contains an unsafe absolute or parent-relative path" >&2
    exit 1
  fi
}

case "$kind" in
  ts-sdk)
    tar -tzf "$archive" | sort -u > "$listing"
    require_entry package/package.json
    require_entry package/dist/index.js
    require_entry package/dist/index.cjs
    require_entry package/dist/index.d.ts
    ;;
  control-panel)
    tar -tzf "$archive" | sort -u > "$listing"
    require_entry package/package.json
    require_entry package/bin/cli.js
    require_entry package/bin/init-env.js
    require_entry package/dist/index.html
    ;;
  python-sdk)
    unzip -Z1 "$archive" | sort -u > "$listing"
    require_entry tandem_client/__init__.py
    if ! grep -Eq '^tandem_client-[^/]+\.dist-info/METADATA$' "$listing"; then
      echo "Python wheel is missing dist-info/METADATA" >&2
      exit 1
    fi
    ;;
  python-sdist)
    tar -tzf "$archive" | sort -u > "$listing"
    mapfile -t pyprojects < <(grep -E '^tandem_client-[^/]+/pyproject\.toml$' "$listing" || true)
    if [[ "${#pyprojects[@]}" -ne 1 ]]; then
      echo "Python sdist is missing pyproject.toml" >&2
      exit 1
    fi
    sdist_root="${pyprojects[0]%/pyproject.toml}"
    require_entry "$sdist_root/tandem_client/__init__.py"
    while IFS= read -r entry; do
      if [[ "$entry" != "$sdist_root/"* ]]; then
        echo "Python sdist contains an entry outside $sdist_root/: $entry" >&2
        exit 1
      fi
    done < "$listing"
    ;;
  *)
    echo "unsupported archive kind: $kind" >&2
    exit 2
    ;;
esac

reject_unsafe_entries

echo "Verified $kind release archive: $(basename "$archive")"
