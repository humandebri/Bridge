#!/usr/bin/env bash
# Root-only SNS activation relay/confirmation. Never creates or submits SNS proposals.
set -Eeuo pipefail

SOURCE_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
# shellcheck source=production-validation.sh
source "$SOURCE_ROOT/scripts/production-validation.sh"

: "${BRIDGE_RELEASE_BUNDLE:?missing historical review bundle}"
: "${BRIDGE_ACTIVATION_PHASE:?set schedule or execute}"
: "${BRIDGE_ACTIVATION_STEP:?set relay or confirm}"
: "${BRIDGE_ACTIVATION_SUBMISSION:?missing SNS proposal submission}"
: "${BRIDGE_ACTIVATION_ARTIFACT:?missing signed pending artifact}"
: "${BRIDGE_ACTIVATION_EVIDENCE:?missing approved Root activation evidence}"
: "${BRIDGE_CURRENT_MODULE_SHA256:?missing approved current module hash}"
: "${BRIDGE_GATE_B_MANIFEST_SHA256:?missing review bundle hash}"

[[ "$BRIDGE_ACTIVATION_PHASE" == schedule || "$BRIDGE_ACTIVATION_PHASE" == execute ]] || { echo "invalid activation phase" >&2; exit 1; }
[[ "$BRIDGE_ACTIVATION_STEP" == relay || "$BRIDGE_ACTIVATION_STEP" == confirm ]] || { echo "invalid activation step" >&2; exit 1; }
EXPECTED_CONFIRMATION=SCHEDULE_PRODUCTION_ASSET_ACTIVATION
if [[ "$BRIDGE_ACTIVATION_PHASE" == execute ]]; then EXPECTED_CONFIRMATION=UNPAUSE_PRODUCTION_ASSET_ACCEPTANCE; fi
[[ "${BRIDGE_CONFIRM_ASSET_ACCEPTANCE:-}" == "$EXPECTED_CONFIRMATION" ]] || { echo "phase-specific production confirmation is required" >&2; exit 1; }

production_require_clean_source "$SOURCE_ROOT"
REVISION="$(git -C "$SOURCE_ROOT" rev-parse HEAD)"
TREE="$(git -C "$SOURCE_ROOT" archive HEAD | shasum -a 256 | awk '{print $1}')"
FROZEN_BUNDLE="$(mktemp -d "${TMPDIR:-/tmp}/bridge-root-activation-bundle.XXXXXX")"
FROZEN_INPUTS="$(mktemp -d "${TMPDIR:-/tmp}/bridge-root-activation-inputs.XXXXXX")"
cleanup() {
  local status=$?
  trap - EXIT
  chmod -R u+w "$FROZEN_BUNDLE" "$FROZEN_INPUTS" 2>/dev/null || true
  rm -rf "$FROZEN_BUNDLE" "$FROZEN_INPUTS"
  exit "$status"
}
trap cleanup EXIT
production_freeze_bundle "$BRIDGE_RELEASE_BUNDLE" "$FROZEN_BUNDLE"
production_freeze_receipt "$BRIDGE_ACTIVATION_SUBMISSION" "$FROZEN_INPUTS/submission.json" "SNS submission"
production_freeze_receipt "$BRIDGE_ACTIVATION_ARTIFACT" "$FROZEN_INPUTS/artifact.json" "activation artifact"
production_freeze_receipt "$BRIDGE_ACTIVATION_EVIDENCE" "$FROZEN_INPUTS/evidence.json" "approved activation evidence"

if [[ "$BRIDGE_ACTIVATION_STEP" == confirm ]]; then
  [[ -n "${BRIDGE_CONFIRMATION_RELAYER_PEM:-}" ]] || { echo "missing confirmation relayer PEM" >&2; exit 1; }
  [[ -n "${BRIDGE_ACTIVATION_CONFIRMATION_RECEIPT:-}" ]] || { echo "missing new confirmation receipt path" >&2; exit 1; }
  [[ -n "${BRIDGE_ACTIVATION_VERIFIED_RECEIPT:-}" ]] || { echo "missing new verified receipt path" >&2; exit 1; }
  [[ ! -e "$BRIDGE_ACTIVATION_CONFIRMATION_RECEIPT" && ! -L "$BRIDGE_ACTIVATION_CONFIRMATION_RECEIPT" ]] || { echo "confirmation receipt already exists" >&2; exit 1; }
  [[ ! -e "$BRIDGE_ACTIVATION_VERIFIED_RECEIPT" && ! -L "$BRIDGE_ACTIVATION_VERIFIED_RECEIPT" ]] || { echo "verified receipt already exists" >&2; exit 1; }
  production_freeze_receipt "$BRIDGE_CONFIRMATION_RELAYER_PEM" "$FROZEN_INPUTS/relayer.pem" "confirmation relayer PEM" 1048576
