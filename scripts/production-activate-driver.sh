#!/usr/bin/env bash
# Role-separated initial controller activation entrypoint. The Canister signs the EVM transaction.
set -Eeuo pipefail

SOURCE_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
# shellcheck source=production-validation.sh
source "$SOURCE_ROOT/scripts/production-validation.sh"

: "${BRIDGE_GATE_B_MANIFEST_SHA256:?missing Gate B evidence hash}"
: "${BRIDGE_RELEASE_BUNDLE:?missing release bundle}"
: "${BRIDGE_ACTIVATION_PHASE:?set BRIDGE_ACTIVATION_PHASE=schedule or execute}"
: "${BRIDGE_ACTIVATION_STEP:?set BRIDGE_ACTIVATION_STEP=prepare, replace, relay, or confirm}"
: "${BRIDGE_ACTIVATION_ARTIFACT:?missing fixed activation artifact path}"
: "${BRIDGE_OPERATIONAL_CONFIG_SEAL_RECEIPT:?missing operational config seal receipt}"

[[ "$BRIDGE_ACTIVATION_PHASE" == schedule || "$BRIDGE_ACTIVATION_PHASE" == execute ]] || {
  echo "invalid activation phase" >&2
  exit 1
}
EXPECTED_CONFIRMATION=SCHEDULE_PRODUCTION_ASSET_ACTIVATION
if [[ "$BRIDGE_ACTIVATION_PHASE" == execute ]]; then
  EXPECTED_CONFIRMATION=UNPAUSE_PRODUCTION_ASSET_ACCEPTANCE
fi
[[ "${BRIDGE_CONFIRM_ASSET_ACCEPTANCE:-}" == "$EXPECTED_CONFIRMATION" ]] || {
  echo "activation requires the phase-specific exact explicit confirmation token" >&2
  exit 1
}
[[ "$BRIDGE_ACTIVATION_STEP" == prepare || "$BRIDGE_ACTIVATION_STEP" == replace || "$BRIDGE_ACTIVATION_STEP" == relay || "$BRIDGE_ACTIVATION_STEP" == confirm ]] || {
  echo "invalid activation step" >&2
  exit 1
}
command -v python3 >/dev/null || { echo "python3 is required" >&2; exit 1; }
[[ -f "$BRIDGE_OPERATIONAL_CONFIG_SEAL_RECEIPT" \
  && ! -L "$BRIDGE_OPERATIONAL_CONFIG_SEAL_RECEIPT" ]] || {
  echo "operational config seal receipt must be an ordinary file" >&2
  exit 1
}

FROZEN_BUNDLE="$(mktemp -d "${TMPDIR:-/tmp}/bridge-activation-plan.XXXXXX")"
FROZEN_INPUTS="$(mktemp -d "${TMPDIR:-/tmp}/bridge-activation-inputs.XXXXXX")"
trap 'chmod -R u+w "$FROZEN_BUNDLE" "$FROZEN_INPUTS" 2>/dev/null || true; rm -rf "$FROZEN_BUNDLE" "$FROZEN_INPUTS"' EXIT
production_freeze_bundle "$BRIDGE_RELEASE_BUNDLE" "$FROZEN_BUNDLE"
BRIDGE_RELEASE_BUNDLE="$FROZEN_BUNDLE"
production_require_bundle_source_binding "$SOURCE_ROOT" "$BRIDGE_RELEASE_BUNDLE"
production_freeze_receipt "$BRIDGE_OPERATIONAL_CONFIG_SEAL_RECEIPT" \
  "$FROZEN_INPUTS/seal-receipt.json" "operational config seal receipt"
BRIDGE_OPERATIONAL_CONFIG_SEAL_RECEIPT="$FROZEN_INPUTS/seal-receipt.json"
if [[ "$BRIDGE_ACTIVATION_PHASE" == execute ]]; then
  : "${BRIDGE_PRIOR_SCHEDULE_RECEIPT:?execute requires the prior schedule receipt}"
  production_freeze_receipt "$BRIDGE_PRIOR_SCHEDULE_RECEIPT" \
    "$FROZEN_INPUTS/prior-schedule-receipt.json" "prior schedule receipt"
  BRIDGE_PRIOR_SCHEDULE_RECEIPT="$FROZEN_INPUTS/prior-schedule-receipt.json"
