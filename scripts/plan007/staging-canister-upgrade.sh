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
EXPECTED_CHAIN_ID="84532"

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

for tool in cast icp python3; do
  command -v "$tool" >/dev/null || {
    echo "$tool is required" >&2
    exit 1
  }
done

PROFILE_VALUES=()
while IFS= read -r line; do
  PROFILE_VALUES[${#PROFILE_VALUES[@]}]="$line"
done < <(python3 - "$PROFILE" <<'PY'
import hashlib
import json
import re
import sys
from urllib.parse import urlsplit

with open(sys.argv[1], encoding="utf-8") as source:
    profile = json.load(source)
if profile.get("environment") != "sepolia-staging":
    raise SystemExit("staging profile has the wrong environment")
chain_id = profile.get("chainId")
if type(chain_id) is not int:
    raise SystemExit("staging profile has no integer chainId")
history = profile.get("baseHistoryRpcUrls")
if not isinstance(history, list):
    raise SystemExit("staging profile has no RPC history list")
urls = [profile.get("baseRpcUrl"), *history]
if len(urls) != 3 or len(set(urls)) != 3:
    raise SystemExit("staging profile must contain exactly three distinct RPC URLs")
for index, url in enumerate(urls):
    if not isinstance(url, str) or url != url.strip() or any(character.isspace() for character in url):
        raise SystemExit(f"staging RPC provider {index} is not a normalized URL")
    parsed = urlsplit(url)
    if parsed.scheme != "https" or not parsed.hostname or parsed.username or parsed.password:
        raise SystemExit(f"staging RPC provider {index} must be credential-free HTTPS")
canonical = json.dumps(urls, ensure_ascii=False, separators=(",", ":")).encode()
computed_digest = hashlib.sha256(canonical).hexdigest()
digest = profile.get("rpcProviderUrlsSha256")
if not isinstance(digest, str) or not re.fullmatch(r"0x[0-9a-fA-F]{64}", digest):
    raise SystemExit("staging profile has no valid rpcProviderUrlsSha256")
if digest[2:].lower() != computed_digest:
    raise SystemExit("staging profile RPC digest does not bind its configured URLs")
print(computed_digest)
print(chain_id)
print(*urls, sep="\n")
PY
)

if [[ "${#PROFILE_VALUES[@]}" -ne 5 ]]; then
  echo "staging profile did not produce one chain binding and three RPC URLs" >&2
  exit 1
fi
profile_digest="${PROFILE_VALUES[0]}"
profile_chain_id="${PROFILE_VALUES[1]}"
RPC_URLS=("${PROFILE_VALUES[@]:2}")

if [[ "$profile_digest" != "$NEW_RPC_DIGEST" ]]; then
  echo "staging profile does not require the reviewed OnFinality RPC digest: $profile_digest" >&2
  exit 1
fi
if [[ "$profile_chain_id" != "$EXPECTED_CHAIN_ID" ]]; then
  echo "staging profile chain ID is $profile_chain_id; expected $EXPECTED_CHAIN_ID" >&2
  exit 1
fi

candid_urls="$(python3 - "${RPC_URLS[@]}" <<'PY'
import json
import sys

print("; ".join(json.dumps(url) for url in sys.argv[1:]))
PY
)"
UPGRADE_ARGS="(record { rpc_provider_update = opt record { custom_evm_rpc_urls = vec { $candid_urls } } })"

verify_all_provider_chain_ids() {
  local index
  local observed_chain_id
  local rpc_url

  for index in "${!RPC_URLS[@]}"; do
    rpc_url="${RPC_URLS[$index]}"
    if ! observed_chain_id="$(cast chain-id --rpc-url "$rpc_url" 2>/dev/null)"; then
      echo "staging RPC provider $index chain ID check failed" >&2
      return 1
    fi
    if [[ "$observed_chain_id" != "$EXPECTED_CHAIN_ID" ]]; then
      echo "staging RPC provider $index returned chain ID $observed_chain_id; expected $EXPECTED_CHAIN_ID" >&2
      return 1
    fi
  done
}

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

verify_all_provider_chain_ids

before_digest="$(read_live_digest)"
case "$before_digest" in
  "$OLD_RPC_DIGEST") ;;
  "$NEW_RPC_DIGEST")
    echo "staging canister already uses the reviewed RPC digest: $before_digest"
    exit 0
    ;;
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
