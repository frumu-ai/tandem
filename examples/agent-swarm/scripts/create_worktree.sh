#!/bin/bash
# create_worktree.sh
# Safely creates a new git worktree inside the .tandem/worktrees/ directory
# Usage: ./create_worktree.sh <task_id>

set -e

if [ -z "$1" ]; then
    echo "Error: task_id is required."
    echo "Usage: $0 <task_id>"
    exit 1
fi

TASK_ID=$1
WORKTREE_DIR=".swarm/worktrees/$TASK_ID"

# Validate task ID format to prevent path traversal
if ! [[ "$TASK_ID" =~ ^[a-zA-Z0-9_-]+$ ]]; then
    echo "Error: Invalid task_id format. Only alphanumeric, dashes, and underscores are allowed."
    exit 1
fi

if [ -d "$WORKTREE_DIR" ]; then
    echo "Error: Worktree directory $WORKTREE_DIR already exists."
    exit 1
fi

# Ensure parent directory exists
mkdir -p .swarm/worktrees

# Extract the repository root relative to this script
REPO_ROOT=$(git rev-parse --show-toplevel)

# Create a new branch originating from the current HEAD
BRANCH_NAME="swarm/$TASK_ID"

echo "Creating new git worktree for $BRANCH_NAME at $WORKTREE_DIR"

# Create the worktree
cd "$REPO_ROOT/examples/agent-swarm"
git worktree add -b "$BRANCH_NAME" "$WORKTREE_DIR"

# Return the absolute path so the Manager Agent knows where to send the Worker
ABSOLUTE_PATH=$(cd "$WORKTREE_DIR" && pwd)

echo "Success!"
echo "WORKTREE_PATH=$ABSOLUTE_PATH"
echo "BRANCH=$BRANCH_NAME"