fi

require_fixed_source() {
  production_require_bundle_source_binding "$SOURCE_ROOT" "$BRIDGE_RELEASE_BUNDLE"
}

read -r BRIDGE_CANISTER_ID IC_HOST < <(
  python3 -c '
import json,sys
p=json.load(open(sys.argv[1],encoding="utf-8"))
print(p.get("bridge_canister_id",""),p.get("ic_host",""))
' "$BRIDGE_RELEASE_BUNDLE/profile.json"
)
[[ -n "$BRIDGE_CANISTER_ID" && -n "$IC_HOST" ]] || {
  echo "Gate B profile is missing the Bridge Canister or IC host" >&2
  exit 1
}
export BRIDGE_CANISTER_ID IC_HOST
AUTHORIZATION_RECEIPT="${BRIDGE_ACTIVATION_ARTIFACT}.authorization.json"
PREPARE_RECEIPT="${BRIDGE_ACTIVATION_ARTIFACT}.prepare-receipt.json"
PROFILE=(cargo run --locked --quiet --release --manifest-path "$SOURCE_ROOT/Cargo.toml" -p bridge-profile --)

write_prepare_receipt() {
  python3 - "$BRIDGE_ACTIVATION_ARTIFACT" "$AUTHORIZATION_RECEIPT" \
    "$PREPARE_RECEIPT" "$BRIDGE_ACTIVATION_PHASE" "$BRIDGE_GATE_B_MANIFEST_SHA256" <<'PY'
import hashlib,json,os,sys,time
artifact_path,authorization_path,output,phase,gate_hash=sys.argv[1:]
with open(artifact_path,'rb') as source: artifact_hash=hashlib.sha256(source.read()).hexdigest()
with open(authorization_path,'rb') as source: authorization_hash=hashlib.sha256(source.read()).hexdigest()
value={'schema_version':1,'phase':phase,'gate_b_manifest_sha256':gate_hash.lower(),
       'artifact_sha256':artifact_hash,'authorization_receipt_sha256':authorization_hash,
       'bound_at_unix':int(time.time())}
fd=os.open(output,os.O_WRONLY|os.O_CREAT|os.O_EXCL,0o400)
try:
 os.write(fd,(json.dumps(value,separators=(',',':'))+'\n').encode()); os.fsync(fd)
finally: os.close(fd)
parent=os.open(os.path.dirname(os.path.abspath(output)),os.O_RDONLY|os.O_DIRECTORY)
try: os.fsync(parent)
finally: os.close(parent)
PY
}

verify_prepare_receipt() {
  python3 - "$BRIDGE_ACTIVATION_ARTIFACT" "$AUTHORIZATION_RECEIPT" \
    "$PREPARE_RECEIPT" "$BRIDGE_ACTIVATION_PHASE" "$BRIDGE_GATE_B_MANIFEST_SHA256" <<'PY'
import hashlib,json,os,sys
artifact_path,authorization_path,receipt_path,phase,gate_hash=sys.argv[1:]
with open(artifact_path,'rb') as source: artifact_hash=hashlib.sha256(source.read()).hexdigest()
with open(authorization_path,'rb') as source: authorization_hash=hashlib.sha256(source.read()).hexdigest()
value=json.load(open(receipt_path,encoding='utf-8'))
expected={'schema_version','phase','gate_b_manifest_sha256','artifact_sha256',
          'authorization_receipt_sha256','bound_at_unix'}
assert set(value)==expected and value['schema_version']==1 and value['phase']==phase
assert value['gate_b_manifest_sha256'].lower()==gate_hash.lower()
assert value['artifact_sha256'].lower()==artifact_hash
assert value['authorization_receipt_sha256'].lower()==authorization_hash
assert type(value['bound_at_unix']) is int and value['bound_at_unix']>0
PY
}

