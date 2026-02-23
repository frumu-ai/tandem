#!/usr/bin/env bash
set -euo pipefail

SERVICE_USER="${SUDO_USER:-$USER}"
PROJECT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
if [[ "$(id -u)" -eq 0 ]]; then
  SUDO_CMD=()
  if [[ -z "${SERVICE_USER:-}" || "$SERVICE_USER" == "root" ]]; then
    SERVICE_USER="${SUDO_USER:-$(logname 2>/dev/null || echo root)}"
  fi
else
  SUDO_CMD=(sudo)
fi

if [[ ! -f "$PROJECT_DIR/package.json" || ! -f "$PROJECT_DIR/server.js" ]]; then
  echo "setup-vps.sh must live in the portal project directory"
  exit 1
fi

# Resolve target user's home deterministically
SERVICE_HOME="$(getent passwd "$SERVICE_USER" | cut -d: -f6 || true)"
if [[ -z "$SERVICE_HOME" || ! -d "$SERVICE_HOME" ]]; then
  SERVICE_HOME="/home/$SERVICE_USER"
fi

log() {
  echo "[setup-vps] $*"
}

fail() {
  echo "[setup-vps] ERROR: $*" >&2
  exit 1
}

run_as_service_user() {
  if [[ "$(id -u)" -eq 0 ]]; then
    sudo -u "$SERVICE_USER" env "HOME=$SERVICE_HOME" "PATH=$SERVICE_PATH" "$@"
  else
    env "HOME=$SERVICE_HOME" "PATH=$SERVICE_PATH" "$@"
  fi
}

resolve_cmd_path_for_user() {
  local cmd_name="$1"
  run_as_service_user bash -c "command -v \"$cmd_name\" 2>/dev/null || true"
}

