#!/usr/bin/env bash
# Plan 007 manifest driver. External mutations remain separate, explicitly approved operator steps.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
MANIFEST="${BRIDGE_STAGING_E2E_MANIFEST:-$ROOT/deployments/sepolia-staging/evidence/sepolia-e2e.json}"
LOCAL_EVIDENCE="${BRIDGE_LOCAL_E2E_EVIDENCE:-}"
PROFILE="$ROOT/deployments/sepolia-staging/frontend-profile.json"
RECORDER="$ROOT/scripts/plan007/sepolia_e2e.py"
RPC_RECORDER="$ROOT/scripts/evm-rpc-rehearsal/rehearsal.py"
MODE="${1:-status}"

case "$MODE" in
  init)
    [[ -n "$LOCAL_EVIDENCE" && "$LOCAL_EVIDENCE" = /* && -f "$LOCAL_EVIDENCE" ]] || {
      echo "BRIDGE_LOCAL_E2E_EVIDENCE must name an existing absolute repo-external schema v8 evidence file" >&2
      exit 2
    }
    CANONICAL_LOCAL_EVIDENCE="$(python3 -c 'import pathlib, sys; print(pathlib.Path(sys.argv[1]).resolve(strict=True))' "$LOCAL_EVIDENCE")"
    CANONICAL_ROOT="$(python3 -c 'import pathlib, sys; print(pathlib.Path(sys.argv[1]).resolve(strict=True))' "$ROOT")"
    case "$CANONICAL_LOCAL_EVIDENCE" in
      "$CANONICAL_ROOT"|"$CANONICAL_ROOT"/*)
        echo "BRIDGE_LOCAL_E2E_EVIDENCE must stay outside the repository" >&2
        exit 2
        ;;
    esac
    python3 "$RECORDER" init "$MANIFEST" "$LOCAL_EVIDENCE" "$PROFILE" --repo-root "$ROOT"
    ;;
  status)
    if [[ ! -f "$MANIFEST" ]]; then
      echo "staging E2E manifest is not initialized: $MANIFEST" >&2
      exit 2
    fi
    python3 "$RECORDER" verify "$MANIFEST" --allow-incomplete
    ;;
  record)
    [[ "$#" -eq 2 ]] || {
      echo "usage: $0 record <stage-evidence.json>" >&2
      exit 2
    }
    python3 "$RECORDER" record "$MANIFEST" "$2"
    ;;
  verify)
    python3 "$RECORDER" verify "$MANIFEST"
    ;;
  rpc-verify)
    [[ "$#" -eq 2 ]] || {
      echo "usage: $0 rpc-verify <rpc-e2e.json>" >&2
      exit 2
    }
    python3 "$RPC_RECORDER" verify "$2"
    ;;
  rpc-capture-fault)
    [[ "$#" -eq 6 ]] || {
      echo "usage: $0 rpc-capture-fault <rpc-e2e.json> <rehearsal-config.json> <single_provider_failure|quorum_loss> <output.json> <run-reference>" >&2
      exit 2
    }
    export PATH="$ROOT/scripts/plan007:$PATH"
    python3 "$RPC_RECORDER" capture-fault "$2" "$3" "$4" "$5" "$6" -- evm-rpc-fault-injector
    ;;
  check-upgrade-instance)
    [[ "$#" -eq 3 ]] || {
      echo "usage: $0 check-upgrade-instance <live-public-config.json> <live-canister-status.json>" >&2
      exit 2
    }
    node "$ROOT/scripts/plan007/check-upgrade-instance.mjs" "$PROFILE" "$2" "$3"
    ;;
  *)
    echo "usage: $0 {init|status|record|verify|rpc-verify|rpc-capture-fault|check-upgrade-instance}" >&2
    exit 2
    ;;
esac
