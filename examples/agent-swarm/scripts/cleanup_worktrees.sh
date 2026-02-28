#!/bin/bash
# cleanup_worktrees.sh
# Removes a specific git worktree and its branch
# Usage: ./cleanup_worktrees.sh <task_id>

set -e

if [ -z "$1" ]; then
    echo "Error: task_id is required."
    echo "Usage: $0 <task_id>"
    exit 1
fi

TASK_ID=$1
WORKTREE_DIR=".swarm/worktrees/$TASK_ID"
BRANCH_NAME="swarm/$TASK_ID"

# Validate task ID format to prevent path traversal
if ! [[ "$TASK_ID" =~ ^[a-zA-Z0-9_-]+$ ]]; then
    echo "Error: Invalid task_id format. Only alphanumeric, dashes, and underscores are allowed."
    exit 1
fi

REPO_ROOT=$(git rev-parse --show-toplevel)
cd "$REPO_ROOT/examples/agent-swarm"

if [ ! -d "$WORKTREE_DIR" ]; then
    echo "Error: Worktree directory $WORKTREE_DIR does not exist."
    exit 1
fi

echo "Removing worktree $WORKTREE_DIR"
git worktree remove --force "$WORKTREE_DIR"

echo "Deleting branch $BRANCH_NAME"
git branch -D "$BRANCH_NAME" || true

echo "Cleanup complete."
