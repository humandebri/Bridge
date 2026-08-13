#!/usr/bin/env bash
# Upgrade the fixed Base Sepolia staging canister with the reviewed OnFinality provider set.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
CANISTER="bridge-sepolia"
ENVIRONMENT="sepolia-staging"
PROFILE="$ROOT/deployments/sepolia-staging/frontend-profile.json"
DID="$ROOT/canister/bridge-canister/bridge.did"
OLD_RPC_DIGEST="e9b9c716dedf57245c75b8d87114b065a55a96bd0f7bd56691683722ac5721fb"
NEW_RPC_DIGEST="3ab53c0532b80b3f39ed076f9661794c0a847b0d2eba1845b5c7e0ed1663ed48"
UPGRADE_ARGS='(record { rpc_provider_update = opt record { custom_evm_rpc_urls = vec { "https://base-sepolia-rpc.publicnode.com"; "https://sepolia.base.org"; "https://base-sepolia.api.onfinality.io/public" } } })'

usage() {
  echo "usage: BRIDGE_STAGING_IDENTITY=<identity> $0 --execute" >&2
}

if [[ "$#" -ne 1 || "$1" != "--execute" ]]; then
  usage
  exit 2
fi

if [[ -z "${BRIDGE_STAGING_IDENTITY:-}" ]]; then
  echo "BRIDGE_STAGING_IDENTITY is required" >&2
  usage
  exit 2
fi

profile_digest="$(python3 - "$PROFILE" <<'PY'
import json
import re
import sys

with open(sys.argv[1], encoding="utf-8") as source:
    digest = json.load(source).get("rpcProviderUrlsSha256")
if not isinstance(digest, str) or not re.fullmatch(r"0x[0-9a-fA-F]{64}", digest):
    raise SystemExit("staging profile has no valid rpcProviderUrlsSha256")
print(digest[2:].lower())
PY
)"

if [[ "$profile_digest" != "$NEW_RPC_DIGEST" ]]; then
  echo "staging profile does not require the reviewed OnFinality RPC digest: $profile_digest" >&2
  exit 1
fi

read_live_digest() {
  local response
  response="$(
    icp canister call "$CANISTER" get_public_config '()' \
      --query \
      -e "$ENVIRONMENT" \
      --identity "$BRIDGE_STAGING_IDENTITY" \
      --candid "$DID" \
      --json \
      --project-root-override "$ROOT"
  )"
  python3 - "$response" <<'PY'
import json
import re
import sys

payload = json.loads(sys.argv[1])
candid = payload.get("response_candid")
if not isinstance(candid, str):
    raise SystemExit("get_public_config did not return response_candid")

vector = re.findall(
    r"\brpc_provider_urls_sha256\s*=\s*vec\s*\{([^}]*)\}", candid, re.S
)
blob = re.findall(
    r'\brpc_provider_urls_sha256\s*=\s*blob\s*"([^"]*)"', candid, re.S
)
if len(vector) + len(blob) != 1:
    raise SystemExit("get_public_config did not expose exactly one RPC provider digest")

if vector:
    values = [int(value) for value in re.findall(r"(\d+)\s*:\s*nat8", vector[0])]
else:
    values = [int(value, 16) for value in re.findall(r"\\([0-9a-fA-F]{2})", blob[0])]
if len(values) != 32 or any(value > 255 for value in values):
    raise SystemExit("get_public_config returned an invalid RPC provider digest")
print(bytes(values).hex())
PY
}

before_digest="$(read_live_digest)"
case "$before_digest" in
  "$OLD_RPC_DIGEST"|"$NEW_RPC_DIGEST") ;;
  *)
    echo "refusing upgrade from unreviewed live RPC digest: $before_digest" >&2
    exit 1
    ;;
esac

icp deploy "$CANISTER" \
  -e "$ENVIRONMENT" \
  --identity "$BRIDGE_STAGING_IDENTITY" \
  --mode upgrade \
  --yes \
  --args "$UPGRADE_ARGS" \
  --project-root-override "$ROOT"

after_digest="$(read_live_digest)"
if [[ "$after_digest" != "$NEW_RPC_DIGEST" ]]; then
  echo "upgrade completed without activating the reviewed OnFinality RPC digest: $after_digest" >&2
  exit 1
fi

echo "staging canister upgrade verified: RPC digest $after_digest"
