#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
CONTRACTS="$ROOT/contracts"
RPC_URL="${BASE_SEPOLIA_RPC_URL:-https://base-sepolia-rpc.publicnode.com}"
CHAIN_ID=84532
DEPLOYER="0x7F4743128368CdeD5413E8c42C9Bd689ea64D192"
MANIFEST="${BASE_SEPOLIA_MANIFEST:-$ROOT/deployments/base-sepolia-contract-experiment.json}"
DEPLOYER_KEYSTORE="${BASE_SEPOLIA_DEPLOYER_KEYSTORE:-$HOME/.foundry/keystores/kinic-base-sepolia-experiment}"
SIGNER_KEYSTORE="${BASE_SEPOLIA_SIGNER_KEYSTORE:-$HOME/.foundry/keystores/kinic-base-sepolia-bridge-signer}"
CANCELLER_KEYSTORE="${BASE_SEPOLIA_CANCELLER_KEYSTORE:-$HOME/.foundry/keystores/kinic-base-sepolia-canceller}"
DEPLOYER_PASSWORD_FILE="${BASE_SEPOLIA_DEPLOYER_PASSWORD_FILE:-}"
SIGNER_PASSWORD_FILE="${BASE_SEPOLIA_SIGNER_PASSWORD_FILE:-}"
CANCELLER_PASSWORD_FILE="${BASE_SEPOLIA_CANCELLER_PASSWORD_FILE:-}"
EXTERNAL_BRIDGE_SIGNER="${BASE_SEPOLIA_EXTERNAL_BRIDGE_SIGNER:-}"
EXTERNAL_GOVERNANCE_OPERATOR="${BASE_SEPOLIA_EXTERNAL_GOVERNANCE_OPERATOR:-}"
EXTERNAL_RUNTIME_ADMINISTRATOR="${BASE_SEPOLIA_EXTERNAL_RUNTIME_ADMINISTRATOR:-}"
EXTERNAL_INDEPENDENT_CANCELLER="${BASE_SEPOLIA_EXTERNAL_INDEPENDENT_CANCELLER:-}"
TIMELOCK_DELAY="${BASE_SEPOLIA_TIMELOCK_DELAY_SECONDS:-259200}"
ZERO_ADDRESS="0x0000000000000000000000000000000000000000"
ZERO_BYTES32="0x0000000000000000000000000000000000000000000000000000000000000000"
MAX_EXPERIMENT_COST_WEI=20000000000000000
SIGNER_FUNDING_WEI=5000000000000000
CALL_GAS_BUDGET=5000000
BRIDGE_DEPLOY_GAS_BUDGET=5000000
DEFAULT_PER_DEPOSIT_LIMIT=15000000000000
DEFAULT_MINT_WINDOW_LIMIT=15000000000000
DEFAULT_MIN_SERVICE_FEE=10000
DEFAULT_MAX_SERVICE_FEE=1000000000
DEFAULT_INITIAL_SERVICE_FEE=50000000

external_control_plane() {
  [[ -n "$EXTERNAL_BRIDGE_SIGNER" || -n "$EXTERNAL_GOVERNANCE_OPERATOR" \
    || -n "$EXTERNAL_RUNTIME_ADMINISTRATOR" || -n "$EXTERNAL_INDEPENDENT_CANCELLER" ]]
}

require_external_control_plane() {
  [[ -n "$EXTERNAL_BRIDGE_SIGNER" && -n "$EXTERNAL_GOVERNANCE_OPERATOR" \
    && -n "$EXTERNAL_RUNTIME_ADMINISTRATOR" && -n "$EXTERNAL_INDEPENDENT_CANCELLER" ]] \
    || die "external control plane requires signer, governance operator, runtime administrator, and independent canceller"
}

die() {
  echo "error: $*" >&2
  exit 1
}

if [[ "$TIMELOCK_DELAY" != 259200 && "$TIMELOCK_DELAY" != 300 ]]; then
  die "Base Sepolia Timelock delay must be 259200 or the test-only staging value 300"
fi
if [[ "$TIMELOCK_DELAY" == 300 && "${BASE_SEPOLIA_SHORT_DELAY_TEST_ONLY:-}" != true ]]; then
  die "the 300-second Timelock requires BASE_SEPOLIA_SHORT_DELAY_TEST_ONLY=true"
fi

log() {
  echo "[$(date -u +%Y-%m-%dT%H:%M:%SZ)] $*" >&2
}

need() {
  command -v "$1" >/dev/null 2>&1 || die "required command not found: $1"
}

require_file() {
  [[ -f "$1" ]] || die "required file not found: $1"
}

hex_to_dec() {
  local value="$1"
  if [[ "$value" == 0x* ]]; then
    cast to-dec "$value"
  else
    echo "$value"
  fi
}

address_eq() {
  local left right
  left="$(printf '%s' "$1" | tr '[:upper:]' '[:lower:]')"
  right="$(printf '%s' "$2" | tr '[:upper:]' '[:lower:]')"
  [[ "$left" == "$right" ]]
}

manifest_update() {
  local filter="$1"
  shift
  local tmp
  tmp="$(mktemp "$ROOT/deployments/.base-sepolia-manifest.XXXXXX")"
  jq "$@" "$filter" "$MANIFEST" >"$tmp"
  mv "$tmp" "$MANIFEST"
}

manifest_get() {
  jq -er "$1" "$MANIFEST"
}

