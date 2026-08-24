#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
LOCAL_EVIDENCE="${1:-}"
export PATH="$ROOT/.tools/bin:$PATH"
export CI="${CI:-true}"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT/target}"

require_clean_checkout() {
  local phase="$1"
  if [[ -n "$(git -C "$ROOT" status --porcelain --untracked-files=all)" ]]; then
    echo "Refusing to $phase from a dirty working tree." >&2
    echo "Commit the reviewed build inputs before running the staging promotion gate." >&2
    exit 3
  fi
}

require_clean_checkout "start the local E2E gate"

if [[ -z "$LOCAL_EVIDENCE" || "$LOCAL_EVIDENCE" != /* ]]; then
  echo "Usage: $0 /absolute/path/outside/repository/local-e2e.json" >&2
  exit 2
fi

if rg -n '\bdfx\b' "$ROOT/icp.yaml" "$ROOT/scripts/plan007" "$ROOT/ui/scripts/build-sepolia-assets.mjs"; then
  echo "Plan 007 staging path must use ICP CLI only" >&2
  exit 1
fi

if rg -n 'frontend-sepolia|assetstorage|certified-assets' "$ROOT/icp.yaml"; then
  echo "Plan 007 frontend must be published through Cloudflare Workers, not an IC Asset Canister" >&2
  exit 1
fi

"$ROOT/scripts/ci-local.sh" all

require_clean_checkout "issue local E2E evidence"
node "$ROOT/scripts/plan007/generate-local-e2e.mjs" --output "$LOCAL_EVIDENCE"
require_clean_checkout "finish the local E2E gate"
