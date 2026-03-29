#!/bin/bash

# check-file-sizes.sh: Scan the codebase for .rs and .tsx files, 
# outputting line counts in CSV format.

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUTPUT_FILE="${ROOT_DIR}/docs/internal/file-size-baseline.csv"

echo "path,line_count" > "$OUTPUT_FILE"

find "$ROOT_DIR" -type f \( -name "*.rs" -o -name "*.tsx" \) \
    -not -path "*/target/*" \
    -not -path "*/node_modules/*" \
    -not -path "*/dist/*" \
    -not -path "*/.git/*" | while read -r file; do
    line_count=$(wc -l < "$file")
    rel_path="${file#$ROOT_DIR/}"
    echo "$rel_path,$line_count" >> "$OUTPUT_FILE"
done

echo "Wrote file size baseline to $OUTPUT_FILE"
