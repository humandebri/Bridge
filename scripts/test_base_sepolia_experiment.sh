#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TEST_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/base-sepolia-experiment-test.XXXXXX")"
trap 'rm -rf "$TEST_ROOT"' EXIT
mkdir -p "$TEST_ROOT/bin"

TRACE="$TEST_ROOT/trace"
BROADCAST_MARKER="$TEST_ROOT/broadcast"
export TRACE BROADCAST_MARKER
: >"$TRACE"

cat >"$TEST_ROOT/bin/cast" <<'SH'
#!/usr/bin/env bash
printf 'cast %s\n' "$*" >>"$TRACE"
if [[ "${1:-} ${2:-}" == "chain-id --rpc-url" ]]; then
  printf '84532\n'
  exit 0
fi
if [[ "${CAST_SCHEDULE_MISMATCH_TEST:-}" == 1 ]]; then
  if [[ "$*" == *'getMinDelay()(uint256)'* ]]; then
    printf '["300"]\n'
    exit 0
  fi
  case "${1:-} ${2:-}" in
    "wallet address")
      printf '0x7F4743128368CdeD5413E8c42C9Bd689ea64D192\n'
      exit 0
      ;;
    "calldata unpauseDepositMints()")
      printf '0x11111111\n'
      exit 0
      ;;
    "calldata unpauseWithdrawals()")
      printf '0x22222222\n'
      exit 0
      ;;
    "calldata scheduleBatch(address[],uint256[],bytes[],bytes32,bytes32,uint256)")
      printf '0xaaaaaaaa\n'
      exit 0
      ;;
    "keccak base-sepolia-contract-experiment-unpause-v1")
      printf '0x%s\n' "$(printf 'b%.0s' {1..64})"
      exit 0
      ;;
    "call 0x5555555555555555555555555555555555555555")
      printf '0x%s\n' "$(printf 'c%.0s' {1..64})"
      exit 0
      ;;
    "tx 0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
      printf '%s\n' '{"from":"0x7F4743128368CdeD5413E8c42C9Bd689ea64D192","to":"0x5555555555555555555555555555555555555555","input":"0xdeadbeef"}'
      exit 0
      ;;
    "send "*)
      touch "$BROADCAST_MARKER"
      exit 99
      ;;
  esac
fi
exit 97
SH

cat >"$TEST_ROOT/bin/forge" <<'SH'
#!/usr/bin/env bash
printf 'forge %s\n' "$*" >>"$TRACE"
touch "$BROADCAST_MARKER"
exit 98
SH
chmod +x "$TEST_ROOT/bin/cast" "$TEST_ROOT/bin/forge"