source_tree_hash() {
  (
    cd "$ROOT"
    while IFS= read -r path; do
      [[ "$path" == deployments/* ]] && continue
      [[ -f "$path" ]] || continue
      printf '%s\0' "$path"
      shasum -a 256 "$path"
    done < <(git ls-files --cached --others --exclude-standard | LC_ALL=C sort)
  ) | shasum -a 256 | awk '{print $1}'
}

dirty_diff_hash() {
  git -C "$ROOT" diff --binary HEAD -- . ':(exclude)deployments/**' | shasum -a 256 | awk '{print $1}'
}

check_chain() {
  local actual
  actual="$(cast chain-id --rpc-url "$RPC_URL")"
  [[ "$actual" == "$CHAIN_ID" ]] || die "RPC chain ID is $actual, expected $CHAIN_ID"
}

wallet_address() {
  cast wallet address --keystore "$1" --password-file "$2"
}

check_wallet() {
  local keystore="$1"
  local password_file="$2"
  local expected="$3"
  require_file "$keystore"
  require_file "$password_file"
  local actual
  actual="$(wallet_address "$keystore" "$password_file")"
  address_eq "$actual" "$expected" || die "keystore address $actual does not match expected $expected"
}

receipt_json() {
  cast receipt "$1" --rpc-url "$RPC_URL" --json
}

wait_receipt() {
  local tx_hash="$1"
  local deadline=$((SECONDS + 300))
  local receipt
  while (( SECONDS < deadline )); do
    if receipt="$(receipt_json "$tx_hash" 2>/dev/null)"; then
      echo "$receipt"
      return 0
    fi
    sleep 3
  done
  die "receipt was not available within 5 minutes: $tx_hash"
}

wait_transactions_confirmed() {
  (( $# > 0 )) || return 0
  local names=("$@")
  local deadline=$((SECONDS + 1800))
  local safe_json safe_hex safe_dec name receipt_block all_confirmed
  while (( SECONDS < deadline )); do
    if safe_json="$(cast block safe --rpc-url "$RPC_URL" --json 2>/dev/null)"; then
      safe_hex="$(jq -r '.number' <<<"$safe_json")"
      safe_dec="$(hex_to_dec "$safe_hex")"
      all_confirmed=true
      for name in "${names[@]}"; do
        receipt_block="$(jq -er --arg name "$name" '.transactions[$name].receipt_block' "$MANIFEST")"
        if (( safe_dec >= receipt_block )); then
          manifest_update \
            '.transactions[$name].confirmed = true | .transactions[$name].confirmed_block = $block | .transactions[$name].confirmed_at = $at' \
            --arg name "$name" --argjson block "$safe_dec" --arg at "$(date -u +%Y-%m-%dT%H:%M:%SZ)"
        else
          all_confirmed=false
        fi
      done
      if [[ "$all_confirmed" == true ]]; then
        log "Safe-confirmed stage transactions at observed block $safe_dec: ${names[*]}"
        return 0
      fi
    fi
    sleep 15
  done
  manifest_update '.state = "PENDING_CONFIRMATION" | .pending_transactions = $names' --argjson names "$(printf '%s\n' "${names[@]}" | jq -R . | jq -s .)"
  die "stage transactions did not reach the Safe head within 30 minutes; no replacement transaction was sent"
}

record_transaction() {
  local name="$1"
  local tx_hash="$2"
  local sender="$3"
  local target="$4"
  local nonce="$5"
  local expected_status="$6"
  local receipt="$7"
  local status_raw block_raw block_hash status block
  status_raw="$(jq -r '.status' <<<"$receipt")"
  block_raw="$(jq -r '.blockNumber' <<<"$receipt")"
  block_hash="$(jq -r '.blockHash' <<<"$receipt")"
  status="$(hex_to_dec "$status_raw")"
  block="$(hex_to_dec "$block_raw")"
  [[ "$status" == "$expected_status" ]] || die "$name receipt status is $status, expected $expected_status"
  manifest_update \
    '.transactions[$name] = {hash:$hash,from:$from,to:$to,nonce:$nonce,status:$status,receipt_block:$block,receipt_block_hash:$block_hash,confirmed:false}' \
    --arg name "$name" --arg hash "$tx_hash" --arg from "$sender" --arg to "$target" \
    --argjson nonce "$nonce" --argjson status "$status" --argjson block "$block" --arg block_hash "$block_hash"
  echo "$block"
}

reuse_transaction() {
  local name="$1"
  local expected_status="$2"
  local existing receipt transaction status sender nonce chain_nonce chain_sender
  existing="$(jq -r --arg name "$name" '.transactions[$name].hash // empty' "$MANIFEST")"
  [[ -n "$existing" ]] || return 1
  receipt="$(receipt_json "$existing")"
  status="$(hex_to_dec "$(jq -r '.status' <<<"$receipt")")"
  [[ "$status" == "$expected_status" ]] || die "recorded $name status changed or is invalid"
  sender="$(jq -r --arg name "$name" '.transactions[$name].from' "$MANIFEST")"
  nonce="$(jq -r --arg name "$name" '.transactions[$name].nonce' "$MANIFEST")"
  transaction="$(cast tx "$existing" --rpc-url "$RPC_URL" --json)"
  chain_nonce="$(hex_to_dec "$(jq -r '.nonce' <<<"$transaction")")"
  chain_sender="$(jq -r '.from' <<<"$transaction")"
  [[ "$nonce" == "$chain_nonce" ]] || die "recorded $name nonce does not match chain"
  address_eq "$sender" "$chain_sender" || die "recorded $name sender does not match chain"
  log "reuse completed transaction $name: $existing"
  echo "$existing"
}

send_success() {
  local name="$1"
  local sender="$2"
  local keystore="$3"
  local password_file="$4"
  local target="$5"
  shift 5
  if reuse_transaction "$name" 1 >/dev/null; then
    jq -r --arg name "$name" '.transactions[$name].hash' "$MANIFEST"
    return 0
  fi
  local nonce result tx_hash receipt block
  nonce="$(cast nonce "$sender" --block pending --rpc-url "$RPC_URL")"
  result="$(cast send "$target" "$@" --rpc-url "$RPC_URL" --chain "$CHAIN_ID" \
    --keystore "$keystore" --password-file "$password_file" --json)"
  tx_hash="$(jq -r '.transactionHash // .transaction_hash // empty' <<<"$result")"
  [[ -n "$tx_hash" ]] || die "could not parse transaction hash for $name"
  receipt="$(wait_receipt "$tx_hash")"
  record_transaction "$name" "$tx_hash" "$sender" "$target" "$nonce" 1 "$receipt" >/dev/null
  echo "$tx_hash"
}

send_expected_revert() {
  local name="$1"
  local sender="$2"
  local keystore="$3"
  local password_file="$4"
  local target="$5"
  shift 5
  if reuse_transaction "$name" 0 >/dev/null; then
    jq -r --arg name "$name" '.transactions[$name].hash' "$MANIFEST"
    return 0
  fi
  local nonce tx_hash receipt block
  nonce="$(cast nonce "$sender" --block pending --rpc-url "$RPC_URL")"
  tx_hash="$(cast send "$target" "$@" --rpc-url "$RPC_URL" --chain "$CHAIN_ID" \
    --keystore "$keystore" --password-file "$password_file" --gas-limit 500000 --async)"
  tx_hash="${tx_hash//\"/}"
  [[ "$tx_hash" == 0x* ]] || die "could not parse reverted transaction hash for $name"
  receipt="$(wait_receipt "$tx_hash")"
  record_transaction "$name" "$tx_hash" "$sender" "$target" "$nonce" 0 "$receipt" >/dev/null
  echo "$tx_hash"
}

assert_call_eq() {
  local expected="$1"
  local target="$2"
  local signature="$3"
  shift 3
  local actual
  actual="$(cast call "$target" "$signature" "$@" --rpc-url "$RPC_URL" --json | jq -r '.[0]')"
  [[ "$actual" == "$expected" ]] || die "$target $signature returned $actual, expected $expected"
}

assert_address_call() {
  local expected="$1"
  local target="$2"
  local signature="$3"
  local actual
  actual="$(cast call "$target" "$signature" --rpc-url "$RPC_URL")"
  address_eq "$actual" "$expected" || die "$target $signature returned $actual, expected $expected"
}

assert_reverts_call() {
  local from="$1"
  local target="$2"
  local data="$3"
  if cast call "$target" --data "$data" --from "$from" --rpc-url "$RPC_URL" >/dev/null 2>&1; then
    die "call unexpectedly succeeded from $from to $target with $data"
  fi
}

init_manifest() {
  [[ ! -e "$MANIFEST" ]] || return 0
  mkdir -p "$(dirname "$MANIFEST")"
  local revision dirty tree created
  revision="$(git -C "$ROOT" rev-parse HEAD)"
  dirty="$(dirty_diff_hash)"
  tree="$(source_tree_hash)"
  created="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
  jq -n \
    --arg created "$created" --arg rpc "$RPC_URL" --arg deployer "$DEPLOYER" \
    --arg revision "$revision" --arg dirty "$dirty" --arg tree "$tree" \
    --argjson chain "$CHAIN_ID" --argjson delay "$TIMELOCK_DELAY" \
    --argjson per_deposit_limit "$DEFAULT_PER_DEPOSIT_LIMIT" \
    --argjson mint_window_limit "$DEFAULT_MINT_WINDOW_LIMIT" \
    --argjson min_service_fee "$DEFAULT_MIN_SERVICE_FEE" \
    --argjson max_service_fee "$DEFAULT_MAX_SERVICE_FEE" \
    --argjson initial_service_fee "$DEFAULT_INITIAL_SERVICE_FEE" \
    '{schema_version:1,experiment:"base-sepolia-contract-only",test_only:true,state:"PREFLIGHT",created_at:$created,chain_id:$chain,rpc_url:$rpc,wallets:{deployer_base_admin_runtime:$deployer},source:{revision:$revision,dirty_diff_sha256:$dirty,source_tree_sha256:$tree},parameters:{token_name:"kinic",token_symbol:"KINIC",token_decimals:8,timelock_delay_seconds:$delay,per_deposit_limit:$per_deposit_limit,mint_window_limit:$mint_window_limit,mint_window_duration_seconds:3600,min_service_fee:$min_service_fee,max_service_fee:$max_service_fee,initial_service_fee:$initial_service_fee},contracts:{},transactions:{},checks:{}}' \
    >"$MANIFEST"
}

require_manifest_chain() {
  require_file "$MANIFEST"
  [[ "$(manifest_get '.chain_id')" == "$CHAIN_ID" ]] || die "manifest chain mismatch"
  address_eq "$(manifest_get '.wallets.deployer_base_admin_runtime')" "$DEPLOYER" || die "manifest deployer mismatch"
}

require_state() {
  local expected="$1"
  local actual
  actual="$(manifest_get '.state')"
  [[ "$actual" == "$expected" ]] || die "manifest state is $actual, expected $expected"
}

contract_code_hash() {
  local code
  code="$(cast code "$1" --rpc-url "$RPC_URL")"
  [[ "$code" != "0x" ]] || die "no contract bytecode at $1"
  cast keccak "$code"
}

record_contract() {
  local name="$1"
  local address="$2"
  local tx_hash="$3"
  local hash
  hash="$(contract_code_hash "$address")"
  manifest_update '.contracts[$name] = {address:$address,deployment_transaction:$tx,runtime_bytecode_hash:$hash}' \
    --arg name "$name" --arg address "$address" --arg tx "$tx_hash" --arg hash "$hash"
}

preflight() {
  need cast
  need forge
  need jq
  need git
  need shasum
  check_chain
  require_file "$DEPLOYER_PASSWORD_FILE"
  check_wallet "$DEPLOYER_KEYSTORE" "$DEPLOYER_PASSWORD_FILE" "$DEPLOYER"
  local signer governance_operator runtime_administrator canceller
  if external_control_plane; then
    require_external_control_plane
    signer="$EXTERNAL_BRIDGE_SIGNER"
    governance_operator="$EXTERNAL_GOVERNANCE_OPERATOR"
    runtime_administrator="$EXTERNAL_RUNTIME_ADMINISTRATOR"
    canceller="$EXTERNAL_INDEPENDENT_CANCELLER"
  else
    require_file "$SIGNER_PASSWORD_FILE"
    require_file "$CANCELLER_PASSWORD_FILE"
    signer="$(wallet_address "$SIGNER_KEYSTORE" "$SIGNER_PASSWORD_FILE")"
    governance_operator="$DEPLOYER"
    runtime_administrator="$DEPLOYER"
    canceller="$(wallet_address "$CANCELLER_KEYSTORE" "$CANCELLER_PASSWORD_FILE")"
  fi
  for role in "$signer" "$governance_operator" "$runtime_administrator" "$canceller"; do
    [[ "$role" =~ ^0x[0-9a-fA-F]{40}$ ]] || die "invalid control-plane address: $role"
  done
  address_eq "$signer" "$runtime_administrator" && die "bridge signer must differ from runtime administrator"
  address_eq "$canceller" "$governance_operator" && die "canceller must differ from governance operator"
  address_eq "$canceller" "$signer" && die "canceller must differ from bridge signer"
  init_manifest
  require_manifest_chain
  manifest_update \
    '.wallets.bridge_signer = $signer | .wallets.governance_operator = $governance | .wallets.runtime_administrator = $runtime | .wallets.independent_canceller = $canceller | .source.revision = $revision | .source.dirty_diff_sha256 = $dirty | .source.source_tree_sha256 = $tree' \
    --arg signer "$signer" --arg governance "$governance_operator" --arg runtime "$runtime_administrator" \
    --arg canceller "$canceller" --arg revision "$(git -C "$ROOT" rev-parse HEAD)" \
    --arg dirty "$(dirty_diff_hash)" --arg tree "$(source_tree_hash)"

  log "running local Solidity and ABI gates"
  FOUNDRY_PROFILE=staging forge fmt --root "$CONTRACTS" --check
  FOUNDRY_PROFILE=staging forge test --root "$CONTRACTS"
  python3 "$ROOT/scripts/abi_snapshot.py" --check
  git -C "$ROOT" diff --check

  if jq -e '.[] | select(.type == "function" and (.name == "setMintLimits" or .name == "reduceMintLimits"))' \
    "$ROOT/contracts/abi/Bridge.json" >/dev/null; then
    die "obsolete mint limit selector is present in Bridge ABI"
  fi

  FOUNDRY_PROFILE=staging forge build --root "$CONTRACTS"
  local timelock_bytecode timelock_args timelock_creation
  timelock_bytecode="$(jq -r '.bytecode.object' "$CONTRACTS/out-staging/BridgeTimelockController.sol/BridgeTimelockController.json")"
  timelock_args="$(cast abi-encode 'constructor(uint256,address[],address[],address[])' "$TIMELOCK_DELAY" "[$governance_operator]" "[$canceller]" "[$governance_operator]")"
  timelock_creation="${timelock_bytecode}${timelock_args#0x}"
  local timelock_gas bridge_gas gas_price total_gas upper_cost balance
  timelock_gas="$(cast estimate --rpc-url "$RPC_URL" --from "$DEPLOYER" --create "$timelock_creation")"
  # Bridge validates the already-deployed Timelock's extcodehash in its
  # constructor, so a pre-deployment eth_estimateGas cannot faithfully execute
  # it against a placeholder address. Use a conservative explicit budget.
  bridge_gas="$BRIDGE_DEPLOY_GAS_BUDGET"
  gas_price="$(cast gas-price --rpc-url "$RPC_URL")"
  total_gas=$((timelock_gas + bridge_gas + CALL_GAS_BUDGET))
  upper_cost=$((total_gas * gas_price))
  (( upper_cost <= MAX_EXPERIMENT_COST_WEI )) || die "gas upper bound $upper_cost wei exceeds 0.02 ETH"
  balance="$(cast balance "$DEPLOYER" --rpc-url "$RPC_URL")"
  (( balance >= upper_cost )) || die "deployer balance is below gas bound"
  manifest_update \
    '.preflight = {timelock_deploy_gas:$timelock_gas,bridge_deploy_gas:$bridge_gas,call_gas_budget:$call_gas,total_gas_upper_bound:$total_gas,gas_price_wei:$gas_price,cost_upper_bound_wei:$cost,balance_wei:$balance,maximum_cost_wei:$max} | .checks.local_gates = true | .state = "READY_TO_DEPLOY"' \
    --argjson timelock_gas "$timelock_gas" --argjson bridge_gas "$bridge_gas" --argjson call_gas "$CALL_GAS_BUDGET" \
    --argjson total_gas "$total_gas" --argjson gas_price "$gas_price" --argjson cost "$upper_cost" \
    --argjson balance "$balance" --argjson max "$MAX_EXPERIMENT_COST_WEI"
  log "preflight complete; upper bound $upper_cost wei"
}

deploy_contract() {
  local name="$1"
  local contract="$2"
  local sender="$3"
  local keystore="$4"
  local password_file="$5"
  shift 5
  local existing
  existing="$(jq -r --arg name "$name" '.contracts[$name].address // empty' "$MANIFEST")"
  if [[ -n "$existing" ]]; then
    contract_code_hash "$existing" >/dev/null
    echo "$existing"
    return 0
  fi
  local nonce result tx_hash address receipt block
  nonce="$(cast nonce "$sender" --block pending --rpc-url "$RPC_URL")"
  result="$(FOUNDRY_PROFILE=staging forge create --root "$CONTRACTS" "$contract" --broadcast --rpc-url "$RPC_URL" --chain "$CHAIN_ID" \
    --keystore "$keystore" --password-file "$password_file" --json --constructor-args "$@")"
  tx_hash="$(jq -r '.transactionHash // empty' <<<"$result")"
  address="$(jq -r '.deployedTo // empty' <<<"$result")"
  [[ -n "$tx_hash" && -n "$address" ]] || die "could not parse deployment result for $name"
  receipt="$(wait_receipt "$tx_hash")"
  record_transaction "deploy_$name" "$tx_hash" "$sender" "$ZERO_ADDRESS" "$nonce" 1 "$receipt" >/dev/null
  record_contract "$name" "$address" "$tx_hash"
  echo "$address"
}

deploy() {
  check_chain
  require_manifest_chain
  require_state READY_TO_DEPLOY
  check_wallet "$DEPLOYER_KEYSTORE" "$DEPLOYER_PASSWORD_FILE" "$DEPLOYER"
  local signer governance_operator runtime_administrator canceller signer_balance fund_tx timelock bridge bsns
  signer="$(manifest_get '.wallets.bridge_signer')"
  governance_operator="$(manifest_get '.wallets.governance_operator')"
  runtime_administrator="$(manifest_get '.wallets.runtime_administrator')"
  canceller="$(manifest_get '.wallets.independent_canceller')"
  if ! external_control_plane; then
    check_wallet "$SIGNER_KEYSTORE" "$SIGNER_PASSWORD_FILE" "$signer"
    check_wallet "$CANCELLER_KEYSTORE" "$CANCELLER_PASSWORD_FILE" "$canceller"
  fi
  signer_balance="$(cast balance "$signer" --rpc-url "$RPC_URL")"
  if ! external_control_plane && (( signer_balance < SIGNER_FUNDING_WEI )); then
    fund_tx="$(send_success fund_bridge_signer "$DEPLOYER" "$DEPLOYER_KEYSTORE" "$DEPLOYER_PASSWORD_FILE" \
      "$signer" --value "$SIGNER_FUNDING_WEI")"
    log "funded signer: $fund_tx"
  elif ! external_control_plane; then
    if ! jq -e '.transactions.fund_bridge_signer.hash' "$MANIFEST" >/dev/null; then
      local recovered_funding_tx recovered_receipt recovered_tx recovered_from recovered_to recovered_value recovered_nonce
      recovered_funding_tx="${BASE_SEPOLIA_FUNDING_TX:-}"
      [[ -n "$recovered_funding_tx" ]] || die "signer is funded but manifest lacks funding tx; set BASE_SEPOLIA_FUNDING_TX"
      recovered_receipt="$(receipt_json "$recovered_funding_tx")"
      recovered_tx="$(cast tx "$recovered_funding_tx" --rpc-url "$RPC_URL" --json)"
      recovered_from="$(jq -r '.from' <<<"$recovered_tx")"
      recovered_to="$(jq -r '.to' <<<"$recovered_tx")"
      recovered_value="$(hex_to_dec "$(jq -r '.value' <<<"$recovered_tx")")"
      recovered_nonce="$(hex_to_dec "$(jq -r '.nonce' <<<"$recovered_tx")")"
      address_eq "$recovered_from" "$DEPLOYER" || die "recovered funding sender mismatch"
      address_eq "$recovered_to" "$signer" || die "recovered funding recipient mismatch"
      [[ "$recovered_value" == "$SIGNER_FUNDING_WEI" ]] || die "recovered funding value mismatch"
      record_transaction fund_bridge_signer "$recovered_funding_tx" "$DEPLOYER" "$signer" "$recovered_nonce" 1 "$recovered_receipt" >/dev/null
    fi
    manifest_update '.checks.signer_already_funded = true | .balances.signer_before_deploy = $balance' \
      --argjson balance "$signer_balance"
  fi
  timelock="$(deploy_contract timelock \
    'src/BridgeTimelockController.sol:BridgeTimelockController' \
    "$DEPLOYER" "$DEPLOYER_KEYSTORE" "$DEPLOYER_PASSWORD_FILE" \
    "$TIMELOCK_DELAY" "[$governance_operator]" "[$canceller]" "[$governance_operator]")"
  local timelock_code_hash
  timelock_code_hash="$(cast codehash "$timelock" --rpc-url "$RPC_URL")"
  bridge="$(deploy_contract bridge 'src/Bridge.sol:Bridge' "$DEPLOYER" "$DEPLOYER_KEYSTORE" "$DEPLOYER_PASSWORD_FILE" \
    "$signer" "$runtime_administrator" "$timelock" "$timelock_code_hash" \
    "$(manifest_get '.parameters.per_deposit_limit')" "$(manifest_get '.parameters.mint_window_limit')" 3600 \
    "$(manifest_get '.parameters.min_service_fee')" "$(manifest_get '.parameters.max_service_fee')" \
    "$(manifest_get '.parameters.initial_service_fee')")"
  bsns="$(cast call "$bridge" 'bsns()(address)' --rpc-url "$RPC_URL")"
  record_contract bsns "$bsns" "$(manifest_get '.contracts.bridge.deployment_transaction')"
  if external_control_plane; then
    wait_transactions_confirmed deploy_timelock deploy_bridge
  else
    wait_transactions_confirmed fund_bridge_signer deploy_timelock deploy_bridge
  fi
  manifest_update '.state = "DEPLOYED" | .balances.deployer_after_deploy = $deployer | .balances.signer_after_deploy = $signer' \
    --argjson deployer "$(cast balance "$DEPLOYER" --rpc-url "$RPC_URL")" \
    --argjson signer "$(cast balance "$signer" --rpc-url "$RPC_URL")"
  verify_deployment
}

verify_deployment() {
  local signer independent_canceller timelock bridge bsns proposer canceller executor admin
  signer="$(manifest_get '.wallets.bridge_signer')"
  independent_canceller="$(manifest_get '.wallets.independent_canceller')"
  timelock="$(manifest_get '.contracts.timelock.address')"
  bridge="$(manifest_get '.contracts.bridge.address')"
  bsns="$(manifest_get '.contracts.bsns.address')"
  assert_address_call "$signer" "$bridge" 'bridgeSigner()(address)'
  assert_address_call "$(manifest_get '.wallets.runtime_administrator')" "$bridge" 'runtimeAdministrator()(address)'
  assert_address_call "$timelock" "$bridge" 'baseAdminTimelock()(address)'
  assert_address_call "$bsns" "$bridge" 'bsns()(address)'
  assert_address_call "$bridge" "$bsns" 'bridge()(address)'
  assert_call_eq "$TIMELOCK_DELAY" "$timelock" 'getMinDelay()(uint256)'
  assert_call_eq "$(manifest_get '.parameters.per_deposit_limit')" "$bridge" 'perDepositLimit()(uint256)'
  assert_call_eq "$(manifest_get '.parameters.mint_window_limit')" "$bridge" 'mintWindowLimit()(uint256)'
  assert_call_eq 3600 "$bridge" 'mintWindowDuration()(uint64)'
  assert_call_eq "$(manifest_get '.parameters.min_service_fee')" "$bridge" 'MIN_SERVICE_FEE()(uint256)'
  assert_call_eq "$(manifest_get '.parameters.max_service_fee')" "$bridge" 'MAX_SERVICE_FEE()(uint256)'
  local deployment_state
  deployment_state="$(manifest_get '.state')"
  if [[ "$deployment_state" == COMPLETE ]]; then
    assert_call_eq "$DEFAULT_INITIAL_SERVICE_FEE" "$bridge" 'serviceFee()(uint256)'
  else
    assert_call_eq "$(manifest_get '.parameters.initial_service_fee')" "$bridge" 'serviceFee()(uint256)'
  fi
  if [[ "$deployment_state" == DEPLOYED || "$deployment_state" == WAITING_TIMELOCK || "$deployment_state" == COMPLETE ]]; then
    assert_call_eq true "$bridge" 'depositMintsPaused()(bool)'
    assert_call_eq true "$bridge" 'withdrawalsPaused()(bool)'
  elif [[ "$deployment_state" == ACTIVE ]]; then
    assert_call_eq false "$bridge" 'depositMintsPaused()(bool)'
    assert_call_eq false "$bridge" 'withdrawalsPaused()(bool)'
  fi
  assert_call_eq 8 "$bsns" 'decimals()(uint8)'
  [[ "$(cast call "$bsns" 'name()(string)' --rpc-url "$RPC_URL")" == '"kinic"' ]] || die "token name mismatch"
  [[ "$(cast call "$bsns" 'symbol()(string)' --rpc-url "$RPC_URL")" == '"KINIC"' ]] || die "token symbol mismatch"
  proposer="$(cast call "$timelock" 'PROPOSER_ROLE()(bytes32)' --rpc-url "$RPC_URL")"
  canceller="$(cast call "$timelock" 'CANCELLER_ROLE()(bytes32)' --rpc-url "$RPC_URL")"
  executor="$(cast call "$timelock" 'EXECUTOR_ROLE()(bytes32)' --rpc-url "$RPC_URL")"
  admin="$(cast call "$timelock" 'DEFAULT_ADMIN_ROLE()(bytes32)' --rpc-url "$RPC_URL")"
  assert_call_eq true "$timelock" 'hasRole(bytes32,address)(bool)' "$proposer" "$(manifest_get '.wallets.governance_operator')"
  assert_call_eq false "$timelock" 'hasRole(bytes32,address)(bool)' "$canceller" "$DEPLOYER"
  assert_call_eq true "$timelock" 'hasRole(bytes32,address)(bool)' "$canceller" "$independent_canceller"
  assert_call_eq false "$timelock" 'hasRole(bytes32,address)(bool)' "$proposer" "$independent_canceller"
  assert_call_eq false "$timelock" 'hasRole(bytes32,address)(bool)' "$executor" "$independent_canceller"
  assert_call_eq true "$timelock" 'hasRole(bytes32,address)(bool)' "$executor" "$(manifest_get '.wallets.governance_operator')"
  assert_call_eq true "$timelock" 'hasRole(bytes32,address)(bool)' "$admin" "$timelock"
  assert_call_eq false "$timelock" 'hasRole(bytes32,address)(bool)' "$admin" "$DEPLOYER"
  if external_control_plane; then
    assert_call_eq false "$timelock" 'hasRole(bytes32,address)(bool)' "$proposer" "$DEPLOYER"
    assert_call_eq false "$timelock" 'hasRole(bytes32,address)(bool)' "$executor" "$DEPLOYER"
  fi
  local set_selector reduce_selector
  set_selector="$(cast sig 'setMintLimits(uint256,uint256,uint64)')"
  reduce_selector="$(cast sig 'reduceMintLimits(uint256,uint256,uint64)')"
  assert_reverts_call "$DEPLOYER" "$bridge" "$set_selector"
  assert_reverts_call "$DEPLOYER" "$bridge" "$reduce_selector"
  assert_reverts_call "$signer" "$bridge" "$set_selector"
  assert_reverts_call "$signer" "$bridge" "$reduce_selector"
  manifest_update '.checks.deployment = true | .checks.obsolete_limit_selectors_revert = true'
}

flow() {
  check_chain
  require_manifest_chain
  require_state ACTIVE
  [[ "$(manifest_get '.parameters.per_deposit_limit')" == "$DEFAULT_PER_DEPOSIT_LIMIT" ]] \
    || die "flow requires per-deposit limit=$DEFAULT_PER_DEPOSIT_LIMIT"
  [[ "$(manifest_get '.parameters.mint_window_limit')" == "$DEFAULT_MINT_WINDOW_LIMIT" ]] \
    || die "flow requires mint window limit=$DEFAULT_MINT_WINDOW_LIMIT"
  [[ "$(manifest_get '.parameters.min_service_fee')" == "$DEFAULT_MIN_SERVICE_FEE" ]] \
    || die "flow requires MIN_SERVICE_FEE=$DEFAULT_MIN_SERVICE_FEE"
  [[ "$(manifest_get '.parameters.max_service_fee')" == "$DEFAULT_MAX_SERVICE_FEE" ]] \
    || die "flow requires MAX_SERVICE_FEE=$DEFAULT_MAX_SERVICE_FEE"
  [[ "$(manifest_get '.parameters.initial_service_fee')" == "$DEFAULT_INITIAL_SERVICE_FEE" ]] \
    || die "flow requires initial service fee=$DEFAULT_INITIAL_SERVICE_FEE"
  local signer bridge bsns deposit_id owner subaccount tx withdrawal1 status1
  local authorization_epoch deadline typed_data signature
  signer="$(manifest_get '.wallets.bridge_signer')"
  bridge="$(manifest_get '.contracts.bridge.address')"
  bsns="$(manifest_get '.contracts.bsns.address')"
  check_wallet "$DEPLOYER_KEYSTORE" "$DEPLOYER_PASSWORD_FILE" "$DEPLOYER"
  check_wallet "$SIGNER_KEYSTORE" "$SIGNER_PASSWORD_FILE" "$signer"
  deposit_id="$(cast keccak 'base-sepolia-contract-experiment-deposit-1')"
  authorization_epoch="$(cast call "$bridge" 'mintAuthorizationEpoch()(uint256)' --rpc-url "$RPC_URL")"
  deadline="$(( $(hex_to_dec "$(jq -r '.timestamp' <<<"$(cast block latest --rpc-url "$RPC_URL" --json)")") + 3600 ))"
  typed_data="$(
    jq -cn \
      --arg deposit_id "$deposit_id" \
      --arg recipient "$DEPLOYER" \
      --arg bridge "$bridge" \
      --argjson deadline "$deadline" \
      --argjson authorization_epoch "$authorization_epoch" \
      '{
        types: {
          EIP712Domain: [
            {name:"name",type:"string"},
            {name:"version",type:"string"},
            {name:"chainId",type:"uint256"},
            {name:"verifyingContract",type:"address"}
          ],
          MintAuthorization: [
            {name:"depositId",type:"bytes32"},
            {name:"recipient",type:"address"},
            {name:"grossAmount",type:"uint256"},
            {name:"maxServiceFee",type:"uint256"},
            {name:"chargedServiceFee",type:"uint256"},
            {name:"deadline",type:"uint256"},
            {name:"authorizationEpoch",type:"uint256"}
          ]
        },
        primaryType:"MintAuthorization",
        domain:{name:"KINIC Bridge",version:"1",chainId:84532,verifyingContract:$bridge},
        message:{
          depositId:$deposit_id,
          recipient:$recipient,
          grossAmount:150000000,
          maxServiceFee:1000000000,
          chargedServiceFee:50000000,
          deadline:$deadline,
          authorizationEpoch:$authorization_epoch
        }
      }'
  )"
  signature="$(
    cast wallet sign "$typed_data" --data \
      --keystore "$SIGNER_KEYSTORE" --password-file "$SIGNER_PASSWORD_FILE"
  )"
  send_success authorization_mint "$DEPLOYER" "$DEPLOYER_KEYSTORE" "$DEPLOYER_PASSWORD_FILE" "$bridge" \
    'mintDepositWithAuthorization((bytes32,address,uint256,uint256,uint256,uint256,uint256),bytes)' \
    "($deposit_id,$DEPLOYER,150000000,1000000000,50000000,$deadline,$authorization_epoch)" \
    "$signature" >/dev/null
  if ! jq -e '.transactions.create_withdrawal_1.hash' "$MANIFEST" >/dev/null; then
    assert_call_eq 100000000 "$bsns" 'balanceOf(address)(uint256)' "$DEPLOYER"
  fi
  owner=0x01
  subaccount="$ZERO_BYTES32"
  send_success approve_withdrawal_1 "$DEPLOYER" "$DEPLOYER_KEYSTORE" "$DEPLOYER_PASSWORD_FILE" "$bsns" \
    'approve(address,uint256)' "$bridge" 75000000 >/dev/null
  send_success create_withdrawal_1 "$DEPLOYER" "$DEPLOYER_KEYSTORE" "$DEPLOYER_PASSWORD_FILE" "$bridge" \
    'createWithdrawal(uint256,uint256,bytes,bytes32)' 75000000 50000000 "$owner" "$subaccount" >/dev/null
  assert_call_eq 25000000 "$bsns" 'balanceOf(address)(uint256)' "$DEPLOYER"
  assert_call_eq 25000000 "$bsns" 'totalSupply()(uint256)'
  withdrawal1="$(cast call "$bridge" 'getWithdrawal(uint256)((address,uint256,uint256,uint256,uint256,bytes,bytes32,uint8))' 1 --rpc-url "$RPC_URL" --json)"
  status1="$(jq -r '.[0][7]' <<<"$withdrawal1")"
  [[ "$status1" == 1 ]] || die "withdrawal 1 status is $status1, expected Committed(1)"
  send_success set_service_fee_1_kinic "$DEPLOYER" "$DEPLOYER_KEYSTORE" "$DEPLOYER_PASSWORD_FILE" "$bridge" \
    'setServiceFee(uint256)' 100000000 >/dev/null
  send_success restore_service_fee_half_kinic "$DEPLOYER" "$DEPLOYER_KEYSTORE" "$DEPLOYER_PASSWORD_FILE" "$bridge" \
    'setServiceFee(uint256)' 50000000 >/dev/null
  send_success pause_deposits "$DEPLOYER" "$DEPLOYER_KEYSTORE" "$DEPLOYER_PASSWORD_FILE" "$bridge" \
    'pauseDepositMints()' >/dev/null
  send_success pause_withdrawals "$DEPLOYER" "$DEPLOYER_KEYSTORE" "$DEPLOYER_PASSWORD_FILE" "$bridge" \
    'pauseWithdrawals()' >/dev/null
  wait_transactions_confirmed authorization_mint approve_withdrawal_1 create_withdrawal_1 \
    set_service_fee_1_kinic restore_service_fee_half_kinic pause_deposits pause_withdrawals
  assert_call_eq true "$bridge" 'depositMintsPaused()(bool)'
  assert_call_eq true "$bridge" 'withdrawalsPaused()(bool)'
  assert_call_eq 50000000 "$bridge" 'serviceFee()(uint256)'
  assert_reverts_call "$DEPLOYER" "$bridge" "$(cast calldata 'unpauseDepositMints()')"
  manifest_update '.checks.asset_flow = true | .checks.direct_unpause_rejected = true | .state = "COMPLETE" | .completed_at = $at' \
    --arg at "$(date -u +%Y-%m-%dT%H:%M:%SZ)"
}

schedule() {
  check_chain
  require_manifest_chain
  require_state DEPLOYED
  check_wallet "$DEPLOYER_KEYSTORE" "$DEPLOYER_PASSWORD_FILE" "$DEPLOYER"
  local timelock bridge data1 data2 targets values payloads salt operation_id ready now
  timelock="$(manifest_get '.contracts.timelock.address')"
  bridge="$(manifest_get '.contracts.bridge.address')"
  data1="$(cast calldata 'unpauseDepositMints()')"
  data2="$(cast calldata 'unpauseWithdrawals()')"
  targets="[$bridge,$bridge]"
  values='[0,0]'
  payloads="[$data1,$data2]"
  salt="$(cast keccak 'base-sepolia-contract-experiment-unpause-v1')"
  operation_id="$(cast call "$timelock" \
    'hashOperationBatch(address[],uint256[],bytes[],bytes32,bytes32)(bytes32)' \
    "$targets" "$values" "$payloads" "$ZERO_BYTES32" "$salt" --rpc-url "$RPC_URL")"
  send_success schedule_unpause "$DEPLOYER" "$DEPLOYER_KEYSTORE" "$DEPLOYER_PASSWORD_FILE" "$timelock" \
    'scheduleBatch(address[],uint256[],bytes[],bytes32,bytes32,uint256)' \
    "$targets" "$values" "$payloads" "$ZERO_BYTES32" "$salt" "$TIMELOCK_DELAY" >/dev/null
  ready="$(cast call "$timelock" 'getTimestamp(bytes32)(uint256)' "$operation_id" --rpc-url "$RPC_URL" --json | jq -r '.[0]')"
  now="$(hex_to_dec "$(jq -r '.timestamp' <<<"$(cast block latest --rpc-url "$RPC_URL" --json)")")"
  (( ready > now )) || die "timelock operation is unexpectedly ready"
  manifest_update \
    '.timelock_operation = {id:$id,predecessor:$predecessor,salt:$salt,ready_timestamp:$ready,targets:[$bridge,$bridge],values:[0,0],payloads:[$data1,$data2]}' \
    --arg id "$operation_id" --arg predecessor "$ZERO_BYTES32" --arg salt "$salt" --argjson ready "$ready" \
    --arg bridge "$bridge" --arg data1 "$data1" --arg data2 "$data2"
  send_expected_revert early_execute_unpause "$DEPLOYER" "$DEPLOYER_KEYSTORE" "$DEPLOYER_PASSWORD_FILE" "$timelock" \
    'executeBatch(address[],uint256[],bytes[],bytes32,bytes32)' \
    "$targets" "$values" "$payloads" "$ZERO_BYTES32" "$salt" >/dev/null
  wait_transactions_confirmed schedule_unpause early_execute_unpause
  manifest_update '.checks.early_execute_reverted = true | .state = "WAITING_TIMELOCK"'
  log "timelock scheduled; ready timestamp $ready"
}

resume() {
  check_chain
  require_manifest_chain
  require_state WAITING_TIMELOCK
  check_wallet "$DEPLOYER_KEYSTORE" "$DEPLOYER_PASSWORD_FILE" "$DEPLOYER"
  local timelock bridge ready now targets values payloads predecessor salt
  timelock="$(manifest_get '.contracts.timelock.address')"
  bridge="$(manifest_get '.contracts.bridge.address')"
  ready="$(manifest_get '.timelock_operation.ready_timestamp')"
  now="$(hex_to_dec "$(jq -r '.timestamp' <<<"$(cast block latest --rpc-url "$RPC_URL" --json)")")"
  (( now >= ready )) || die "timelock is not ready; current=$now ready=$ready"
  targets="[$bridge,$bridge]"
  values='[0,0]'
  payloads="[$(manifest_get '.timelock_operation.payloads[0]'),$(manifest_get '.timelock_operation.payloads[1]')]"
  predecessor="$(manifest_get '.timelock_operation.predecessor')"
  salt="$(manifest_get '.timelock_operation.salt')"
  send_success execute_unpause "$DEPLOYER" "$DEPLOYER_KEYSTORE" "$DEPLOYER_PASSWORD_FILE" "$timelock" \
    'executeBatch(address[],uint256[],bytes[],bytes32,bytes32)' \
    "$targets" "$values" "$payloads" "$predecessor" "$salt" >/dev/null
  assert_call_eq false "$bridge" 'depositMintsPaused()(bool)'
  assert_call_eq false "$bridge" 'withdrawalsPaused()(bool)'
  wait_transactions_confirmed execute_unpause
  manifest_update '.checks.timelock_unpause = true | .state = "ACTIVE"'
  verify
}

verify() {
  check_chain
  require_manifest_chain
  local bridge bsns state status1 withdrawal1
  bridge="$(manifest_get '.contracts.bridge.address')"
  bsns="$(manifest_get '.contracts.bsns.address')"
  state="$(manifest_get '.state')"
  contract_code_hash "$bridge" >/dev/null
  contract_code_hash "$bsns" >/dev/null
  verify_deployment
  if [[ "$state" == DEPLOYED ]]; then
    assert_call_eq true "$bridge" 'depositMintsPaused()(bool)'
    assert_call_eq true "$bridge" 'withdrawalsPaused()(bool)'
    return
  fi
  if [[ "$state" == WAITING_TIMELOCK ]]; then
    assert_call_eq true "$bridge" 'depositMintsPaused()(bool)'
    assert_call_eq true "$bridge" 'withdrawalsPaused()(bool)'
    assert_call_eq true "$(manifest_get '.contracts.timelock.address')" 'isOperationPending(bytes32)(bool)' \
      "$(manifest_get '.timelock_operation.id')"
    assert_call_eq "$(manifest_get '.timelock_operation.ready_timestamp')" \
      "$(manifest_get '.contracts.timelock.address')" 'getTimestamp(bytes32)(uint256)' \
      "$(manifest_get '.timelock_operation.id')"
    return
  fi
  if [[ "$state" == ACTIVE ]]; then
    assert_call_eq false "$bridge" 'depositMintsPaused()(bool)'
    assert_call_eq false "$bridge" 'withdrawalsPaused()(bool)'
    return
  fi
  [[ "$state" == COMPLETE ]] || die "unsupported manifest state $state"
  assert_call_eq 25000000 "$bsns" 'balanceOf(address)(uint256)' "$DEPLOYER"
  assert_call_eq 25000000 "$bsns" 'totalSupply()(uint256)'
  withdrawal1="$(cast call "$bridge" 'getWithdrawal(uint256)((address,uint256,uint256,uint256,uint256,bytes,bytes32,uint8))' 1 --rpc-url "$RPC_URL" --json)"
  status1="$(jq -r '.[0][7]' <<<"$withdrawal1")"
  [[ "$status1" == 1 ]] || die "withdrawal 1 status is $status1, expected Committed(1)"
  address_eq "$(jq -r '.[0][0]' <<<"$withdrawal1")" "$DEPLOYER" \
    || die "withdrawal 1 requester mismatch"
  [[ "$(jq -r '.[0][1]' <<<"$withdrawal1")" == 75000000 ]] || die "withdrawal 1 amount mismatch"
  [[ "$(jq -r '.[0][2]' <<<"$withdrawal1")" == 50000000 ]] || die "withdrawal 1 max fee mismatch"
  [[ "$(jq -r '.[0][3]' <<<"$withdrawal1")" == 50000000 ]] || die "withdrawal 1 charged fee mismatch"
  [[ "$(jq -r '.[0][4]' <<<"$withdrawal1")" == 25000000 ]] || die "withdrawal 1 amount out mismatch"
  assert_call_eq true "$bridge" 'depositMintsPaused()(bool)'
  assert_call_eq true "$bridge" 'withdrawalsPaused()(bool)'
  assert_call_eq "$DEFAULT_INITIAL_SERVICE_FEE" "$bridge" 'serviceFee()(uint256)'
  local name hash receipt recorded_hash status block safe
  while IFS= read -r name; do
    hash="$(jq -r --arg name "$name" '.transactions[$name].hash' "$MANIFEST")"
    receipt="$(receipt_json "$hash")"
    recorded_hash="$(jq -r '.transactionHash' <<<"$receipt")"
    address_eq "$recorded_hash" "$hash" || die "receipt hash mismatch for $name"
    status="$(hex_to_dec "$(jq -r '.status' <<<"$receipt")")"
    [[ "$status" == "$(jq -r --arg name "$name" '.transactions[$name].status' "$MANIFEST")" ]] || die "receipt status mismatch for $name"
    block="$(hex_to_dec "$(jq -r '.blockNumber' <<<"$receipt")")"
    safe="$(hex_to_dec "$(jq -r '.number' <<<"$(cast block safe --rpc-url "$RPC_URL" --json)")")"
    (( safe >= block )) || die "$name receipt block has not reached the Safe head"
  done < <(jq -r '.transactions | keys[]' "$MANIFEST")
  manifest_update '.checks.rpc_reread = true | .last_verified_at = $at | .last_confirmed_base_block = $block' \
    --arg at "$(date -u +%Y-%m-%dT%H:%M:%SZ)" \
    --argjson block "$(hex_to_dec "$(jq -r '.number' <<<"$(cast block safe --rpc-url "$RPC_URL" --json)")")"
  log "verification complete; state=$state"
}

usage() {
  cat <<'EOF'
Usage: experiment.sh preflight|deploy|verify-deployment|flow|schedule|resume|verify

Signing stages require BASE_SEPOLIA_DEPLOYER_PASSWORD_FILE and/or
BASE_SEPOLIA_SIGNER_PASSWORD_FILE. Prefer run-with-keychain.sh so passwords are
never placed in the environment or command line.
EOF
}

main() {
  local command="${1:-}"
  case "$command" in
    preflight) preflight ;;
    deploy) deploy ;;
    verify-deployment) check_chain; require_manifest_chain; verify_deployment ;;
    flow) flow ;;
    schedule) schedule ;;
    resume) resume ;;
    verify) verify ;;
    *) usage; exit 2 ;;
  esac
}

main "$@"
