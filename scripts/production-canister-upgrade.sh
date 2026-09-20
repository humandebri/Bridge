#!/usr/bin/env bash
# Validate and upgrade the production Bridge from authenticated current state.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# shellcheck source=production-validation.sh
source "$ROOT/scripts/production-validation.sh"

MODE="${1:-}"
shift || true
WASM=""
EXPECTED_CURRENT_WASM=""
CONTROLLER_PEM=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    --wasm) WASM="$2"; shift 2 ;;
    --expected-current-wasm)
      EXPECTED_CURRENT_WASM="$(printf '%s' "$2" | tr '[:upper:]' '[:lower:]')"
      shift 2
      ;;
    --controller-pem) CONTROLLER_PEM="$2"; shift 2 ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
done

usage() {
  echo "usage: BRIDGE_RELEASE_BUNDLE=ABS BRIDGE_ICP_IDENTITY=production $0 check --wasm ABS" >&2
  echo "       BRIDGE_RELEASE_BUNDLE=ABS BRIDGE_ICP_IDENTITY=production BRIDGE_CONFIRM_PRODUCTION_CANISTER_UPGRADE=UPGRADE_PRODUCTION_BRIDGE_CANISTER $0 execute --wasm ABS --expected-current-wasm SHA256 --controller-pem ABS" >&2
  exit 2
}
[[ "$MODE" == check || "$MODE" == execute ]] || usage
[[ "${BRIDGE_ICP_IDENTITY:-}" == production ]] || {
  echo "production upgrade requires BRIDGE_ICP_IDENTITY=production" >&2; exit 1;
}
: "${BRIDGE_RELEASE_BUNDLE:?missing reviewed release bundle}"
PROFILE="$BRIDGE_RELEASE_BUNDLE/profile.json"
for path in "$WASM" "$PROFILE"; do
  [[ "$path" == /* && -f "$path" && ! -L "$path" ]] || {
    echo "production upgrade inputs must be absolute regular files" >&2; exit 1;
  }
done
if [[ "$MODE" == execute ]]; then
  [[ "${BRIDGE_CONFIRM_PRODUCTION_CANISTER_UPGRADE:-}" == UPGRADE_PRODUCTION_BRIDGE_CANISTER ]] || {
    echo "production upgrade requires the exact explicit confirmation token" >&2; exit 1;
  }
  [[ "$EXPECTED_CURRENT_WASM" =~ ^[0-9a-f]{64}$ ]] || {
    echo "execute requires --expected-current-wasm SHA256" >&2; exit 1;
  }
  [[ "$CONTROLLER_PEM" == /* && -f "$CONTROLLER_PEM" && ! -L "$CONTROLLER_PEM" ]] || {
    echo "execute requires an absolute production controller PEM" >&2; exit 1;
  }
elif [[ -n "${BRIDGE_CONFIRM_PRODUCTION_CANISTER_UPGRADE:-}" ]]; then
  echo "the production confirmation token is accepted only in execute mode" >&2; exit 1
fi
for tool in cargo git icp python3 shasum; do
  command -v "$tool" >/dev/null || { echo "$tool is required" >&2; exit 1; }
done
production_require_clean_source "$ROOT"
REVISION="$(git -C "$ROOT" rev-parse HEAD)"
TREE="$(git -C "$ROOT" archive HEAD | shasum -a 256 | awk '{print tolower($1)}')"
require_source_identity() {
  production_require_clean_source "$ROOT" \
    && [[ "$(git -C "$ROOT" rev-parse HEAD)" == "$REVISION" ]] \
    && [[ "$(git -C "$ROOT" archive HEAD | shasum -a 256 | awk '{print tolower($1)}')" == "$TREE" ]] || {
      echo "source changed during production validation" >&2
      return 1
    }
}

TMP="$(mktemp -d "${TMPDIR:-/tmp}/bridge-current-upgrade.XXXXXX")"
cleanup() { chmod -R u+w "$TMP" 2>/dev/null || true; rm -rf "$TMP"; }
trap cleanup EXIT
python3 -I -S - "$WASM" "$TMP/candidate.wasm" <<'PY'
import os,stat,sys
source,target=sys.argv[1:]
fd=os.open(source,os.O_RDONLY|getattr(os,'O_NOFOLLOW',0))
try:
 before=os.fstat(fd)
 if not stat.S_ISREG(before.st_mode) or before.st_size>128*1024*1024: raise SystemExit('unsafe Wasm input')
 chunks=[]
 while True:
  chunk=os.read(fd,1024*1024)
  if not chunk: break
  chunks.append(chunk)
 after=os.fstat(fd)
 if (before.st_dev,before.st_ino,before.st_size,before.st_mtime_ns,before.st_ctime_ns)!=(after.st_dev,after.st_ino,after.st_size,after.st_mtime_ns,after.st_ctime_ns):
  raise SystemExit('Wasm changed while being read')
finally: os.close(fd)
out=os.open(target,os.O_WRONLY|os.O_CREAT|os.O_EXCL,0o400)
try:
 for chunk in chunks: os.write(out,chunk)
 os.fsync(out)
finally: os.close(out)
PY
WASM="$TMP/candidate.wasm"
CANDIDATE_WASM="$(shasum -a 256 "$WASM" | awk '{print tolower($1)}')"
read -r CANISTER CONTROLLER HOST < <(python3 -I -S - "$PROFILE" <<'PY'
import json,sys
p=json.load(open(sys.argv[1]))
print(p['bridge_canister_id'],p['pause_principal'],p['ic_host'])
PY
)
[[ "$CANISTER" == lb5i5-ziaaa-aaaar-qcgwq-cai \
  && "$CONTROLLER" == lqfvd-m7ihy-e5dvc-gngvr-blzbt-pupeq-6t7ua-r7v4p-bvqjw-ea7gl-4qe \
  && "$HOST" == https://icp-api.io ]] || {
  echo "release profile does not select the fixed production domain" >&2; exit 1;
}
[[ "$(icp identity principal --identity production)" == "$CONTROLLER" ]] || {
  echo "production identity differs from the configured controller" >&2; exit 1;
}

CARGO_TARGET_DIR="$TMP/profile" cargo build --quiet --locked --release --manifest-path "$ROOT/Cargo.toml" -p bridge-profile
PROFILE_BIN="$TMP/profile/release/bridge-profile"
if [[ -n "$EXPECTED_CURRENT_WASM" ]]; then
  CURRENT_WASM="$EXPECTED_CURRENT_WASM"
else
  CURRENT_WASM="$(icp canister status "$CANISTER" -n ic --identity production --json | python3 -c '
import json,re,sys
v=json.load(sys.stdin)
def find(x,key):
 if isinstance(x,dict):
  out=[]
  for k,v in x.items(): out += ([v] if k==key else []) + find(v,key)
  return out
 if isinstance(x,list):
  out=[]
  for v in x: out += find(v,key)
  return out
 return []
values=find(v,"module_hash") or find(v,"module")
if len(values)!=1: raise SystemExit("ambiguous module hash")
s=str(values[0]).strip().strip(chr(34)).lower().removeprefix("0x")
if not re.fullmatch(r"[0-9a-f]{64}",s): raise SystemExit("invalid module hash")
print(s)')"
fi
[[ "$CURRENT_WASM" =~ ^[0-9a-f]{64}$ ]] || { echo "invalid current module hash" >&2; exit 1; }
export BRIDGE_PRODUCTION_INSTALLER_IDENTITY=production
"$PROFILE_BIN" verify-production-current-state "$PROFILE" "$CONTROLLER" "$CURRENT_WASM" sole

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
printf 'production_upgrade_check=pass current_module_sha256=%s candidate_module_sha256=%s source_revision=%s\n' \
  "$CURRENT_WASM" "$CANDIDATE_WASM" "$REVISION"
[[ "$MODE" == check ]] && exit 0
[[ "$CURRENT_WASM" == "$EXPECTED_CURRENT_WASM" ]] || {
  echo "certified module changed since operator review" >&2; exit 1;
}
[[ "$CANDIDATE_WASM" != "$CURRENT_WASM" ]] || {
  echo "candidate Wasm is already installed" >&2; exit 1;
}
require_source_identity
if ! "$PROFILE_BIN" execute-production-canister-upgrade \
  "$HOST" "$CANISTER" "$CONTROLLER" "$CONTROLLER_PEM" "$WASM" "$PROFILE" "$EXPECTED_CURRENT_WASM"; then
  exit 1
fi
printf 'production_upgrade=complete module_sha256=%s\n' "$CANDIDATE_WASM"
