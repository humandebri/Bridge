#!/usr/bin/env bash
# Prepare and submit the native Kinic SNS registration and same-Wasm upgrade proposals.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# shellcheck source=production-validation.sh
source "$ROOT/scripts/production-validation.sh"

MODE="${1:-}"
shift || true
WASM=""
EXPECTED_CURRENT_WASM=""
REGISTRATION_PROPOSAL_ID=""
UPGRADE_PROPOSAL_ID=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    --wasm) WASM="$2"; shift 2 ;;
    --expected-current-wasm)
      EXPECTED_CURRENT_WASM="$(printf '%s' "$2" | tr '[:upper:]' '[:lower:]')"
      shift 2
      ;;
    --registration-proposal-id) REGISTRATION_PROPOSAL_ID="$2"; shift 2 ;;
    --upgrade-proposal-id) UPGRADE_PROPOSAL_ID="$2"; shift 2 ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
done

usage() {
  cat >&2 <<USAGE
usage: BRIDGE_RELEASE_BUNDLE=ABS BRIDGE_ICP_IDENTITY=production $0 check-registration --wasm ABS
       BRIDGE_RELEASE_BUNDLE=ABS BRIDGE_ICP_IDENTITY=production BRIDGE_CONFIRM_SNS_DAPP_REGISTRATION=REGISTER_PRODUCTION_BRIDGE_WITH_KINIC_SNS $0 execute-registration --wasm ABS --expected-current-wasm SHA256
       BRIDGE_RELEASE_BUNDLE=ABS BRIDGE_ICP_IDENTITY=production $0 check-upgrade --wasm ABS --registration-proposal-id ID
       BRIDGE_RELEASE_BUNDLE=ABS BRIDGE_ICP_IDENTITY=production BRIDGE_CONFIRM_SNS_SAME_WASM_UPGRADE=UPGRADE_REGISTERED_BRIDGE_WITH_SAME_WASM $0 execute-upgrade --wasm ABS --expected-current-wasm SHA256 --registration-proposal-id ID
       BRIDGE_RELEASE_BUNDLE=ABS BRIDGE_ICP_IDENTITY=production $0 verify-upgrade --wasm ABS --registration-proposal-id ID --upgrade-proposal-id ID
USAGE
  exit 2
}
case "$MODE" in
  check-registration|execute-registration|check-upgrade|execute-upgrade|verify-upgrade) ;;
  *) usage ;;
esac

