#!/bin/bash

# ci-file-size-check.sh: Warn when touched .rs/.tsx files exceed 1500 lines.

MAX_LINES=1500
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

echo "Checking touched files for line count > $MAX_LINES..."

# Get list of changed .rs and .tsx files (compared to HEAD or main)
touched_files=$(git diff --name-only HEAD | grep -E "\.(rs|tsx)$")

if [ -z "$touched_files" ]; then
    echo "No relevant files touched."
    exit 0
fi

has_warning=false
for file in $touched_files; do
    if [ -f "$file" ]; then
        line_count=$(wc -l < "$file")
        if [ "$line_count" -gt "$MAX_LINES" ]; then
            echo "WARNING: $file has $line_count lines, exceeding the $MAX_LINES threshold."
            has_warning=true
        fi
    fi
done

if [ "$has_warning" = true ]; then
    echo "Please consider splitting large files to maintain codebase health."
    # We exit with 0 to make it a warning, not a hard failure.
    exit 0
else
    echo "All touched files are within limits."
    exit 0
fi
