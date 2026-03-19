#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SETUP_JS="$SCRIPT_DIR/setup.js"
NODE_BIN="${NODE_BIN:-$(command -v node || true)}"

if [[ -z "${NODE_BIN}" ]]; then
  echo "node not found in PATH" >&2
  exit 1
fi

cmd="${1:-help}"
arg1="${2:-}"
arg2="${3:-}"

run_setup() {
  sudo "$NODE_BIN" "$SETUP_JS" "$@"
}

case "$cmd" in
  install-panel)
    panel_port="${arg1:-3402}"
    sudo TANDEM_CONTROL_PANEL_PORT="$panel_port" "$NODE_BIN" "$SETUP_JS" --install-services --service-mode=panel
    ;;
  install-both)
    engine_port="${arg1:-39731}"
    panel_port="${arg2:-3402}"
    sudo TANDEM_ENGINE_PORT="$engine_port" TANDEM_CONTROL_PANEL_PORT="$panel_port" \
      "$NODE_BIN" "$SETUP_JS" --install-services --service-mode=both
    ;;
  restart-panel)
    run_setup --service-op=restart --service-mode=panel
    ;;
  restart-both)
    run_setup --service-op=restart --service-mode=both
    ;;
  status-panel)
    run_setup --service-op=status --service-mode=panel
    ;;
  status-both)
    run_setup --service-op=status --service-mode=both
    ;;
  logs-panel)
    run_setup --service-op=logs --service-mode=panel
    ;;
  logs-both)
    run_setup --service-op=logs --service-mode=both
    ;;
  *)
    cat <<'EOF'
Usage:
  bash packages/tandem-control-panel/bin/service-local.sh install-panel [panel_port]
  bash packages/tandem-control-panel/bin/service-local.sh install-both [engine_port] [panel_port]
  bash packages/tandem-control-panel/bin/service-local.sh restart-panel
  bash packages/tandem-control-panel/bin/service-local.sh restart-both
  bash packages/tandem-control-panel/bin/service-local.sh status-panel
  bash packages/tandem-control-panel/bin/service-local.sh status-both
  bash packages/tandem-control-panel/bin/service-local.sh logs-panel
  bash packages/tandem-control-panel/bin/service-local.sh logs-both
EOF
    ;;
esac
