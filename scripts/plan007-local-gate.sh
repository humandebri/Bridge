#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
export PATH="$ROOT/.tools/bin:$PATH"

if rg -n '\bdfx\b' "$ROOT/icp.yaml" "$ROOT/scripts/plan007" "$ROOT/ui/scripts/build-sepolia-assets.mjs"; then
  echo "Plan 007 staging path must use ICP CLI only" >&2
  exit 1
fi

if rg -n 'frontend-sepolia|assetstorage|certified-assets' "$ROOT/icp.yaml"; then
  echo "Plan 007 frontend must be published through Cloudflare Workers, not an IC Asset Canister" >&2
  exit 1
fi

"$ROOT/scripts/ci-local.sh" all

if [[ -n "$(git -C "$ROOT" status --porcelain)" ]]; then
  echo "Local E2E passed, but the working tree is dirty; local-e2e.json was not issued." >&2
  echo "Commit the reviewed changes and rerun this gate before any staging deployment." >&2
  exit 3
fi

node "$ROOT/scripts/plan007/generate-local-e2e.mjs"
