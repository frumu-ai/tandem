#!/bin/bash

# ci-file-size-check.sh: fail on touched .rs/.tsx files over 2,000 lines and warn
# when they are in the warning window [1,800, 2,000).
set -euo pipefail

HARD_MAX_LINES=2000
WARNING_MAX_LINES=1800
CHECK_EXTENSIONS="\\.(rs|tsx)$"
HEAD_SHA="${GITHUB_SHA:-HEAD}"
BASE_REF="${GITHUB_BASE_REF:-}"

echo "Checking touched .rs/.tsx files for line-count policy..."

get_diff_ref() {
  local diff_ref=""

  if [ -n "${CI_FILE_SIZE_BASE_REF:-}" ]; then
    diff_ref="${CI_FILE_SIZE_BASE_REF}"
    if ! git rev-parse --verify "${diff_ref}" >/dev/null 2>&1; then
      echo "Configured CI_FILE_SIZE_BASE_REF ($diff_ref) not found in local refs."
      diff_ref=""
    fi
  fi

  if [ -z "${diff_ref}" ] && [ "${GITHUB_EVENT_NAME:-}" = "pull_request" ] && [ -n "${BASE_REF}" ]; then
    diff_ref="origin/${BASE_REF}"
    if ! git rev-parse --verify "${diff_ref}" >/dev/null 2>&1; then
      git fetch --no-tags --depth=1 origin "$BASE_REF" >/dev/null 2>&1 || true
      if ! git rev-parse --verify "${diff_ref}" >/dev/null 2>&1; then
        diff_ref=""
      fi
    fi
  fi

  if [ -z "${diff_ref}" ] && git rev-parse --verify "${HEAD_SHA}^" >/dev/null 2>&1; then
    diff_ref="${HEAD_SHA}^"
  fi

  echo "${diff_ref}"
}

# Prefer working-tree or staged changes for local use, then fall back to commit diff.
touched_files="$(git diff --name-only -- . | grep -E "$CHECK_EXTENSIONS" || true)"
if [ -z "${touched_files}" ]; then
  touched_files="$(git diff --name-only --cached -- . | grep -E "$CHECK_EXTENSIONS" || true)"
fi

if [ -z "${touched_files}" ]; then
  DIFF_REF="$(get_diff_ref)"
  if [ -n "${DIFF_REF}" ]; then
    touched_files="$(git diff --name-only "${DIFF_REF}" "${HEAD_SHA}" -- . | grep -E "$CHECK_EXTENSIONS" || true)"
  fi
fi

if [ -z "${touched_files}" ]; then
  echo "No relevant files touched."
  exit 0
fi

has_warning=false
has_violation=false
for file in ${touched_files}; do
    if [ -f "${file}" ]; then
        line_count="$(wc -l < "${file}")"
        if [ "${line_count}" -gt "${HARD_MAX_LINES}" ]; then
            echo "ERROR: ${file} has ${line_count} lines, exceeding hard limit of ${HARD_MAX_LINES}."
            has_violation=true
        elif [ "${line_count}" -ge "${WARNING_MAX_LINES}" ]; then
            echo "WARNING: ${file} has ${line_count} lines; target band is below ${WARNING_MAX_LINES} with hard cap ${HARD_MAX_LINES}."
            has_warning=true
        fi
    fi
done

if [ "$has_warning" = true ]; then
    echo "Please consider splitting touched files to maintain the target size envelope."
fi

if [ "$has_violation" = true ]; then
    echo "Hard line-count gate failed for one or more touched files."
    exit 1
fi

if [ "$has_warning" = true ]; then
    exit 0
else
    echo "All touched files are within limits."
    exit 0
fi