verify_authorization() {
  "${PROFILE[@]}" verify-controller-activation-authorization "$BRIDGE_ACTIVATION_PHASE" \
    "$BRIDGE_RELEASE_BUNDLE" "$BRIDGE_GATE_B_MANIFEST_SHA256" \
    "$BRIDGE_OPERATIONAL_CONFIG_SEAL_RECEIPT" "$AUTHORIZATION_RECEIPT"
}

freeze_activation_inputs() {
  python3 - "$BRIDGE_ACTIVATION_ARTIFACT" "$AUTHORIZATION_RECEIPT" "$PREPARE_RECEIPT" \
    "$FROZEN_INPUTS" "$BRIDGE_RELEASE_BUNDLE" "$BRIDGE_ACTIVATION_PHASE" \
    "$BRIDGE_GATE_B_MANIFEST_SHA256" "$BRIDGE_OPERATIONAL_CONFIG_SEAL_RECEIPT" <<'PY'
import hashlib,json,os,sys
artifact,authorization,binding,destination,bundle,phase,gate_hash,seal_receipt=sys.argv[1:]
def read(path):
 fd=os.open(path,os.O_RDONLY|os.O_NOFOLLOW)
 try:
  chunks=[]
  while True:
   chunk=os.read(fd,1024*1024)
   if not chunk: return b''.join(chunks)
   chunks.append(chunk)
 finally: os.close(fd)
values={name:read(path) for name,path in [('artifact.json',artifact),('authorization.json',authorization),('binding.json',binding)]}
bound=json.loads(values['binding.json'])
authorized=json.loads(values['authorization.json'])
manifest=json.load(open(os.path.join(bundle,'release-manifest.json'),encoding='utf-8'))
gate=json.load(open(os.path.join(bundle,'gate-a-receipt.json'),encoding='utf-8'))
profile=json.load(open(os.path.join(bundle,'profile.json'),encoding='utf-8'))
expected_authorization={'schema_version','phase','release_id','source_revision','source_tree_sha256',
 'gate_b_manifest_sha256','operational_config_seal_receipt_sha256',
 'controller_principal','certified_controller_set',
 'certified_module_sha256','authorized_at_unix'}
controller=gate['canister_install']['installer_principal']
assert set(authorized)==expected_authorization and authorized['schema_version']==1 and authorized['phase']==phase
assert authorized['release_id']==manifest['release_id'] and authorized['source_revision']==manifest['source_revision']
assert authorized['source_tree_sha256'].lower()==manifest['source_tree_sha256'].lower()
assert authorized['gate_b_manifest_sha256'].lower()==gate_hash.lower()
assert authorized['operational_config_seal_receipt_sha256'].lower()==hashlib.sha256(read(seal_receipt)).hexdigest()
assert authorized['controller_principal']==controller and authorized['certified_controller_set']==[controller]
assert authorized['certified_module_sha256'].lower()==profile['bridge_canister_wasm_sha256'].lower()
assert type(authorized['authorized_at_unix']) is int and authorized['authorized_at_unix']>0
assert set(bound)=={'schema_version','phase','gate_b_manifest_sha256','artifact_sha256','authorization_receipt_sha256','bound_at_unix'}
assert bound['schema_version']==1 and bound['phase']==phase and bound['gate_b_manifest_sha256'].lower()==gate_hash.lower()
assert bound['artifact_sha256'].lower()==hashlib.sha256(values['artifact.json']).hexdigest()
assert bound['authorization_receipt_sha256'].lower()==hashlib.sha256(values['authorization.json']).hexdigest()
assert type(bound['bound_at_unix']) is int and bound['bound_at_unix']>0
for name,data in values.items():
 fd=os.open(os.path.join(destination,name),os.O_WRONLY|os.O_CREAT|os.O_EXCL|os.O_NOFOLLOW,0o400)
 try: os.write(fd,data); os.fsync(fd)
 finally: os.close(fd)
directory=os.open(destination,os.O_RDONLY|os.O_DIRECTORY)
try: os.fsync(directory)
finally: os.close(directory)
os.chmod(destination,0o500)
PY
}

