#!/bin/bash
# script/publish-crates.sh
# Usage: ./scripts/publish-crates.sh [--dry-run]

DRY_RUN=""
if [ "$1" == "--dry-run" ]; then
    DRY_RUN="--dry-run"
    echo "🔍 Running in DRY-RUN mode"
fi

# Topological order of publishing (independent first)
CRATES=(
    "crates/tandem-enterprise-contract"
    "crates/tandem-types"
    "crates/tandem-wire"
    "crates/tandem-observability"
    "crates/tandem-document"
    "crates/tandem-providers"
    "crates/tandem-skills"
    "crates/tandem-agent-teams"
    "crates/tandem-orchestrator"
    "crates/tandem-memory"
    "crates/tandem-tools"
    "crates/tandem-core"
    "crates/tandem-runtime"
    "crates/tandem-channels"
    "crates/tandem-server"
    "crates/tandem-tui" # Binary, can publish if lib? It's binary.
    "engine" # tandem-engine binary
)

echo "📦 Publishing crates in order..."

for crate in "${CRATES[@]}"; do
    if [ ! -d "$crate" ]; then
        echo "⚠️  Skipping missing directory: $crate"
        continue
    fi

    echo "---------------------------------------------------"
    echo "🚀 Processing $crate"
    
    # Check for path dependencies
    if grep -q 'path =' "$crate/Cargo.toml"; then
        echo "❌ Error: $crate contains local 'path' dependencies."
        echo "   Crates.io does not allow 'path' dependencies."
        echo "   Please replace 'path = \"...\"' with version dependencies."
        grep 'path =' "$crate/Cargo.toml"
        
        read -p "   Continue anyway (local install)? [y/N] " -n 1 -r
        echo
        if [[ ! $REPLY =~ ^[Yy]$ ]]; then
            exit 1
        fi
    fi

    echo "   Publishing..."
    # We use --allow-dirty for beta testing if source control isn't perfect, 
    # but strictly we should commit first.
    # For CI, we might need token.
    (cd "$crate" && cargo publish $DRY_RUN)
    
    if [ $? -ne 0 ]; then
        echo "❌ Failed to publish $crate"
        exit 1
    fi
    
    # Wait a bit for crates.io to propagate
    echo "   Waiting 10s for propagation..."
    sleep 10
done

echo "✅ All crates published!"
