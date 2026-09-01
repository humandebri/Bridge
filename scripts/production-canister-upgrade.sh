#!/usr/bin/env bash
# Prepare and execute the one-time production controller-bootstrap Wasm upgrade.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
MODE="${1:-}"
shift || true
WASM=""
GATE_A_PROFILE=""
GATE_A_RECEIPT=""
PREFLIGHT=""
OUTPUT=""
CONTROLLER_PEM=""
RECOVERED=false
while [[ "$#" -gt 0 ]]; do
  case "$1" in
    --wasm) WASM="$2"; shift 2 ;;
    --gate-a-profile) GATE_A_PROFILE="$2"; shift 2 ;;
    --gate-a-receipt) GATE_A_RECEIPT="$2"; shift 2 ;;
    --preflight) PREFLIGHT="$2"; shift 2 ;;
    --controller-pem) CONTROLLER_PEM="$2"; shift 2 ;;
    --evidence|--receipt) OUTPUT="$2"; shift 2 ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
done

usage() {
  echo "usage: BRIDGE_ICP_IDENTITY=production $0 preflight --wasm ABS --gate-a-profile ABS --gate-a-receipt ABS --evidence ABS" >&2
  echo "       BRIDGE_ICP_IDENTITY=production $0 execute --wasm ABS --gate-a-profile ABS --gate-a-receipt ABS --preflight ABS --controller-pem ABS --receipt ABS" >&2
  echo "       BRIDGE_ICP_IDENTITY=production $0 recover --wasm ABS --gate-a-profile ABS --gate-a-receipt ABS --preflight ABS --controller-pem ABS --receipt ABS" >&2
  exit 2
}
[[ "$MODE" == preflight || "$MODE" == execute || "$MODE" == recover ]] || usage
[[ "${BRIDGE_ICP_IDENTITY:-}" == production ]] || { echo "production upgrade requires BRIDGE_ICP_IDENTITY=production" >&2; exit 1; }
for path in "$WASM" "$GATE_A_PROFILE" "$GATE_A_RECEIPT"; do
  [[ "$path" == /* && -f "$path" && ! -L "$path" ]] || { echo "upgrade inputs must be absolute regular files" >&2; exit 1; }
done
[[ "$OUTPUT" == /* && ! -e "$OUTPUT" && ! -L "$OUTPUT" && -d "$(dirname "$OUTPUT")" ]] || {
  echo "upgrade output must be a new absolute file in an existing directory" >&2; exit 1;
}
if [[ "$MODE" == execute || "$MODE" == recover ]]; then
  [[ "$PREFLIGHT" == /* && -f "$PREFLIGHT" && ! -L "$PREFLIGHT" ]] || {
    echo "execute requires an absolute preflight evidence file" >&2; exit 1;
  }
  [[ "$CONTROLLER_PEM" == /* && -f "$CONTROLLER_PEM" && ! -L "$CONTROLLER_PEM" ]] || {
    echo "execute requires an absolute production controller PEM file" >&2; exit 1;
  }
fi
for tool in cargo git icp python3 shasum; do command -v "$tool" >/dev/null || { echo "$tool is required" >&2; exit 1; }; done
[[ -z "$(git -C "$ROOT" status --porcelain=v1 --untracked-files=all --ignore-submodules=none)" ]] || {
  echo "production upgrade requires a clean source tree" >&2; exit 1;
}
SOURCE_REVISION="$(git -C "$ROOT" rev-parse HEAD)"
SOURCE_TREE="$(git -C "$ROOT" archive HEAD | shasum -a 256 | awk '{print tolower($1)}')"
WASM_SHA256="$(shasum -a 256 "$WASM" | awk '{print tolower($1)}')"
read -r CANISTER OLD_WASM INSTALLER RECEIPT_SOURCE IC_HOST < <(python3 -I -S - "$GATE_A_PROFILE" "$GATE_A_RECEIPT" <<'PY'
import json,sys
profile=json.load(open(sys.argv[1],encoding='utf-8')); receipt=json.load(open(sys.argv[2],encoding='utf-8'))
install=receipt.get('canister_install',{})
expected=(profile.get('bridge_canister_id'),profile.get('bridge_canister_wasm_sha256'))
actual=(install.get('canister_id'),receipt.get('bridge_canister_wasm_sha256'))
if expected != actual: raise SystemExit('Gate A profile and receipt identity differ')
print(expected[0],expected[1],install.get('installer_principal',''),receipt.get('source_revision',''),profile.get('ic_host',''))
PY
)
[[ "$CANISTER" == "lb5i5-ziaaa-aaaar-qcgwq-cai" && "$OLD_WASM" =~ ^[0-9a-fA-F]{64}$ \
  && "$INSTALLER" =~ ^[a-z0-9-]+$ && "$RECEIPT_SOURCE" =~ ^[0-9a-f]{40}$ && "$IC_HOST" == https://* ]] || {
  echo "Gate A upgrade identity is malformed" >&2; exit 1;
}
[[ "$WASM_SHA256" != "$(printf '%s' "$OLD_WASM" | tr '[:upper:]' '[:lower:]')" ]] || {
  echo "controller-bootstrap upgrade must change the Gate A Wasm" >&2; exit 1;
}
git -C "$ROOT" merge-base --is-ancestor "$RECEIPT_SOURCE" "$SOURCE_REVISION" || {
  echo "upgrade source is not descended from the Gate A source" >&2; exit 1;
}
EXECUTING_PRINCIPAL="$(icp identity principal --identity production)"
[[ "$EXECUTING_PRINCIPAL" == "$INSTALLER" ]] || { echo "production identity is not the Gate A installer" >&2; exit 1; }
DID="$ROOT/canister/bridge-canister/bridge.did"
PROFILE_TARGET="$(mktemp -d "${TMPDIR:-/tmp}/bridge-upgrade-profile-target.XXXXXX")"
trap 'rm -rf "$PROFILE_TARGET"' EXIT
CARGO_TARGET_DIR="$PROFILE_TARGET" cargo build --quiet --locked --manifest-path "$ROOT/Cargo.toml" -p bridge-profile
PROFILE_BIN="$PROFILE_TARGET/debug/bridge-profile"

status_fields() {
  python3 -I -S - "$1" <<'PY'
import json,re,sys
value=json.loads(sys.argv[1])
def named(node,name):
 out=[]
 if isinstance(node,dict):
  for key,child in node.items():
   if key==name: out.append(child)
   out += named(child,name)
 elif isinstance(node,list):
  for child in node: out += named(child,name)
 return out
def module(node):
 if isinstance(node,str) and re.fullmatch(r'(?:0x)?[0-9a-fA-F]{64}',node): return node.removeprefix('0x').lower()
 if isinstance(node,list) and len(node)==32 and all(type(v) is int and 0<=v<=255 for v in node): return bytes(node).hex()
 if isinstance(node,dict) and len(node)==1: return module(next(iter(node.values())))
 return None
controllers=named(value,'controllers'); modules=named(value,'module_hash'); versions=named(value,'canister_version')
if len(controllers)!=1 or not isinstance(controllers[0],list) or len(modules)!=1 or len(versions)!=1: raise SystemExit('ambiguous management status')
resolved=module(modules[0])
if not resolved: raise SystemExit('invalid management module hash')
version=str(versions[0]).replace('_','')
if not version.isdigit(): raise SystemExit('invalid canister version')
print(','.join(sorted(str(v) for v in controllers[0])),resolved,version)
PY
}

query_hex() {
  icp canister call "$CANISTER" "$1" '()' -n ic --query --identity production \
    --candid "$DID" --output hex
}

snapshot() {
  local prefix="$1" status controllers module version
  status="$(icp canister status "$CANISTER" -n ic --json --identity production)"
  read -r controllers module version < <(status_fields "$status")
  [[ "$controllers" == "$INSTALLER" ]] || { echo "production Canister is not controlled solely by the installer" >&2; return 1; }
  printf -v "${prefix}_MANAGEMENT" '%s' "$status"
  printf -v "${prefix}_MODULE" '%s' "$module"
  printf -v "${prefix}_VERSION" '%s' "$version"
  printf -v "${prefix}_BRIDGE_STATUS" '%s' "$(query_hex get_bridge_status)"
  printf -v "${prefix}_LIFECYCLE" '%s' "$(query_hex get_production_lifecycle)"
  printf -v "${prefix}_RUNTIME" '%s' "$(query_hex get_runtime_binding)"
  printf -v "${prefix}_INTEGRITY" '%s' "$(query_hex storage_integrity_check)"
}

write_json() {
  local target="$1" kind="$2" stdout_file="${3:-}" stderr_file="${4:-}" request_id="${5:-}"
  TARGET="$target" KIND="$kind" SOURCE_REVISION="$SOURCE_REVISION" SOURCE_TREE="$SOURCE_TREE" \
  CANISTER="$CANISTER" INSTALLER="$INSTALLER" OLD_WASM="$OLD_WASM" WASM_SHA256="$WASM_SHA256" \
  EXECUTING_PRINCIPAL="$EXECUTING_PRINCIPAL" BEFORE_MANAGEMENT="$BEFORE_MANAGEMENT" BEFORE_MODULE="$BEFORE_MODULE" \
  BEFORE_BRIDGE_STATUS="$BEFORE_BRIDGE_STATUS" BEFORE_LIFECYCLE="$BEFORE_LIFECYCLE" \
  BEFORE_RUNTIME="$BEFORE_RUNTIME" BEFORE_INTEGRITY="$BEFORE_INTEGRITY" \
  BEFORE_VERSION="$BEFORE_VERSION" BEFORE_PUBLIC_STATE="$BEFORE_PUBLIC_STATE" \
  AFTER_MANAGEMENT="${AFTER_MANAGEMENT:-}" AFTER_MODULE="${AFTER_MODULE:-}" \
  AFTER_BRIDGE_STATUS="${AFTER_BRIDGE_STATUS:-}" AFTER_LIFECYCLE="${AFTER_LIFECYCLE:-}" \
  AFTER_RUNTIME="${AFTER_RUNTIME:-}" AFTER_INTEGRITY="${AFTER_INTEGRITY:-}" \
  AFTER_VERSION="${AFTER_VERSION:-}" AFTER_PUBLIC_STATE="${AFTER_PUBLIC_STATE:-}" IC_HOST="$IC_HOST" \
  RESPONSE_STDOUT_FILE="$stdout_file" RESPONSE_STDERR_FILE="$stderr_file" \
  SUBMISSION_FILE="${SUBMISSION_FILE:-}" REQUEST_ID="$request_id" RECOVERED="${RECOVERED:-false}" \
  python3 -I -S - <<'PY'
import hashlib,json,os,time
def h(value): return hashlib.sha256(value).hexdigest()
def hx(value): return value.hex()
def raw(name): return os.environ[name].encode()
def candid(name): return bytes.fromhex(os.environ[name].strip())
before=[candid('BEFORE_BRIDGE_STATUS'),candid('BEFORE_LIFECYCLE'),candid('BEFORE_RUNTIME'),candid('BEFORE_INTEGRITY')]
value={'schema_version':1,'kind':os.environ['KIND'],'source_revision':os.environ['SOURCE_REVISION'],
 'source_tree_sha256':os.environ['SOURCE_TREE'],'bridge_canister_id':os.environ['CANISTER'],
 'install_mode':'upgrade','executing_principal':os.environ['EXECUTING_PRINCIPAL'],
 'wasm_sha256':os.environ['WASM_SHA256'],'before_module_sha256':os.environ['BEFORE_MODULE'],
 'before_canister_version':int(os.environ['BEFORE_VERSION']),
 'before_controllers':[os.environ['INSTALLER']],'before_schema_version':35,'before_lifecycle':'Bootstrap',
 'before_deposits_paused':True,'before_storage_validation_complete':True,
 'before_management_status_json_hex':hx(raw('BEFORE_MANAGEMENT')),
 'before_management_status_json_sha256':h(raw('BEFORE_MANAGEMENT')),
 'before_bridge_status_response_hex':hx(before[0]),'before_bridge_status_response_sha256':h(before[0]),
 'before_lifecycle_response_hex':hx(before[1]),'before_lifecycle_response_sha256':h(before[1]),
 'before_runtime_binding_response_hex':hx(before[2]),'before_runtime_binding_response_sha256':h(before[2]),
 'before_storage_integrity_response_hex':hx(before[3]),'before_storage_integrity_response_sha256':h(before[3]),
 'before_public_state_sha256':os.environ['BEFORE_PUBLIC_STATE'],'observed_at_unix':int(time.time())}
if os.environ['KIND']=='production-controller-bootstrap-upgrade':
 value.pop('observed_at_unix',None)
 after=[candid('AFTER_BRIDGE_STATUS'),candid('AFTER_LIFECYCLE'),candid('AFTER_RUNTIME'),candid('AFTER_INTEGRITY')]
 stdout=open(os.environ['RESPONSE_STDOUT_FILE'],'rb').read(); stderr=open(os.environ['RESPONSE_STDERR_FILE'],'rb').read()
 submission=open(os.environ['SUBMISSION_FILE'],'rb').read()
 now=int(time.time()); recovered=os.environ['RECOVERED']=='true'
 value.update({'executed_at_unix':int(os.environ['EXECUTED_AT']),'verified_at_unix':now,
  'recovered':recovered,'recovered_at_unix':now if recovered else None,
  'after_controllers':[os.environ['INSTALLER']],'after_module_sha256':os.environ['AFTER_MODULE'],
  'after_canister_version':int(os.environ['AFTER_VERSION']),
  'after_schema_version':35,'after_lifecycle':'Bootstrap','after_deposits_paused':True,
  'after_storage_validation_complete':True,'after_management_status_json_hex':hx(raw('AFTER_MANAGEMENT')),
  'after_management_status_json_sha256':h(raw('AFTER_MANAGEMENT')),
  'after_bridge_status_response_hex':hx(after[0]),'after_bridge_status_response_sha256':h(after[0]),
  'after_lifecycle_response_hex':hx(after[1]),'after_lifecycle_response_sha256':h(after[1]),
  'after_runtime_binding_response_hex':hx(after[2]),'after_runtime_binding_response_sha256':h(after[2]),
  'after_storage_integrity_response_hex':hx(after[3]),'after_storage_integrity_response_sha256':h(after[3]),
  'after_public_state_sha256':os.environ['AFTER_PUBLIC_STATE'],'command_argv':['bridge-profile','submit-production-canister-upgrade',os.environ['IC_HOST'],os.environ['CANISTER'],os.environ['INSTALLER'],'<production-controller-pem>','<verified-release-artifact>','<durable-submission-artifact>','<durable-response-artifact>'],
  'submission_json_hex':hx(submission),'submission_json_sha256':h(submission),
  'request_id':os.environ['REQUEST_ID'],'response_stdout_hex':hx(stdout),'response_stdout_sha256':h(stdout),
  'response_stderr_hex':hx(stderr),'response_stderr_sha256':h(stderr)})
target=os.environ['TARGET']; parent=os.path.dirname(target)
fd=os.open(target,os.O_WRONLY|os.O_CREAT|os.O_EXCL,0o600)
with os.fdopen(fd,'w') as output: json.dump(value,output,sort_keys=True,separators=(',',':')); output.write('\n'); output.flush(); os.fsync(output.fileno())
fd=os.open(parent,os.O_RDONLY|os.O_DIRECTORY); os.fsync(fd); os.close(fd)
PY
}

preflight_value() {
  python3 -I -S - "$PREFLIGHT" "$1" "${2:-text}" <<'PY'
import json,sys
value=json.load(open(sys.argv[1],encoding='utf-8'))[sys.argv[2]]
if sys.argv[3]=='hex': value=bytes.fromhex(value).decode()
print(value,end='')
PY
}

if [[ "$MODE" == recover ]]; then
  STDOUT_FILE="$OUTPUT.stdout"
  STDERR_FILE="$OUTPUT.stderr"
  SUBMISSION_FILE="$OUTPUT.submission.json"
  EXECUTION_FILE="$OUTPUT.execution.json"
  for sidecar in "$STDOUT_FILE" "$STDERR_FILE" "$SUBMISSION_FILE" "$EXECUTION_FILE"; do
    [[ -f "$sidecar" && ! -L "$sidecar" ]] || { echo "recovery sidecar is missing or unsafe: $sidecar" >&2; exit 1; }
  done
  BEFORE_MANAGEMENT="$(preflight_value before_management_status_json_hex hex)"
  BEFORE_MODULE="$(preflight_value before_module_sha256)"
  BEFORE_VERSION="$(preflight_value before_canister_version)"
  BEFORE_BRIDGE_STATUS="$(preflight_value before_bridge_status_response_hex)"
  BEFORE_LIFECYCLE="$(preflight_value before_lifecycle_response_hex)"
  BEFORE_RUNTIME="$(preflight_value before_runtime_binding_response_hex)"
  BEFORE_INTEGRITY="$(preflight_value before_storage_integrity_response_hex)"
  BEFORE_PUBLIC_STATE="$(preflight_value before_public_state_sha256)"
  read -r RECORDED_CONTROLLERS RECORDED_MODULE RECORDED_VERSION < <(status_fields "$BEFORE_MANAGEMENT")
  [[ "$RECORDED_CONTROLLERS" == "$INSTALLER" && "$RECORDED_MODULE" == "$OLD_WASM" \
    && "$RECORDED_VERSION" == "$BEFORE_VERSION" ]] || {
    echo "reviewed preflight does not bind the sole controller and immutable Gate A Wasm" >&2; exit 1;
  }
  python3 -I -S - "$PREFLIGHT" "$SOURCE_REVISION" "$SOURCE_TREE" "$CANISTER" "$OLD_WASM" "$WASM_SHA256" "$INSTALLER" <<'PY'
import json,sys
p=json.load(open(sys.argv[1],encoding='utf-8'))
actual=[p.get('source_revision'),p.get('source_tree_sha256'),p.get('bridge_canister_id'),
 p.get('before_module_sha256','').lower(),p.get('wasm_sha256'),p.get('executing_principal')]
if actual != [sys.argv[2],sys.argv[3],sys.argv[4],sys.argv[5].lower(),sys.argv[6],sys.argv[7]]:
 raise SystemExit('recovery preflight identity differs from the reviewed source or Gate A lineage')
PY
  read -r EXECUTED_AT EXECUTION_VERSION < <(python3 -I -S - "$EXECUTION_FILE" "$SOURCE_REVISION" "$WASM_SHA256" <<'PY'
import json,sys
v=json.load(open(sys.argv[1],encoding='utf-8'))
if v.get('schema_version')!=1 or v.get('source_revision')!=sys.argv[2] or v.get('wasm_sha256')!=sys.argv[3]:
 raise SystemExit('execution marker differs from the reviewed upgrade')
print(v.get('executed_at_unix'),v.get('before_canister_version'))
PY
  )
  [[ "$EXECUTION_VERSION" == "$BEFORE_VERSION" && "$EXECUTED_AT" =~ ^[0-9]+$ ]] || {
    echo "execution marker does not bind the reviewed before version" >&2; exit 1;
  }
  REQUEST_ID="$($PROFILE_BIN verify-production-upgrade-submission \
    "$IC_HOST" "$CANISTER" "$INSTALLER" "$WASM" "$SUBMISSION_FILE")"
  python3 -I -S - "$STDOUT_FILE" "$REQUEST_ID" <<'PY'
import re,sys
found=re.findall(r'(?im)^request_id=([0-9a-f]{64})$',open(sys.argv[1],errors='replace').read())
if set(v.lower() for v in found)!={sys.argv[2].lower()}: raise SystemExit('stdout does not bind the signed request ID')
PY
  snapshot AFTER
  [[ "$AFTER_MODULE" == "$WASM_SHA256" && "$AFTER_VERSION" -eq "$((BEFORE_VERSION + 1))" ]] || {
    echo "recovery requires the exact reviewed Wasm and one Canister version increment" >&2; exit 1;
  }
  AFTER_PUBLIC_STATE="$($PROFILE_BIN verify-production-upgrade-state-preserved \
    "$BEFORE_BRIDGE_STATUS" "$BEFORE_LIFECYCLE" "$BEFORE_RUNTIME" "$BEFORE_INTEGRITY" \
    "$AFTER_BRIDGE_STATUS" "$AFTER_LIFECYCLE" "$AFTER_RUNTIME" "$AFTER_INTEGRITY")"
  export EXECUTED_AT
  RECOVERED=true
  write_json "$OUTPUT" production-controller-bootstrap-upgrade "$STDOUT_FILE" "$STDERR_FILE" "$REQUEST_ID"
  echo "production controller-bootstrap upgrade receipt recovered without sending an update: $OUTPUT"
  exit 0
fi

snapshot BEFORE
[[ "$BEFORE_MODULE" == "$(printf '%s' "$OLD_WASM" | tr '[:upper:]' '[:lower:]')" ]] || {
  echo "live module does not match the immutable Gate A profile" >&2; exit 1;
}
BEFORE_PUBLIC_STATE="$($PROFILE_BIN production-upgrade-public-state-sha256 \
  "$BEFORE_BRIDGE_STATUS" "$BEFORE_LIFECYCLE" "$BEFORE_RUNTIME" "$BEFORE_INTEGRITY")"

if [[ "$MODE" == preflight ]]; then
  write_json "$OUTPUT" production-controller-bootstrap-upgrade-preflight
  echo "production controller-bootstrap upgrade preflight passed: $OUTPUT"
  exit 0
fi

python3 -I -S - "$PREFLIGHT" "$SOURCE_REVISION" "$SOURCE_TREE" "$CANISTER" "$OLD_WASM" "$WASM_SHA256" \
  "$BEFORE_MODULE" "$BEFORE_BRIDGE_STATUS" "$BEFORE_LIFECYCLE" "$BEFORE_RUNTIME" "$BEFORE_INTEGRITY" <<'PY'
import json,sys
p=json.load(open(sys.argv[1],encoding='utf-8'))
expected=[sys.argv[2],sys.argv[3],sys.argv[4],sys.argv[5].lower(),sys.argv[6],sys.argv[7],*sys.argv[8:12]]
actual=[p.get('source_revision'),p.get('source_tree_sha256'),p.get('bridge_canister_id'),p.get('before_module_sha256','').lower(),p.get('wasm_sha256'),sys.argv[7],p.get('before_bridge_status_response_hex'),p.get('before_lifecycle_response_hex'),p.get('before_runtime_binding_response_hex'),p.get('before_storage_integrity_response_hex')]
if actual!=expected: raise SystemExit('live state or source differs from the reviewed production upgrade preflight')
PY

STDOUT_FILE="$OUTPUT.stdout"
STDERR_FILE="$OUTPUT.stderr"
SUBMISSION_FILE="$OUTPUT.submission.json"
EXECUTION_FILE="$OUTPUT.execution.json"
for sidecar in "$STDOUT_FILE" "$STDERR_FILE" "$SUBMISSION_FILE" "$EXECUTION_FILE"; do
  [[ ! -e "$sidecar" && ! -L "$sidecar" ]] || { echo "upgrade sidecar already exists: $sidecar" >&2; exit 1; }
done
EXECUTED_AT="$(date +%s)"
export EXECUTED_AT
TARGET="$EXECUTION_FILE" EXECUTED_AT="$EXECUTED_AT" SOURCE_REVISION="$SOURCE_REVISION" \
WASM_SHA256="$WASM_SHA256" BEFORE_VERSION="$BEFORE_VERSION" python3 -I -S - <<'PY'
import json,os
value={'schema_version':1,'executed_at_unix':int(os.environ['EXECUTED_AT']),
 'source_revision':os.environ['SOURCE_REVISION'],'wasm_sha256':os.environ['WASM_SHA256'],
 'before_canister_version':int(os.environ['BEFORE_VERSION'])}
fd=os.open(os.environ['TARGET'],os.O_WRONLY|os.O_CREAT|os.O_EXCL,0o400)
with os.fdopen(fd,'w') as f: json.dump(value,f,sort_keys=True,separators=(',',':')); f.write('\n'); f.flush(); os.fsync(f.fileno())
fd=os.open(os.path.dirname(os.environ['TARGET']),os.O_RDONLY|os.O_DIRECTORY); os.fsync(fd); os.close(fd)
PY
set +e
"$PROFILE_BIN" submit-production-canister-upgrade "$IC_HOST" "$CANISTER" "$INSTALLER" \
  "$CONTROLLER_PEM" "$WASM" "$SUBMISSION_FILE" "$STDOUT_FILE" \
  >/dev/null 2>"$STDERR_FILE"
STATUS=$?
set -e
[[ "$STATUS" -eq 0 ]] || { echo "production upgrade result is unknown; preserve the preflight and inspect live state" >&2; exit "$STATUS"; }
REQUEST_ID="$(python3 -I -S - "$STDOUT_FILE" "$STDERR_FILE" <<'PY'
import re,sys
value=open(sys.argv[1],errors='replace').read()
found=re.findall(r'(?im)^request_id=([0-9a-f]{64})$',value)
if len(set(v.lower() for v in found))!=1: raise SystemExit('upgrade response has no unique request ID')
print(found[0].lower())
PY
)" || { echo "successful upgrade response did not expose a unique request ID; preserve live state" >&2; exit 1; }
snapshot AFTER
[[ "$AFTER_MODULE" == "$WASM_SHA256" ]] || { echo "post-upgrade module hash differs from the reviewed Wasm" >&2; exit 1; }
[[ "$AFTER_VERSION" -eq "$((BEFORE_VERSION + 1))" ]] || { echo "production Canister version did not advance exactly once" >&2; exit 1; }
AFTER_PUBLIC_STATE="$($PROFILE_BIN verify-production-upgrade-state-preserved \
  "$BEFORE_BRIDGE_STATUS" "$BEFORE_LIFECYCLE" "$BEFORE_RUNTIME" "$BEFORE_INTEGRITY" \
  "$AFTER_BRIDGE_STATUS" "$AFTER_LIFECYCLE" "$AFTER_RUNTIME" "$AFTER_INTEGRITY")"
write_json "$OUTPUT" production-controller-bootstrap-upgrade "$STDOUT_FILE" "$STDERR_FILE" "$REQUEST_ID"
echo "production controller-bootstrap upgrade verified: $OUTPUT"