copy_replacement_authorization() {
  python3 - "$FROZEN_INPUTS/authorization.json" "${BRIDGE_ACTIVATION_REPLACEMENT_ARTIFACT}.authorization.json" <<'PY'
import os,sys
source,output=sys.argv[1:]
data=open(source,'rb').read()
if os.path.exists(output):
 assert open(output,'rb').read()==data
 raise SystemExit(0)
fd=os.open(output,os.O_WRONLY|os.O_CREAT|os.O_EXCL|os.O_NOFOLLOW,0o400)
try: os.write(fd,data); os.fsync(fd)
finally: os.close(fd)
parent=os.open(os.path.dirname(os.path.abspath(output)),os.O_RDONLY|os.O_DIRECTORY)
try: os.fsync(parent)
finally: os.close(parent)
PY
}

CLI=(node --no-warnings --experimental-strip-types "$SOURCE_ROOT/tools/governance-relayer/cli.ts")
case "$BRIDGE_ACTIVATION_STEP" in
  prepare)
    : "${BRIDGE_PRODUCTION_CONTROLLER_PEM:?missing production controller identity PEM}"
    : "${BRIDGE_CONFIRMATION_RELAYER_IDENTITY:?missing confirmation relayer ICP identity name}"
    [[ -f "$BRIDGE_PRODUCTION_CONTROLLER_PEM" && ! -L "$BRIDGE_PRODUCTION_CONTROLLER_PEM" ]] || { echo "production controller PEM must be an ordinary file" >&2; exit 1; }
    if [[ ! -e "$AUTHORIZATION_RECEIPT" ]]; then
      [[ ! -e "$BRIDGE_ACTIVATION_ARTIFACT" && ! -e "$PREPARE_RECEIPT" ]] || { echo "activation outputs exist without their authorization receipt" >&2; exit 1; }
      production_validate_gate gate-b-live "$BRIDGE_RELEASE_BUNDLE" "$BRIDGE_GATE_B_MANIFEST_SHA256"
      require_fixed_source
      "${PROFILE[@]}" authorize-controller-activation "$BRIDGE_ACTIVATION_PHASE" \
        "$BRIDGE_RELEASE_BUNDLE" "$BRIDGE_GATE_B_MANIFEST_SHA256" \
        "$BRIDGE_OPERATIONAL_CONFIG_SEAL_RECEIPT" "$AUTHORIZATION_RECEIPT"
      "${PROFILE[@]}" verify-controller-activation-authorization-fresh "$BRIDGE_ACTIVATION_PHASE" \
        "$BRIDGE_RELEASE_BUNDLE" "$BRIDGE_GATE_B_MANIFEST_SHA256" \
        "$BRIDGE_OPERATIONAL_CONFIG_SEAL_RECEIPT" "$AUTHORIZATION_RECEIPT"
      export IC_IDENTITY_PEM="$BRIDGE_PRODUCTION_CONTROLLER_PEM"
      require_fixed_source
      "${CLI[@]}" "prepare-${BRIDGE_ACTIVATION_PHASE}-activation" \
        --artifact-file "$BRIDGE_ACTIVATION_ARTIFACT"
      require_fixed_source
    else
      verify_authorization
      unset IC_IDENTITY_PEM
      if ! "${CLI[@]}" recover-activation --phase "$BRIDGE_ACTIVATION_PHASE" \
          --authorization-file "$AUTHORIZATION_RECEIPT" \
          --artifact-file "$BRIDGE_ACTIVATION_ARTIFACT"; then
        # A Prepared record is intentionally absent from the anonymous pending query.
        # A fresh live gate plus the same controller call resumes it idempotently; if
        # the first call never reached the Canister, this creates the one allowed record.
        production_validate_gate gate-b-live "$BRIDGE_RELEASE_BUNDLE" "$BRIDGE_GATE_B_MANIFEST_SHA256"
        require_fixed_source
        "${PROFILE[@]}" verify-controller-activation-authorization-fresh "$BRIDGE_ACTIVATION_PHASE" \
          "$BRIDGE_RELEASE_BUNDLE" "$BRIDGE_GATE_B_MANIFEST_SHA256" \
          "$BRIDGE_OPERATIONAL_CONFIG_SEAL_RECEIPT" "$AUTHORIZATION_RECEIPT"
        export IC_IDENTITY_PEM="$BRIDGE_PRODUCTION_CONTROLLER_PEM"
        require_fixed_source
        "${CLI[@]}" "prepare-${BRIDGE_ACTIVATION_PHASE}-activation" \
          --artifact-file "$BRIDGE_ACTIVATION_ARTIFACT"
        require_fixed_source
      fi
    fi
    if [[ ! -e "$PREPARE_RECEIPT" ]]; then write_prepare_receipt; else verify_prepare_receipt; fi
    ;;
  replace)
    : "${BRIDGE_PRODUCTION_CONTROLLER_PEM:?missing production controller identity PEM}"
    : "${BRIDGE_ACTIVATION_REPLACEMENT_ARTIFACT:?missing replacement artifact output path}"
    : "${BRIDGE_ACTIVATION_REPLACEMENT_MAX_FEE:?missing replacement max fee}"
    : "${BRIDGE_ACTIVATION_REPLACEMENT_PRIORITY_FEE:?missing replacement priority fee}"
    [[ -f "$BRIDGE_PRODUCTION_CONTROLLER_PEM" && ! -L "$BRIDGE_PRODUCTION_CONTROLLER_PEM" ]] || { echo "production controller PEM must be an ordinary file" >&2; exit 1; }
    [[ "$BRIDGE_ACTIVATION_REPLACEMENT_ARTIFACT" != "$BRIDGE_ACTIVATION_ARTIFACT" ]] || { echo "replacement artifact must use a new path" >&2; exit 1; }
    verify_prepare_receipt
    freeze_activation_inputs
    "${PROFILE[@]}" verify-controller-activation-authorization "$BRIDGE_ACTIVATION_PHASE" \
      "$BRIDGE_RELEASE_BUNDLE" "$BRIDGE_GATE_B_MANIFEST_SHA256" \
      "$BRIDGE_OPERATIONAL_CONFIG_SEAL_RECEIPT" "$FROZEN_INPUTS/authorization.json"
    PRIOR_RECEIPT="${BRIDGE_PRIOR_SCHEDULE_RECEIPT:--}"
    [[ -n "$PRIOR_RECEIPT" ]] || PRIOR_RECEIPT="-"
    "${PROFILE[@]}" verify-controller-activation-artifact "$BRIDGE_ACTIVATION_PHASE" \
      "$BRIDGE_RELEASE_BUNDLE" "$FROZEN_INPUTS/artifact.json" \
      "$BRIDGE_OPERATIONAL_CONFIG_SEAL_RECEIPT" \
      "$FROZEN_INPUTS/authorization.json" "$FROZEN_INPUTS/binding.json" \
      "$PRIOR_RECEIPT"
    copy_replacement_authorization
    export IC_IDENTITY_PEM="$BRIDGE_PRODUCTION_CONTROLLER_PEM"
    require_fixed_source
    "${CLI[@]}" replace-activation \
      --artifact-file "$FROZEN_INPUTS/artifact.json" \
      --authorization-file "$FROZEN_INPUTS/authorization.json" \
      --binding-file "$FROZEN_INPUTS/binding.json" \
      --output-artifact-file "$BRIDGE_ACTIVATION_REPLACEMENT_ARTIFACT" \
      --output-binding-file "${BRIDGE_ACTIVATION_REPLACEMENT_ARTIFACT}.prepare-receipt.json" \
      --max-fee "$BRIDGE_ACTIVATION_REPLACEMENT_MAX_FEE" \
      --priority-fee "$BRIDGE_ACTIVATION_REPLACEMENT_PRIORITY_FEE"
    require_fixed_source
    ;;
  relay)
    : "${BASE_RPC_URL:?missing Base RPC URL for raw relay}"
    verify_prepare_receipt
    freeze_activation_inputs
    "${PROFILE[@]}" verify-controller-activation-authorization "$BRIDGE_ACTIVATION_PHASE" \
      "$BRIDGE_RELEASE_BUNDLE" "$BRIDGE_GATE_B_MANIFEST_SHA256" \
      "$BRIDGE_OPERATIONAL_CONFIG_SEAL_RECEIPT" "$FROZEN_INPUTS/authorization.json"
    PRIOR_RECEIPT="${BRIDGE_PRIOR_SCHEDULE_RECEIPT:--}"
    [[ -n "$PRIOR_RECEIPT" ]] || PRIOR_RECEIPT="-"
    "${PROFILE[@]}" verify-controller-activation-artifact "$BRIDGE_ACTIVATION_PHASE" \
      "$BRIDGE_RELEASE_BUNDLE" "$FROZEN_INPUTS/artifact.json" \
      "$BRIDGE_OPERATIONAL_CONFIG_SEAL_RECEIPT" \
      "$FROZEN_INPUTS/authorization.json" "$FROZEN_INPUTS/binding.json" \
      "$PRIOR_RECEIPT"
    unset IC_IDENTITY_PEM
    require_fixed_source
    "${CLI[@]}" relay --artifact-file "$FROZEN_INPUTS/artifact.json" \
      --authorization-file "$FROZEN_INPUTS/authorization.json" \
      --binding-file "$FROZEN_INPUTS/binding.json"
    require_fixed_source
    ;;
  confirm)
    : "${BRIDGE_CONFIRMATION_RELAYER_PEM:?missing confirmation relayer identity PEM}"
    : "${BRIDGE_ACTIVATION_CONFIRMATION_RECEIPT:?missing confirmation receipt output}"
    : "${BRIDGE_CONTROLLER_ACTIVATION_RECEIPT:?missing verified controller activation receipt output}"
    [[ -f "$BRIDGE_CONFIRMATION_RELAYER_PEM" && ! -L "$BRIDGE_CONFIRMATION_RELAYER_PEM" ]] || { echo "confirmation relayer PEM must be an ordinary file" >&2; exit 1; }
    verify_prepare_receipt
    freeze_activation_inputs
    PRIOR_RECEIPT="${BRIDGE_PRIOR_SCHEDULE_RECEIPT:--}"
    [[ -n "$PRIOR_RECEIPT" ]] || PRIOR_RECEIPT="-"
    "${PROFILE[@]}" verify-controller-activation-confirm-inputs "$BRIDGE_ACTIVATION_PHASE" \
      "$BRIDGE_RELEASE_BUNDLE" "$FROZEN_INPUTS/artifact.json" \
      "$BRIDGE_OPERATIONAL_CONFIG_SEAL_RECEIPT" \
      "$FROZEN_INPUTS/authorization.json" "$FROZEN_INPUTS/binding.json" \
      "$PRIOR_RECEIPT"
    export IC_IDENTITY_PEM="$BRIDGE_CONFIRMATION_RELAYER_PEM"
    require_fixed_source
    "${CLI[@]}" confirm --artifact-file "$FROZEN_INPUTS/artifact.json" \
      --authorization-file "$FROZEN_INPUTS/authorization.json" \
      --binding-file "$FROZEN_INPUTS/binding.json" \
      --receipt-file "$BRIDGE_ACTIVATION_CONFIRMATION_RECEIPT"
    require_fixed_source
    "${CLI[@]}" refresh-attestation
    require_fixed_source
    "${PROFILE[@]}" verify-controller-activation "$BRIDGE_ACTIVATION_PHASE" \
      "$BRIDGE_RELEASE_BUNDLE" "$FROZEN_INPUTS/artifact.json" \
      "$BRIDGE_OPERATIONAL_CONFIG_SEAL_RECEIPT" \
      "$FROZEN_INPUTS/authorization.json" "$FROZEN_INPUTS/binding.json" \
      "$BRIDGE_ACTIVATION_CONFIRMATION_RECEIPT" "$PRIOR_RECEIPT" \
      "$BRIDGE_CONTROLLER_ACTIVATION_RECEIPT"
    require_fixed_source
    ;;
esac