fi
PRIOR=-
if [[ "$BRIDGE_ACTIVATION_PHASE" == execute && "$BRIDGE_ACTIVATION_STEP" == confirm ]]; then
  [[ -n "${BRIDGE_PRIOR_SCHEDULE_RECEIPT:-}" ]] || { echo "execute confirmation requires the verified schedule receipt" >&2; exit 1; }
  production_freeze_receipt "$BRIDGE_PRIOR_SCHEDULE_RECEIPT" "$FROZEN_INPUTS/prior.json" "schedule receipt"
  PRIOR="$FROZEN_INPUTS/prior.json"
fi

read -r BRIDGE_CANISTER_ID IC_HOST < <(python3 -c 'import json,sys; p=json.load(open(sys.argv[1])); print(p["bridge_canister_id"],p["ic_host"])' "$FROZEN_BUNDLE/profile.json")
export BRIDGE_CANISTER_ID IC_HOST BRIDGE_GATE_B_MANIFEST_SHA256 BRIDGE_CURRENT_MODULE_SHA256
(
  unset BRIDGE_CONFIRM_ASSET_ACCEPTANCE
  production_run_proof_gate "$SOURCE_ROOT" "$REVISION" "$TREE"
)
production_require_clean_source "$SOURCE_ROOT"
[[ "$(git -C "$SOURCE_ROOT" rev-parse HEAD)" == "$REVISION" ]] || { echo "source changed after proof" >&2; exit 1; }

PROFILE=(cargo run --locked --quiet --release --manifest-path "$SOURCE_ROOT/Cargo.toml" -p bridge-profile --)
"${PROFILE[@]}" verify-root-activation-pending "$BRIDGE_ACTIVATION_PHASE" "$FROZEN_BUNDLE" \
  "$FROZEN_INPUTS/submission.json" "$FROZEN_INPUTS/artifact.json" compare "$FROZEN_INPUTS/evidence.json"

python3 - "$FROZEN_INPUTS/artifact.json" "$FROZEN_INPUTS/evidence.json" "$FROZEN_INPUTS/binding.json" "$BRIDGE_ACTIVATION_PHASE" "$BRIDGE_GATE_B_MANIFEST_SHA256" <<'PY'
import hashlib,json,os,sys,time
artifact,evidence,path,phase,gate=sys.argv[1:]
artifact_bytes=open(artifact,'rb').read()
evidence_bytes=open(evidence,'rb').read()
review=json.loads(evidence_bytes)
if review.get('phase')!=phase or review.get('gate_b_manifest_sha256')!=gate.lower():
 raise SystemExit('approved evidence differs from phase or review bundle hash')
binding={'schema_version':1,'phase':phase,'gate_b_manifest_sha256':gate.lower(),
 'artifact_sha256':hashlib.sha256(artifact_bytes).hexdigest(),
 'authorization_receipt_sha256':hashlib.sha256(evidence_bytes).hexdigest(),
 'bound_at_unix':int(time.time())}
fd=os.open(path,os.O_WRONLY|os.O_CREAT|os.O_EXCL|os.O_NOFOLLOW,0o400)
try:
 data=(json.dumps(binding,separators=(',',':'))+'\n').encode()
 while data:
  written=os.write(fd,data)
  if written<=0: raise SystemExit('short write to activation binding')
  data=data[written:]
 os.fsync(fd)
finally: os.close(fd)
PY

production_require_clean_source "$SOURCE_ROOT"
[[ "$(git -C "$SOURCE_ROOT" rev-parse HEAD)" == "$REVISION" ]] || { echo "source changed before relay" >&2; exit 1; }
CLI=(node --no-warnings --experimental-strip-types "$SOURCE_ROOT/tools/governance-relayer/cli.ts")
if [[ "$BRIDGE_ACTIVATION_STEP" == relay ]]; then
  unset IC_IDENTITY_PEM
  "${CLI[@]}" relay --artifact-file "$FROZEN_INPUTS/artifact.json" --authorization-file "$FROZEN_INPUTS/evidence.json" --binding-file "$FROZEN_INPUTS/binding.json"
else
  export IC_IDENTITY_PEM="$FROZEN_INPUTS/relayer.pem"
  "${CLI[@]}" confirm --artifact-file "$FROZEN_INPUTS/artifact.json" --authorization-file "$FROZEN_INPUTS/evidence.json" --binding-file "$FROZEN_INPUTS/binding.json" --receipt-file "$BRIDGE_ACTIVATION_CONFIRMATION_RECEIPT"
  production_require_clean_source "$SOURCE_ROOT"
  [[ "$(git -C "$SOURCE_ROOT" rev-parse HEAD)" == "$REVISION" ]] || { echo "confirmation completed but source changed; preserve the receipt and do not retry" >&2; exit 1; }
  "${PROFILE[@]}" verify-root-activation "$BRIDGE_ACTIVATION_PHASE" "$FROZEN_BUNDLE" "$FROZEN_INPUTS/submission.json" "$PRIOR" "$BRIDGE_ACTIVATION_VERIFIED_RECEIPT"
fi