: "${BRIDGE_RELEASE_BUNDLE:?missing reviewed release bundle}"
[[ "${BRIDGE_ICP_IDENTITY:-}" == production ]] || {
  echo "handover validation requires BRIDGE_ICP_IDENTITY=production" >&2; exit 1;
}
PROFILE="$BRIDGE_RELEASE_BUNDLE/profile.json"
for path in "$WASM" "$PROFILE"; do
  [[ "$path" == /* && -f "$path" && ! -L "$path" ]] || {
    echo "handover inputs must be absolute regular files" >&2; exit 1;
  }
done
if [[ "$MODE" == execute-registration ]]; then
  [[ "${BRIDGE_CONFIRM_SNS_DAPP_REGISTRATION:-}" == REGISTER_PRODUCTION_BRIDGE_WITH_KINIC_SNS ]] || {
    echo "registration requires the exact explicit confirmation token" >&2; exit 1;
  }
elif [[ -n "${BRIDGE_CONFIRM_SNS_DAPP_REGISTRATION:-}" ]]; then
  echo "the registration confirmation token is accepted only in execute-registration" >&2; exit 1
fi
if [[ "$MODE" == execute-upgrade ]]; then
  [[ "${BRIDGE_CONFIRM_SNS_SAME_WASM_UPGRADE:-}" == UPGRADE_REGISTERED_BRIDGE_WITH_SAME_WASM ]] || {
    echo "SNS upgrade requires the exact explicit confirmation token" >&2; exit 1;
  }
elif [[ -n "${BRIDGE_CONFIRM_SNS_SAME_WASM_UPGRADE:-}" ]]; then
  echo "the upgrade confirmation token is accepted only in execute-upgrade" >&2; exit 1
fi
if [[ "$MODE" == execute-* ]]; then
  [[ "$EXPECTED_CURRENT_WASM" =~ ^[0-9a-f]{64}$ ]] || {
    echo "execute requires --expected-current-wasm SHA256" >&2; exit 1;
  }
fi
if [[ "$MODE" == *-upgrade ]]; then
  [[ "$REGISTRATION_PROPOSAL_ID" =~ ^[1-9][0-9]*$ ]] || {
    echo "upgrade requires --registration-proposal-id" >&2; exit 1;
  }
fi
if [[ "$MODE" == verify-upgrade ]]; then
  [[ "$UPGRADE_PROPOSAL_ID" =~ ^[1-9][0-9]*$ ]] || {
    echo "verify-upgrade requires --upgrade-proposal-id" >&2; exit 1;
  }
elif [[ -n "$UPGRADE_PROPOSAL_ID" ]]; then
  echo "--upgrade-proposal-id is accepted only in verify-upgrade" >&2; exit 1
fi
for tool in cargo git icp node python3 shasum split; do
  command -v "$tool" >/dev/null || { echo "$tool is required" >&2; exit 1; }
done

readonly GOVERNANCE=74ncn-fqaaa-aaaaq-aaasa-cai
readonly SNS_ROOT=7jkta-eyaaa-aaaaq-aaarq-cai
readonly PRODUCTION_CONTROLLER=lqfvd-m7ihy-e5dvc-gngvr-blzbt-pupeq-6t7ua-r7v4p-bvqjw-ea7gl-4qe
readonly PROPOSER_IDENTITY=llm-wiki-mainnet
readonly PROPOSER_PRINCIPAL=r75h6-lqd7b-5jack-at55d-vvti2-lg5qy-ly73a-5ezve-odnkc-kagu3-nae
# Whitespace-normalized reviewed snapshot of the Governance canister's candid:service metadata.
readonly GOVERNANCE_CANDID="$ROOT/scripts/candid/kinic-sns-governance.did"
readonly GOVERNANCE_CANDID_SHA256=5550f148fb63467b94f25f3b5e05db5fbb1168ebf65e5f70c3c0e192e1aeea27

[[ -f "$GOVERNANCE_CANDID" && ! -L "$GOVERNANCE_CANDID" ]] || {
  echo "reviewed SNS Governance Candid is unavailable" >&2; exit 1;
}
[[ "$(shasum -a 256 "$GOVERNANCE_CANDID" | awk '{print tolower($1)}')" == "$GOVERNANCE_CANDID_SHA256" ]] || {
  echo "SNS Governance Candid differs from the reviewed interface" >&2; exit 1;
}

production_require_clean_source "$ROOT"
REVISION="$(git -C "$ROOT" rev-parse HEAD)"
TREE="$(git -C "$ROOT" archive HEAD | shasum -a 256 | awk '{print tolower($1)}')"
require_source_identity() {
  production_require_clean_source "$ROOT" \
    && [[ "$(git -C "$ROOT" rev-parse HEAD)" == "$REVISION" ]] \
    && [[ "$(git -C "$ROOT" archive HEAD | shasum -a 256 | awk '{print tolower($1)}')" == "$TREE" ]] || {
      echo "source changed during SNS handover validation" >&2
      return 1
    }
}

TMP="$(mktemp -d "${TMPDIR:-/tmp}/bridge-sns-handover.XXXXXX")"
cleanup() { chmod -R u+w "$TMP" 2>/dev/null || true; rm -rf "$TMP"; }
trap cleanup EXIT
python3 -I -S - "$WASM" "$TMP/candidate.wasm" <<'PY'
import os,stat,sys
source,target=sys.argv[1:]
fd=os.open(source,os.O_RDONLY|getattr(os,'O_NOFOLLOW',0))
try:
 before=os.fstat(fd)
 if not stat.S_ISREG(before.st_mode) or before.st_size>128*1024*1024: raise SystemExit('unsafe Wasm input')
 out=os.open(target,os.O_WRONLY|os.O_CREAT|os.O_EXCL,0o400)
 try:
  while True:
   chunk=os.read(fd,1024*1024)
   if not chunk: break
   os.write(out,chunk)
  os.fsync(out)
 finally: os.close(out)
 after=os.fstat(fd)
 if (before.st_dev,before.st_ino,before.st_size,before.st_mtime_ns,before.st_ctime_ns)!=(after.st_dev,after.st_ino,after.st_size,after.st_mtime_ns,after.st_ctime_ns):
  raise SystemExit('Wasm changed while being read')
finally: os.close(fd)
PY
WASM="$TMP/candidate.wasm"
CANDIDATE_WASM="$(shasum -a 256 "$WASM" | awk '{print tolower($1)}')"
read -r CANISTER PROFILE_CONTROLLER PROFILE_ROOT HOST < <(python3 -I -S - "$PROFILE" <<'PY'
import json,sys
p=json.load(open(sys.argv[1]))
print(p['bridge_canister_id'],p['pause_principal'],p['root_canister_id'],p['ic_host'])
PY
)
[[ "$CANISTER" == lb5i5-ziaaa-aaaar-qcgwq-cai \
  && "$PROFILE_CONTROLLER" == "$PRODUCTION_CONTROLLER" \
  && "$PROFILE_ROOT" == "$SNS_ROOT" \
  && "$HOST" == https://icp-api.io ]] || {
  echo "release profile differs from the fixed Kinic production domain" >&2; exit 1;
}
[[ "$(icp identity principal --identity production)" == "$PRODUCTION_CONTROLLER" ]] || {
  echo "production identity differs from the fixed controller" >&2; exit 1;
}
[[ "$(icp identity principal --identity "$PROPOSER_IDENTITY")" == "$PROPOSER_PRINCIPAL" ]] || {
  echo "SNS proposer identity differs from the reviewed signer" >&2; exit 1;
}

NEURON_ARG='(record { neuron_id = opt record { id = blob "\5e\0f\2f\10\3a\68\88\29\ee\f9\c9\6b\f7\f8\31\5e\d4\61\03\7c\23\47\d5\5f\50\80\a3\67\b7\1c\1f\60" } })'
NEURON_RESPONSE="$(icp canister call --network ic --identity "$PROPOSER_IDENTITY" \
  --candid "$GOVERNANCE_CANDID" "$GOVERNANCE" get_neuron "$NEURON_ARG" --query)"
printf '%s' "$NEURON_RESPONSE" | python3 -I -S -c '
import re,sys
principal=sys.argv[1]; text=sys.stdin.read(); at=text.find(principal)
if at<0: raise SystemExit("reviewed signer has no permission on proposer neuron")
block=text[at:at+500]
if not re.search(r"permission_type\s*=\s*vec\s*\{[^}]*\b3\s*:\s*int32",block,re.S) or not re.search(r"permission_type\s*=\s*vec\s*\{[^}]*\b4\s*:\s*int32",block,re.S):
 raise SystemExit("reviewed signer lacks SubmitProposal/Vote permissions")
' "$PROPOSER_PRINCIPAL"

CARGO_TARGET_DIR="$TMP/profile" cargo build --quiet --locked --release --manifest-path "$ROOT/Cargo.toml" -p bridge-profile
PROFILE_BIN="$TMP/profile/release/bridge-profile"
if [[ "$MODE" == *-registration ]]; then
  CONTROLLER_MODE=joint-unregistered
  VALIDATION_PRINCIPAL="$PRODUCTION_CONTROLLER"
  if [[ -n "$EXPECTED_CURRENT_WASM" ]]; then
    CURRENT_WASM="$EXPECTED_CURRENT_WASM"
  else
    CURRENT_WASM="$(icp canister status "$CANISTER" -n ic --identity production --json | python3 -c '
import json,re,sys
value=json.load(sys.stdin); values=[]
def walk(item):
 if isinstance(item,dict):
  for key,val in item.items():
   if key in ("module_hash","module"): values.append(val)
   walk(val)
 elif isinstance(item,list):
  for val in item: walk(val)
walk(value)
if len(values)!=1: raise SystemExit("ambiguous module hash")
digest=str(values[0]).strip().lower().removeprefix("0x")
if not re.fullmatch(r"[0-9a-f]{64}",digest): raise SystemExit("invalid module hash")
print(digest)')"
  fi
else
  CONTROLLER_MODE=root-registered
  VALIDATION_PRINCIPAL="$SNS_ROOT"
  CURRENT_WASM="${EXPECTED_CURRENT_WASM:-$CANDIDATE_WASM}"
fi
[[ "$CURRENT_WASM" == "$CANDIDATE_WASM" ]] || {
  echo "handover test requires the candidate to be the exact current Wasm" >&2; exit 1;
}
export BRIDGE_PRODUCTION_INSTALLER_IDENTITY=production
"$PROFILE_BIN" verify-production-current-state "$PROFILE" "$VALIDATION_PRINCIPAL" "$CURRENT_WASM" "$CONTROLLER_MODE"
if [[ "$MODE" == *-upgrade ]]; then
  "$PROFILE_BIN" verify-sns-registration-live "$PROFILE" "$CURRENT_WASM" "$REGISTRATION_PROPOSAL_ID"
fi
production_run_proof_gate "$ROOT" "$REVISION" "$TREE"
for index in 1 2; do
  CARGO_NET_OFFLINE=true CARGO_TARGET_DIR="$TMP/repro-$index" \
    icp build bridge-canister -e production --project-root-override "$ROOT" >/dev/null
  BUILT="$TMP/repro-$index/wasm32-unknown-unknown/release/bridge_canister.wasm"
  [[ -f "$BUILT" && "$(shasum -a 256 "$BUILT" | awk '{print tolower($1)}')" == "$CANDIDATE_WASM" ]] || {
    echo "candidate Wasm is not reproducible from current source" >&2; exit 1;
  }
done
require_source_identity
"$PROFILE_BIN" verify-production-current-state "$PROFILE" "$VALIDATION_PRINCIPAL" "$CURRENT_WASM" "$CONTROLLER_MODE"
if [[ "$MODE" == *-upgrade ]]; then
  "$PROFILE_BIN" verify-sns-registration-live "$PROFILE" "$CURRENT_WASM" "$REGISTRATION_PROPOSAL_ID"
fi

if [[ "$MODE" == verify-upgrade ]]; then
  "$PROFILE_BIN" verify-sns-upgrade-live \
    "$PROFILE" "$CURRENT_WASM" "$WASM" "$REGISTRATION_PROPOSAL_ID" "$UPGRADE_PROPOSAL_ID"
  exit 0
fi

if [[ "$MODE" == *-registration ]]; then
  PROPOSAL="$(node "$ROOT/tools/sns-proposal/handover.mjs" registration-payload "$CANISTER")"
  KIND=registration
else
  PROPOSAL="$(node "$ROOT/tools/sns-proposal/handover.mjs" upgrade-payload "$CANISTER" "$WASM" "$CANDIDATE_WASM")"
  KIND=upgrade
fi
PROPOSAL_SHA256="$(printf '%s' "$PROPOSAL" | shasum -a 256 | awk '{print tolower($1)}')"
printf 'sns_handover_check=pass kind=%s current_module_sha256=%s candidate_module_sha256=%s source_revision=%s proposal_sha256=%s\n' \
  "$KIND" "$CURRENT_WASM" "$CANDIDATE_WASM" "$REVISION" "$PROPOSAL_SHA256"
printf '%s\n' "$PROPOSAL"
[[ "$MODE" == check-* ]] && exit 0

upload_chunks() {
  mkdir -m 700 "$TMP/chunks"
  split -b 1000000 -d -a 3 "$WASM" "$TMP/chunks/chunk-"
  icp canister call --network ic --identity production aaaaa-aa clear_chunk_store \
    "(record { canister_id = principal \"$CANISTER\" })" --json >"$TMP/clear.json"
  : >"$TMP/expected-chunks"
  for chunk in "$TMP"/chunks/chunk-*; do
    digest="$(shasum -a 256 "$chunk" | awk '{print tolower($1)}')"
    printf '%s\n' "$digest" >>"$TMP/expected-chunks"
    python3 -I -S - "$CANISTER" "$chunk" "$TMP/upload.did" <<'PY'
import os,sys
canister,source,target=sys.argv[1:]
data=open(source,'rb').read()
body='(record { canister_id = principal "'+canister+'"; chunk = blob "'+''.join(f'\\{byte:02x}' for byte in data)+'" })\n'
fd=os.open(target,os.O_WRONLY|os.O_CREAT|os.O_TRUNC,0o600)
try: os.write(fd,body.encode())
finally: os.close(fd)
PY
    icp canister call --network ic --identity production --args-file "$TMP/upload.did" \
      aaaaa-aa upload_chunk --json >"$TMP/upload.json"
    observed="$(node "$ROOT/tools/sns-proposal/handover.mjs" decode-chunk "$TMP/upload.json")"
    [[ "$observed" == "$digest" ]] || { echo "uploaded chunk hash differs" >&2; return 1; }
  done
  icp canister call --network ic --identity production aaaaa-aa stored_chunks \
    "(record { canister_id = principal \"$CANISTER\" })" --json >"$TMP/stored.json"
  node "$ROOT/tools/sns-proposal/handover.mjs" decode-stored-chunks "$TMP/stored.json" >"$TMP/stored-hashes.json"
  python3 -I -S - "$TMP/expected-chunks" "$TMP/stored-hashes.json" <<'PY'
import json,sys
expected=sorted(line.strip() for line in open(sys.argv[1]) if line.strip())
observed=json.load(open(sys.argv[2]))
if observed!=expected: raise SystemExit('stored chunk set differs from the exact candidate chunks')
print(f'sns_handover_chunks=verified count={len(expected)}')
PY
}

if [[ "$MODE" == execute-registration ]]; then
  upload_chunks
  require_source_identity
  "$PROFILE_BIN" verify-production-current-state "$PROFILE" "$PRODUCTION_CONTROLLER" "$CURRENT_WASM" joint-unregistered
fi

SUBACCOUNT_BLOB='blob "\5e\0f\2f\10\3a\68\88\29\ee\f9\c9\6b\f7\f8\31\5e\d4\61\03\7c\23\47\d5\5f\50\80\a3\67\b7\1c\1f\60"'
MANAGE_ARG="(record { subaccount = $SUBACCOUNT_BLOB; command = opt variant { MakeProposal = $PROPOSAL } })"
set +e
icp canister call --network ic --identity "$PROPOSER_IDENTITY" --candid "$GOVERNANCE_CANDID" \
  "$GOVERNANCE" manage_neuron "$MANAGE_ARG" --json \
  >"$TMP/proposal-response.json" 2>"$TMP/proposal-response.stderr"
SUBMIT_STATUS=$?
set -e
if [[ $SUBMIT_STATUS -ne 0 ]]; then
  sed -n '1,120p' "$TMP/proposal-response.stderr" >&2
  echo "proposal submission outcome is uncertain; do not resubmit for 6 minutes, then search Governance for the exact proposer and proposal SHA-256 $PROPOSAL_SHA256" >&2
  exit 1
fi
PROPOSAL_ID="$(node "$ROOT/tools/sns-proposal/handover.mjs" decode-response "$TMP/proposal-response.json")" || {
  echo "proposal ID is unavailable; do not resubmit for 6 minutes, then search Governance for the exact proposal" >&2
  exit 1
}
READBACK_ARG="(record { proposal_id = opt record { id = $PROPOSAL_ID : nat64 } })"
icp canister call --network ic --identity "$PROPOSER_IDENTITY" --candid "$GOVERNANCE_CANDID" \
  "$GOVERNANCE" get_proposal "$READBACK_ARG" --query \
  >"$TMP/proposal-readback.txt"
cat "$TMP/proposal-readback.txt"
printf 'sns_handover_proposal=submitted kind=%s proposal_id=%s proposal_sha256=%s\n' \
  "$KIND" "$PROPOSAL_ID" "$PROPOSAL_SHA256"
