#!/bin/sh
set -eu

: "${TANDEM_ENGINE_HOST:=0.0.0.0}"
: "${TANDEM_ENGINE_PORT:=39731}"
: "${TANDEM_STATE_DIR:=/var/lib/tandem/engine}"
: "${TANDEM_API_TOKEN_FILE:=/run/secrets/tandem_api_token}"

if [ ! -f "$TANDEM_API_TOKEN_FILE" ] || [ ! -r "$TANDEM_API_TOKEN_FILE" ] || [ ! -s "$TANDEM_API_TOKEN_FILE" ]; then
  echo "[tandem-engine] API token secret must be a non-empty readable file: $TANDEM_API_TOKEN_FILE" >&2
  echo "[tandem-engine] create it on the host before starting the container" >&2
  exit 1
fi

token="$(tr -d '\r\n' < "$TANDEM_API_TOKEN_FILE")"
if ! printf '%s' "$token" | grep -Eq '^tk_[0-9a-f]{32}$'; then
  echo "[tandem-engine] API token secret has an invalid format" >&2
  exit 1
fi

export TANDEM_API_TOKEN="$token"
export TANDEM_ENGINE_HOST
export TANDEM_ENGINE_PORT
export TANDEM_STATE_DIR

exec tandem-engine serve --hostname "$TANDEM_ENGINE_HOST" --port "$TANDEM_ENGINE_PORT"
