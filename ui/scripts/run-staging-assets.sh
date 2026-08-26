#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
SOURCE_ROOT="$(cd -- "$SCRIPT_DIR/../.." && pwd)"
VERSION_FILE="$SOURCE_ROOT/.node-version"

if [[ ! -f "$VERSION_FILE" || -L "$VERSION_FILE" ]]; then
  echo "staging UI entrypoint requires a regular .node-version file" >&2
  exit 1
fi

required_version="$(<"$VERSION_FILE")"
if [[ ! "$required_version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  echo "staging UI entrypoint found an invalid .node-version" >&2
  exit 1
fi

current_version="$(node --version 2>/dev/null || true)"
if [[ "$current_version" != "v$required_version" ]]; then
  if [[ "${BRIDGE_STAGING_NODE_REEXEC:-0}" == "1" ]]; then
    echo "fnm did not activate required Node v$required_version (current: ${current_version:-missing})" >&2
    exit 1
  fi
  if ! command -v fnm >/dev/null 2>&1; then
    echo "staging UI entrypoint requires fnm to activate Node v$required_version" >&2
    exit 1
  fi
  export BRIDGE_STAGING_NODE_REEXEC=1
  exec fnm exec --using "$required_version" -- "$0" "$@"
fi

unset BRIDGE_STAGING_NODE_REEXEC
exec node "$SCRIPT_DIR/staging-assets.mjs" "$@"