SERVICE_PATH="/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin:/snap/bin:$SERVICE_HOME/.local/share/pnpm"
if compgen -G "$SERVICE_HOME/.nvm/versions/node/*/bin" >/dev/null; then
  NVM_BIN_PATHS=""
  while IFS= read -r bin_dir; do
    if [[ -z "$NVM_BIN_PATHS" ]]; then
      NVM_BIN_PATHS="$bin_dir"
    else
      NVM_BIN_PATHS="$NVM_BIN_PATHS:$bin_dir"
    fi
  done < <(ls -d "$SERVICE_HOME"/.nvm/versions/node/*/bin 2>/dev/null | sort -r)
  if [[ -n "$NVM_BIN_PATHS" ]]; then
    SERVICE_PATH="$NVM_BIN_PATHS:$SERVICE_PATH"
  fi
fi

resolve_node() {
  local candidate
  if compgen -G "$SERVICE_HOME/.nvm/versions/node/*/bin/node" >/dev/null; then
    while IFS= read -r candidate; do
      if [[ -x "$candidate" ]]; then
        echo "$candidate"
        return 0
      fi
    done < <(ls -d "$SERVICE_HOME"/.nvm/versions/node/*/bin/node 2>/dev/null | sort -Vr)
  fi

  for candidate in \
    "$SERVICE_HOME/.nvm/versions/node/current/bin/node" \
    "$SERVICE_HOME/.local/share/pnpm/node" \
    "/usr/local/bin/node" \
    "/usr/bin/node"; do
      if [[ -x "$candidate" ]]; then
        echo "$candidate"
        return 0
      fi
    done

  candidate="$(resolve_cmd_path_for_user node)"
  if [[ -n "$candidate" && -x "$candidate" ]]; then
    echo "$candidate"
    return 0
  fi
  return 1
}

resolve_npm() {
  local candidate
  if compgen -G "$SERVICE_HOME/.nvm/versions/node/*/bin/npm" >/dev/null; then
    while IFS= read -r candidate; do
      if [[ -x "$candidate" ]]; then
        echo "$candidate"
        return 0
      fi
    done < <(ls -d "$SERVICE_HOME"/.nvm/versions/node/*/bin/npm 2>/dev/null | sort -Vr)
  fi

  for candidate in \
    "/usr/local/bin/npm" \
    "/usr/bin/npm"; do
    if [[ -x "$candidate" ]]; then
      echo "$candidate"
      return 0
    fi
  done

  candidate="$(resolve_cmd_path_for_user npm)"
  if [[ -n "$candidate" && -x "$candidate" ]]; then
    echo "$candidate"
    return 0
  fi
  return 1
}

resolve_npm_global_bin() {
  local npm_path="$1"
  local npm_dir
  npm_dir="$(dirname "$npm_path")"
  if [[ -x "$npm_dir/node" ]]; then
    echo "$npm_dir"
    return 0
  fi

  local prefix
  prefix="$(run_as_service_user "$npm_path" prefix -g 2>/dev/null || true)"
  if [[ -n "$prefix" && "$prefix" != "undefined" ]]; then
    echo "$prefix/bin"
    return 0
  fi

  prefix="$(run_as_service_user "$npm_path" config get prefix 2>/dev/null || true)"
  if [[ -n "$prefix" && "$prefix" != "undefined" ]]; then
    echo "$prefix/bin"
    return 0
  fi
  return 1
}

resolve_npm_install_prefix() {
  local npm_path="$1"
  local npm_dir
  npm_dir="$(dirname "$npm_path")"
  if [[ -x "$npm_dir/node" ]]; then
    # npm lives in .../bin/npm, so prefix root is parent of bin
    dirname "$npm_dir"
    return 0
  fi

  local prefix
  prefix="$(run_as_service_user "$npm_path" prefix -g 2>/dev/null || true)"
  if [[ -n "$prefix" && "$prefix" != "undefined" ]]; then
    echo "$prefix"
    return 0
  fi
  return 1
}

resolve_pnpm() {
  local candidate
  for candidate in \
    "${PNPM_HOME:-}/pnpm" \
    "$SERVICE_HOME/.local/share/pnpm/pnpm" \
    "/usr/local/bin/pnpm" \
    "/usr/bin/pnpm"; do
    if [[ -n "$candidate" && -x "$candidate" ]]; then
      echo "$candidate"
      return 0
    fi
  done

  if run_as_service_user corepack pnpm --version >/dev/null 2>&1; then
    echo "corepack:pnpm"
    return 0
  fi

  candidate="$(resolve_cmd_path_for_user pnpm)"
  if [[ -n "$candidate" && -x "$candidate" ]]; then
    echo "$candidate"
    return 0
  fi
  return 1
}

resolve_tandem_engine() {
  local candidate
  if [[ -n "${NPM_INSTALL_PREFIX:-}" ]]; then
    candidate="$NPM_INSTALL_PREFIX/bin/tandem-engine"
    if [[ -x "$candidate" ]]; then
      echo "$candidate"
      return 0
    fi
  fi
  if [[ -n "${NPM_GLOBAL_BIN_PATH:-}" ]]; then
    candidate="$NPM_GLOBAL_BIN_PATH/tandem-engine"
    if [[ -x "$candidate" ]]; then
      echo "$candidate"
      return 0
    fi
  fi
  for candidate in \
    "$SERVICE_HOME/.local/share/pnpm/tandem-engine" \
    "$SERVICE_HOME/.npm-global/bin/tandem-engine" \
    "/usr/local/bin/tandem-engine" \
    "/usr/bin/tandem-engine"; do
    if [[ -x "$candidate" ]]; then
      echo "$candidate"
      return 0
    fi
  done

  candidate="$(resolve_cmd_path_for_user tandem-engine)"
  if [[ -n "$candidate" && -x "$candidate" ]]; then
    echo "$candidate"
    return 0
  fi
  return 1
}

resolve_npx() {
  local candidate
  candidate="$(resolve_cmd_path_for_user npx)"
  if [[ -n "$candidate" && -x "$candidate" ]]; then
    echo "$candidate"
    return 0
  fi
  return 1
}

engine_cmd() {
  local sub="$1"
  shift || true
  local args=("$sub" "$@")

  if [[ -n "${ENGINE_PATH:-}" && -x "${ENGINE_PATH:-}" ]]; then
    "$ENGINE_PATH" "${args[@]}"
  else
    run_as_service_user npx -y @frumu/tandem "${args[@]}"
  fi
}

install_tandem_engine() {
  local npm_path="$1"
  local pnpm_resolved="$2"

  if [[ -z "$npm_path" || ! -x "$npm_path" ]]; then
    if [[ "${SETUP_ALLOW_PNPM_FALLBACK:-0}" == "1" && -n "$pnpm_resolved" ]]; then
      log "npm unavailable; SETUP_ALLOW_PNPM_FALLBACK=1 so installing @frumu/tandem with pnpm"
      if [[ "$pnpm_resolved" == "corepack:pnpm" ]]; then
        run_as_service_user corepack pnpm add -g @frumu/tandem
      else
        run_as_service_user "$pnpm_resolved" add -g @frumu/tandem
      fi
      return 0
    fi
    fail "npm not found for user '$SERVICE_USER'. Install npm for that user and rerun setup-vps.sh. \
If you intentionally need pnpm fallback, set SETUP_ALLOW_PNPM_FALLBACK=1."
  fi

  log "Installing @frumu/tandem with npm for user '$SERVICE_USER'"
  local npm_install_prefix="$NPM_INSTALL_PREFIX"
  if [[ -z "$npm_install_prefix" ]]; then
    npm_install_prefix="$(resolve_npm_install_prefix "$npm_path" || true)"
  fi
  if [[ -z "$npm_install_prefix" ]]; then
    fail "Could not determine npm install prefix for $npm_path"
  fi

  # Clean stale/broken installs first so PATH doesn't resolve old shims.
  run_as_service_user "$npm_path" --prefix "$npm_install_prefix" uninstall -g @frumu/tandem >/dev/null 2>&1 || true
  if [[ -n "$pnpm_resolved" ]]; then
    if [[ "$pnpm_resolved" == "corepack:pnpm" ]]; then
      run_as_service_user corepack pnpm remove -g @frumu/tandem >/dev/null 2>&1 || true
    else
      run_as_service_user "$pnpm_resolved" remove -g @frumu/tandem >/dev/null 2>&1 || true
    fi
  fi
  run_as_service_user rm -f "$SERVICE_HOME/.local/share/pnpm/tandem-engine" >/dev/null 2>&1 || true
  run_as_service_user "$npm_path" --prefix "$npm_install_prefix" install -g @frumu/tandem
}

validate_tandem_engine() {
  if [[ -n "${ENGINE_PATH:-}" && -x "${ENGINE_PATH:-}" ]]; then
    if "$ENGINE_PATH" --version >/dev/null 2>&1; then
      return 0
    fi
    log "Detected unusable tandem-engine binary at '$ENGINE_PATH'; falling back to npx runtime"
    ENGINE_PATH=""
    return 1
  fi
  return 1
}

log "Using service user: $SERVICE_USER"
log "Service home: $SERVICE_HOME"

NODE_PATH="$(resolve_node || true)"
if [[ -z "$NODE_PATH" ]]; then
  fail "node not found for user '$SERVICE_USER'. Checked nvm/system paths and PATH=$SERVICE_PATH"
fi

NPM_PATH="$(resolve_npm || true)"
NPM_GLOBAL_BIN_PATH=""
NPM_INSTALL_PREFIX=""
if [[ -n "$NPM_PATH" ]]; then
  NPM_INSTALL_PREFIX="$(resolve_npm_install_prefix "$NPM_PATH" || true)"
  NPM_GLOBAL_BIN_PATH="$(resolve_npm_global_bin "$NPM_PATH" || true)"
  log "Resolved npm: $NPM_PATH"
  if [[ -n "$NPM_INSTALL_PREFIX" ]]; then
    log "Resolved npm install prefix: $NPM_INSTALL_PREFIX"
  fi
  if [[ -n "$NPM_GLOBAL_BIN_PATH" ]]; then
    log "Resolved npm global bin: $NPM_GLOBAL_BIN_PATH"
  fi
else
  log "npm not found for service user"
fi

PNPM_PATH="$(resolve_pnpm || true)"
if [[ -n "$PNPM_PATH" ]]; then
  log "Resolved pnpm: $PNPM_PATH"
else
  log "pnpm not found"
fi

ENGINE_PATH="$(resolve_tandem_engine || true)"
if [[ -z "$ENGINE_PATH" ]]; then
  install_tandem_engine "$NPM_PATH" "$PNPM_PATH"
  ENGINE_PATH="$(resolve_tandem_engine || true)"
  if [[ -n "$ENGINE_PATH" ]]; then
    validate_tandem_engine || true
  fi
fi
if [[ -n "$ENGINE_PATH" ]]; then
  if ! validate_tandem_engine; then
    log "Reinstalling @frumu/tandem with npm because resolved binary is unusable"
    install_tandem_engine "$NPM_PATH" "$PNPM_PATH"
    ENGINE_PATH="$(resolve_tandem_engine || true)"
    if [[ -z "$ENGINE_PATH" ]]; then
      fail "tandem-engine is still unusable after npm reinstall"
    fi
    if ! validate_tandem_engine; then
      fail "tandem-engine is still unusable after npm reinstall"
    fi
  fi
fi

if [[ -z "$ENGINE_PATH" ]]; then
  log "No standalone tandem-engine binary found; using npx @frumu/tandem fallback"
  NPX_PATH="$(resolve_npx || true)"
  if [[ -z "$NPX_PATH" ]]; then
    fail "Cannot run fallback 'npx @frumu/tandem' because npx is unavailable for user '$SERVICE_USER'. \
Install Node/npm for that user or ensure tandem-engine binary is installed."
  fi
fi
log "Resolved node: $NODE_PATH"
if [[ -n "$ENGINE_PATH" ]]; then
  log "Resolved tandem-engine: $ENGINE_PATH"
else
  log "Resolved tandem-engine: npx -y @frumu/tandem"
fi

TOKEN="${TANDEM_API_TOKEN:-}"
if [[ -z "$TOKEN" && -f "$PROJECT_DIR/.env" ]]; then
  EXISTING_PORTAL_KEY="$(sed -n 's/^VITE_PORTAL_KEY=//p' "$PROJECT_DIR/.env" | tail -n1 || true)"
  if [[ -n "$EXISTING_PORTAL_KEY" ]]; then
    TOKEN="$EXISTING_PORTAL_KEY"
    log "Reusing existing .env VITE_PORTAL_KEY for TANDEM_API_TOKEN"
  fi
fi
if [[ -z "$TOKEN" ]]; then
  TOKEN="$(engine_cmd token generate)"
fi

STATE_DIR="${TANDEM_STATE_DIR:-/srv/tandem}"
ENGINE_ENV_PATH="/etc/tandem/engine.env"
ENGINE_CONFIG_PATH="$STATE_DIR/config.json"

"${SUDO_CMD[@]}" mkdir -p /etc/tandem "$STATE_DIR"
"${SUDO_CMD[@]}" chown -R "$SERVICE_USER":"$SERVICE_USER" "$STATE_DIR"

EXISTING_ENGINE_ENV="$("${SUDO_CMD[@]}" sh -c "test -f '$ENGINE_ENV_PATH' && cat '$ENGINE_ENV_PATH' || true")"
PROVIDER_KEY_REGEX='^(OPENROUTER_API_KEY|OPENAI_API_KEY|ANTHROPIC_API_KEY|GROQ_API_KEY|MISTRAL_API_KEY|COHERE_API_KEY|TOGETHER_API_KEY|GITHUB_TOKEN)='
PRESERVED_ENGINE_ENV="$(printf '%s\n' "$EXISTING_ENGINE_ENV" | grep -Ev '^(TANDEM_API_TOKEN|TANDEM_STATE_DIR)=' | grep -Ev "$PROVIDER_KEY_REGEX" || true)"
EXISTING_PROVIDER_ENV="$(printf '%s\n' "$EXISTING_ENGINE_ENV" | grep -E "$PROVIDER_KEY_REGEX" || true)"
PROJECT_PROVIDER_ENV=""
if [[ -f "$PROJECT_DIR/.env" ]]; then
  PROJECT_PROVIDER_ENV="$(grep -E "$PROVIDER_KEY_REGEX" "$PROJECT_DIR/.env" || true)"
fi

# Keep existing provider keys by default, but let project .env override/add values
# so demo users can set keys in one place before running setup-vps.sh.
MERGED_PROVIDER_ENV="$(
  {
    printf '%s\n' "$EXISTING_PROVIDER_ENV"
    printf '%s\n' "$PROJECT_PROVIDER_ENV"
  } | awk -F= '/^[A-Z0-9_]+=/ {
      key=$1
      if (!(key in seen)) {
        order[++count]=key
        seen[key]=1
      }
      value[key]=$0
    }
    END {
      for (i=1; i<=count; i++) {
        print value[order[i]]
      }
    }'
)"

"${SUDO_CMD[@]}" tee "$ENGINE_ENV_PATH" >/dev/null <<EOF
TANDEM_API_TOKEN=$TOKEN
TANDEM_STATE_DIR=$STATE_DIR
EOF
if [[ -n "$PRESERVED_ENGINE_ENV" ]]; then
  printf '%s\n' "$PRESERVED_ENGINE_ENV" | "${SUDO_CMD[@]}" tee -a "$ENGINE_ENV_PATH" >/dev/null
fi
if [[ -n "$MERGED_PROVIDER_ENV" ]]; then
  printf '%s\n' "$MERGED_PROVIDER_ENV" | "${SUDO_CMD[@]}" tee -a "$ENGINE_ENV_PATH" >/dev/null
  if [[ -n "$PROJECT_PROVIDER_ENV" ]]; then
    log "Synced provider API key vars from $PROJECT_DIR/.env into $ENGINE_ENV_PATH"
  fi
fi

# Add commented key hints once, without clobbering existing values.
if ! "${SUDO_CMD[@]}" grep -q '^# OPENROUTER_API_KEY=' "$ENGINE_ENV_PATH"; then
  "${SUDO_CMD[@]}" tee -a "$ENGINE_ENV_PATH" >/dev/null <<'EOF'

# Optional provider API keys (persist across restarts)
# Uncomment and fill at least one before first use:
# OPENROUTER_API_KEY=or-...
# OPENAI_API_KEY=sk-...
# ANTHROPIC_API_KEY=sk-ant-...
# GROQ_API_KEY=gsk_...
# MISTRAL_API_KEY=...
# COHERE_API_KEY=...
# TOGETHER_API_KEY=...
# GITHUB_TOKEN=...
EOF
fi

# Bootstrap persistent provider/model config if missing.
if ! "${SUDO_CMD[@]}" test -f "$ENGINE_CONFIG_PATH"; then
  "${SUDO_CMD[@]}" mkdir -p "$(dirname "$ENGINE_CONFIG_PATH")"
  "${SUDO_CMD[@]}" tee "$ENGINE_CONFIG_PATH" >/dev/null <<'EOF'
{
  "default_provider": "openrouter",
  "providers": {
    "openrouter": { "default_model": "google/gemini-3.1-pro-preview" },
    "openai": { "default_model": "gpt-4o-mini" },
    "anthropic": { "default_model": "claude-4-6-sonnet-latest" }
  }
}
EOF
  "${SUDO_CMD[@]}" chown "$SERVICE_USER":"$SERVICE_USER" "$ENGINE_CONFIG_PATH"
  log "Bootstrapped engine config at $ENGINE_CONFIG_PATH"
fi

HAS_PROVIDER_KEYS=0
if "${SUDO_CMD[@]}" grep -Eq "$PROVIDER_KEY_REGEX" "$ENGINE_ENV_PATH"; then
  HAS_PROVIDER_KEYS=1
fi

"${SUDO_CMD[@]}" tee /etc/systemd/system/tandem-engine.service >/dev/null <<EOF
[Unit]
Description=Tandem Engine
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
User=$SERVICE_USER
Group=$SERVICE_USER
EnvironmentFile=/etc/tandem/engine.env
ExecStart=$ENGINE_PATH serve --hostname 127.0.0.1 --port 39731
Restart=on-failure
RestartSec=5
NoNewPrivileges=true
PrivateTmp=true
ProtectSystem=strict
ReadWritePaths=$STATE_DIR

[Install]
WantedBy=multi-user.target
EOF

if [[ -z "$ENGINE_PATH" ]]; then
  "${SUDO_CMD[@]}" tee /etc/systemd/system/tandem-engine.service >/dev/null <<EOF
[Unit]
Description=Tandem Engine
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
User=$SERVICE_USER
Group=$SERVICE_USER
EnvironmentFile=/etc/tandem/engine.env
WorkingDirectory=$PROJECT_DIR
Environment=PATH=$SERVICE_PATH
ExecStart=/usr/bin/env npx -y @frumu/tandem serve --hostname 127.0.0.1 --port 39731
Restart=on-failure
RestartSec=5
NoNewPrivileges=true
PrivateTmp=true
ProtectSystem=strict
ReadWritePaths=$STATE_DIR

[Install]
WantedBy=multi-user.target
EOF
fi

# Install a tightly scoped engine control helper for portal process actions.
ENGINE_CTL_SCRIPT="/usr/local/bin/tandem-engine-ctl"
"${SUDO_CMD[@]}" tee "$ENGINE_CTL_SCRIPT" >/dev/null <<'EOF'
#!/usr/bin/env bash
set -euo pipefail

ACTION="${1:-}"
SERVICE="${2:-tandem-engine.service}"

case "$ACTION" in
  status|start|stop|restart) ;;
  *)
    echo '{"ok":false,"error":"invalid action"}'
    exit 2
    ;;
esac

if [[ "$SERVICE" != "tandem-engine.service" ]]; then
  echo '{"ok":false,"error":"invalid service"}'
  exit 2
fi

if [[ "$ACTION" == "status" ]]; then
  ACTIVE_STATE="$(systemctl show "$SERVICE" -p ActiveState --value || true)"
  SUB_STATE="$(systemctl show "$SERVICE" -p SubState --value || true)"
  LOADED_STATE="$(systemctl show "$SERVICE" -p LoadState --value || true)"
  UNIT_FILE_STATE="$(systemctl show "$SERVICE" -p UnitFileState --value || true)"
  TS="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
  printf '{"ok":true,"serviceName":"%s","activeState":"%s","subState":"%s","loadedState":"%s","unitFileState":"%s","timestamp":"%s"}\n' \
    "$SERVICE" "$ACTIVE_STATE" "$SUB_STATE" "$LOADED_STATE" "$UNIT_FILE_STATE" "$TS"
  exit 0
fi

systemctl "$ACTION" "$SERVICE"
exec "$0" status "$SERVICE"
EOF
"${SUDO_CMD[@]}" chmod 755 "$ENGINE_CTL_SCRIPT"
"${SUDO_CMD[@]}" chown root:root "$ENGINE_CTL_SCRIPT"

# Allow the portal service user to invoke only the helper script without password.
SUDOERS_PATH="/etc/sudoers.d/tandem-portal-engine-control"
"${SUDO_CMD[@]}" tee "$SUDOERS_PATH" >/dev/null <<EOF
$SERVICE_USER ALL=(root) NOPASSWD: $ENGINE_CTL_SCRIPT
EOF
"${SUDO_CMD[@]}" chmod 440 "$SUDOERS_PATH"

"${SUDO_CMD[@]}" systemctl daemon-reload
"${SUDO_CMD[@]}" systemctl enable --now tandem-engine
"${SUDO_CMD[@]}" systemctl restart tandem-engine

# Build and setup portal service
cd "$PROJECT_DIR"
if [[ -n "$PNPM_PATH" ]]; then
  log "Installing/building portal with pnpm"
  if [[ "$PNPM_PATH" == "corepack:pnpm" ]]; then
    run_as_service_user corepack pnpm install --frozen-lockfile
    run_as_service_user corepack pnpm run build
  else
    run_as_service_user "$PNPM_PATH" install --frozen-lockfile
    run_as_service_user "$PNPM_PATH" run build
  fi
else
  NPM_PATH="$(resolve_cmd_path_for_user npm)"
  if [[ -z "$NPM_PATH" || ! -x "$NPM_PATH" ]]; then
    fail "Cannot build portal: neither pnpm nor npm available for user '$SERVICE_USER'"
  fi
  log "Building portal with npm fallback"
  run_as_service_user "$NPM_PATH" install
  run_as_service_user "$NPM_PATH" run build
fi

if [[ ! -f "$PROJECT_DIR/.env" ]]; then
  cat > "$PROJECT_DIR/.env" <<EOF
# The port the Node/Express proxy will listen on (Publicly accessible)
PORT=80

# The Token generated by 'tandem-engine token generate' in Step 1
# This is also the login key for the portal.
VITE_PORTAL_KEY=$TOKEN

# (Optional) Engine local address
VITE_TANDEM_ENGINE_URL=http://127.0.0.1:39731
EOF
fi

# Always sync portal key to engine token to avoid proxy 401 mismatches.
if grep -q '^VITE_PORTAL_KEY=' "$PROJECT_DIR/.env"; then
  sed -i "s/^VITE_PORTAL_KEY=.*/VITE_PORTAL_KEY=$TOKEN/" "$PROJECT_DIR/.env"
else
  echo "VITE_PORTAL_KEY=$TOKEN" >> "$PROJECT_DIR/.env"
fi

if ! grep -q '^PORT=' "$PROJECT_DIR/.env"; then
  echo "PORT=80" >> "$PROJECT_DIR/.env"
fi

if ! grep -q '^VITE_TANDEM_ENGINE_URL=' "$PROJECT_DIR/.env"; then
  echo "VITE_TANDEM_ENGINE_URL=http://127.0.0.1:39731" >> "$PROJECT_DIR/.env"
fi
if ! grep -q '^TANDEM_SYSTEM_CONTROL_MODE=' "$PROJECT_DIR/.env"; then
  echo "TANDEM_SYSTEM_CONTROL_MODE=systemd" >> "$PROJECT_DIR/.env"
fi
if ! grep -q '^TANDEM_ENGINE_SERVICE_NAME=' "$PROJECT_DIR/.env"; then
  echo "TANDEM_ENGINE_SERVICE_NAME=tandem-engine.service" >> "$PROJECT_DIR/.env"
fi
if ! grep -q '^TANDEM_ENGINE_CONTROL_SCRIPT=' "$PROJECT_DIR/.env"; then
  echo "TANDEM_ENGINE_CONTROL_SCRIPT=/usr/local/bin/tandem-engine-ctl" >> "$PROJECT_DIR/.env"
fi
if ! grep -q '^TANDEM_ARTIFACT_READ_ROOTS=' "$PROJECT_DIR/.env"; then
  echo "TANDEM_ARTIFACT_READ_ROOTS=$STATE_DIR" >> "$PROJECT_DIR/.env"
fi
if ! grep -q '^TANDEM_PORTAL_MAX_ARTIFACT_BYTES=' "$PROJECT_DIR/.env"; then
  echo "TANDEM_PORTAL_MAX_ARTIFACT_BYTES=1048576" >> "$PROJECT_DIR/.env"
fi

"${SUDO_CMD[@]}" tee /etc/systemd/system/tandem-portal.service >/dev/null <<EOF
[Unit]
Description=Tandem Portal
After=network-online.target tandem-engine.service
Wants=network-online.target

[Service]
Type=simple
User=$SERVICE_USER
Group=$SERVICE_USER
WorkingDirectory=$PROJECT_DIR
EnvironmentFile=$PROJECT_DIR/.env
ExecStart=$NODE_PATH server.js
AmbientCapabilities=CAP_NET_BIND_SERVICE CAP_SETUID CAP_SETGID
CapabilityBoundingSet=CAP_NET_BIND_SERVICE CAP_SETUID CAP_SETGID
Restart=always
RestartSec=2

[Install]
WantedBy=multi-user.target
EOF

"${SUDO_CMD[@]}" systemctl daemon-reload
"${SUDO_CMD[@]}" systemctl enable --now tandem-portal
"${SUDO_CMD[@]}" systemctl restart tandem-portal

log "Tandem Engine running. API token: $TOKEN"
log "Tandem Portal running from: $PROJECT_DIR"
log "Run checks: systemctl status tandem-engine tandem-portal; ss -ltnp | grep ':80 '; curl -I http://127.0.0.1:80"
if [[ "$HAS_PROVIDER_KEYS" -eq 0 ]]; then
  log "WARNING: No provider API key found in $ENGINE_ENV_PATH"
  log "Edit that file and set at least one key (e.g. OPENROUTER_API_KEY), then run: ${SUDO_CMD[*]} systemctl restart tandem-engine"
fi