write_manifest() {
  local delay="$1"
  local mode=local-keystore
  if (( $# > 1 )); then
    mode="$2"
  fi
  jq -n --argjson delay "$delay" --arg mode "$mode" '{
    schema_version: 1,
    control_plane_mode: $mode,
    state: "PREFLIGHT",
    chain_id: 84532,
    wallets: {
      deployer_base_admin_runtime: "0x7F4743128368CdeD5413E8c42C9Bd689ea64D192",
      bridge_signer: "0x1111111111111111111111111111111111111111",
      governance_operator: "0x2222222222222222222222222222222222222222",
      runtime_administrator: "0x3333333333333333333333333333333333333333",
      independent_canceller: "0x4444444444444444444444444444444444444444"
    },
    parameters: {timelock_delay_seconds: $delay}
  }' >"$TEST_ROOT/manifest.json"
}

run_mode() {
  local mode="$1"
  shift
  PATH="$TEST_ROOT/bin:$PATH" \
    BASE_SEPOLIA_MANIFEST="$TEST_ROOT/manifest.json" \
    "$@" bash "$ROOT/scripts/base-sepolia-experiment/experiment.sh" "$mode"
}

run_deploy() {
  run_mode deploy "$@"
}

expect_rejected_before_broadcast() {
  local expected="$1"
  shift
  local before after
  before="$(shasum -a 256 "$TEST_ROOT/manifest.json" | awk '{print $1}')"
  : >"$TRACE"
  rm -f "$BROADCAST_MARKER"
  if run_deploy "$@" >"$TEST_ROOT/stdout" 2>"$TEST_ROOT/stderr"; then
    echo "expected experiment deploy to fail" >&2
    exit 1
  fi
  grep -F "$expected" "$TEST_ROOT/stderr" >/dev/null
  after="$(shasum -a 256 "$TEST_ROOT/manifest.json" | awk '{print $1}')"
  [[ "$before" == "$after" ]]
  [[ ! -e "$BROADCAST_MARKER" ]]
  ! grep -q '^forge ' "$TRACE"
}

write_manifest 300
expect_rejected_before_broadcast \
  "manifest Timelock delay 300 does not match invocation delay 259200" \
  env BASE_SEPOLIA_TIMELOCK_DELAY_SECONDS=259200

write_manifest 259200
expect_rejected_before_broadcast \
  "manifest Timelock delay 259200 does not match invocation delay 300" \
  env BASE_SEPOLIA_TIMELOCK_DELAY_SECONDS=300 BASE_SEPOLIA_SHORT_DELAY_TEST_ONLY=true

write_manifest 42
expect_rejected_before_broadcast \
  "Base Sepolia Timelock delay must be 259200 or the test-only staging value 300" \
  env -u BASE_SEPOLIA_TIMELOCK_DELAY_SECONDS

write_manifest 300
expect_rejected_before_broadcast \
  "manifest state is PREFLIGHT, expected READY_TO_DEPLOY" \
  env -u BASE_SEPOLIA_TIMELOCK_DELAY_SECONDS BASE_SEPOLIA_SHORT_DELAY_TEST_ONLY=true

write_manifest 300
expect_rejected_before_broadcast \
  "the manifest's 300-second Timelock requires BASE_SEPOLIA_SHORT_DELAY_TEST_ONLY=true for state-changing stages" \
  env -u BASE_SEPOLIA_TIMELOCK_DELAY_SECONDS -u BASE_SEPOLIA_SHORT_DELAY_TEST_ONLY

write_manifest 300 external
jq '.state = "DEPLOYED"' "$TEST_ROOT/manifest.json" >"$TEST_ROOT/manifest.next.json"
mv "$TEST_ROOT/manifest.next.json" "$TEST_ROOT/manifest.json"
: >"$TRACE"
rm -f "$BROADCAST_MARKER"
if run_mode schedule env -u BASE_SEPOLIA_TIMELOCK_DELAY_SECONDS BASE_SEPOLIA_SHORT_DELAY_TEST_ONLY=true >"$TEST_ROOT/stdout" 2>"$TEST_ROOT/stderr"; then
  echo "expected external-control-plane schedule to fail" >&2
  exit 1
fi
grep -F "external-control-plane staging must use Plan 007 governance/wallet stages" "$TEST_ROOT/stderr" >/dev/null
[[ ! -e "$BROADCAST_MARKER" ]]
! grep -q 'cast send' "$TRACE"

write_manifest 300
jq '.state = "DEPLOYED"
  | .contracts.timelock.address = "0x5555555555555555555555555555555555555555"
  | .contracts.bridge.address = "0x6666666666666666666666666666666666666666"
  | .transactions.schedule_unpause.hash = "0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"' \
  "$TEST_ROOT/manifest.json" >"$TEST_ROOT/manifest.next.json"
mv "$TEST_ROOT/manifest.next.json" "$TEST_ROOT/manifest.json"
touch "$TEST_ROOT/deployer-keystore" "$TEST_ROOT/deployer-password"
: >"$TRACE"
rm -f "$BROADCAST_MARKER"
if run_mode schedule env \
  CAST_SCHEDULE_MISMATCH_TEST=1 \
  BASE_SEPOLIA_SHORT_DELAY_TEST_ONLY=true \
  BASE_SEPOLIA_DEPLOYER_KEYSTORE="$TEST_ROOT/deployer-keystore" \
  BASE_SEPOLIA_DEPLOYER_PASSWORD_FILE="$TEST_ROOT/deployer-password" \
  >"$TEST_ROOT/stdout" 2>"$TEST_ROOT/stderr"; then
  echo "expected unverified deployment schedule to fail" >&2
  exit 1
fi
grep -F "schedule requires a successfully verified deployment" "$TEST_ROOT/stderr" >/dev/null
[[ ! -e "$BROADCAST_MARKER" ]]
! grep -q '^cast send ' "$TRACE"

jq '.checks.deployment = true' \
  "$TEST_ROOT/manifest.json" >"$TEST_ROOT/manifest.next.json"
mv "$TEST_ROOT/manifest.next.json" "$TEST_ROOT/manifest.json"
: >"$TRACE"
rm -f "$BROADCAST_MARKER"
if run_mode schedule env \
  CAST_SCHEDULE_MISMATCH_TEST=1 \
  BASE_SEPOLIA_SHORT_DELAY_TEST_ONLY=true \
  BASE_SEPOLIA_DEPLOYER_KEYSTORE="$TEST_ROOT/deployer-keystore" \
  BASE_SEPOLIA_DEPLOYER_PASSWORD_FILE="$TEST_ROOT/deployer-password" \
  >"$TEST_ROOT/stdout" 2>"$TEST_ROOT/stderr"; then
  echo "expected mismatched recorded schedule calldata to fail" >&2
  exit 1
fi
grep -F "calldata does not match the manifest" "$TEST_ROOT/stderr" >/dev/null
[[ ! -e "$BROADCAST_MARKER" ]]
! grep -q '^cast send ' "$TRACE"

echo "base Sepolia experiment manifest binding tests passed"
