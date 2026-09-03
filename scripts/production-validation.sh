#!/usr/bin/env bash
# Shared fail-closed evidence validation for fixed production drivers.

production_require_clean_source() {
  local source_root="$1" dirty submodule_status
  [[ -d "$source_root/.git" ]] || { echo "production source root is not a Git worktree" >&2; return 1; }
  dirty="$(git -C "$source_root" status --porcelain=v1 --untracked-files=all --ignore-submodules=none)" || return 1
  [[ -z "$dirty" ]] || { echo "release source or a nested submodule is dirty" >&2; return 1; }
  submodule_status="$(git -C "$source_root" submodule status --recursive 2>/dev/null)" || {
    echo "failed to inspect release source submodules" >&2
    return 1
  }
  if printf '%s\n' "$submodule_status" | sed -nE '/^[+-U]/p' | read -r _; then
    echo "release source has an uninitialized or non-recorded submodule revision" >&2
    return 1
  fi
}

production_require_bundle_source_binding() {
  local source_root="$1" bundle="$2" revision tree manifest_revision manifest_tree
  production_require_clean_source "$source_root" || return 1
  [[ -f "$bundle/release-manifest.json" && ! -L "$bundle/release-manifest.json" ]] || {
    echo "release manifest is missing or unsafe" >&2; return 1;
  }
  revision="$(git -C "$source_root" rev-parse HEAD)" || return 1
  tree="$(git -C "$source_root" archive HEAD | shasum -a 256 | awk '{print $1}')" || return 1
  read -r manifest_revision manifest_tree < <(
    python3 -c 'import json,sys;m=json.load(open(sys.argv[1]));print(m.get("source_revision",""),m.get("source_tree_sha256",""))' "$bundle/release-manifest.json"
  )
  [[ "$revision" == "$manifest_revision" \
    && "$tree" == "$(printf '%s' "$manifest_tree" | tr '[:upper:]' '[:lower:]')" ]] || {
    echo "release bundle is not bound to the fixed clean source" >&2; return 1;
  }
}

production_run_proof_gate() {
  local source_root="$1" expected_revision="$2" expected_tree="$3"
  local proof_script before_revision before_tree after_revision after_tree
  proof_script="$source_root/scripts/ci-local.sh"
  [[ -x "$proof_script" ]] || {
    echo "tracked proof gate is missing or not executable" >&2
    return 1
  }
  git -C "$source_root" ls-files --error-unmatch scripts/ci-local.sh >/dev/null || {
    echo "proof gate is not tracked by the bound source revision" >&2
    return 1
  }
  production_require_clean_source "$source_root" || return 1
  before_revision="$(git -C "$source_root" rev-parse HEAD)" || return 1
  before_tree="$(git -C "$source_root" archive HEAD | shasum -a 256 | awk '{print $1}')" || return 1
  [[ "$before_revision" == "$expected_revision" \
    && "$before_tree" == "$(printf '%s' "$expected_tree" | tr '[:upper:]' '[:lower:]')" ]] || {
    echo "proof gate source does not match the release manifest" >&2
    return 1
  }
  "$proof_script" proofs || {
    echo "release proof gate failed" >&2
    return 1
  }
  production_require_clean_source "$source_root" || {
    echo "release source changed while proofs were running" >&2
    return 1
  }
  after_revision="$(git -C "$source_root" rev-parse HEAD)" || return 1
  after_tree="$(git -C "$source_root" archive HEAD | shasum -a 256 | awk '{print $1}')" || return 1
  [[ "$after_revision" == "$before_revision" && "$after_tree" == "$before_tree" ]] || {
    echo "release source identity changed while proofs were running" >&2
    return 1
  }
}

# Reserve evidence before an irreversible operation. An interrupted attempt
# deliberately leaves an empty marker so that reruns cannot silently redeploy.
production_reserve_output() {
  local target="$1" label="${2:-output}"
  [[ -n "$target" && ! -L "$target" && ! -e "$target" ]] || {
    echo "$label already exists or is a symlink" >&2; return 1;
  }
  python3 - "$target" "$label" <<'PY'
import os, sys
target, label = sys.argv[1:]
parent = os.path.dirname(os.path.abspath(target)) or '.'
if not os.path.isdir(parent):
    raise SystemExit(f'{label} parent directory does not exist')
fd = os.open(target, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
try:
    os.fsync(fd)
finally:
    os.close(fd)
dirfd = os.open(parent, os.O_RDONLY)
try:
    os.fsync(dirfd)
finally:
    os.close(dirfd)
PY
}

production_atomic_replace() {
  local staged="$1" target="$2"
  python3 - "$staged" "$target" <<'PY'
import os, stat, sys
staged, target = sys.argv[1:]
fd = os.open(staged, os.O_RDONLY | os.O_NOFOLLOW)
try:
    if not stat.S_ISREG(os.fstat(fd).st_mode):
        raise SystemExit('staged evidence is not a regular file')
    os.fsync(fd)
finally:
    os.close(fd)
os.replace(staged, target)
parent = os.path.dirname(os.path.abspath(target)) or '.'
fd = os.open(parent, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
try:
    os.fsync(fd)
finally:
    os.close(fd)
PY
}

# Copy one self-consistent, hash-verified view of a release bundle into a private
# directory. Every source file is opened without following symlinks and read from
# that descriptor exactly once, so later path swaps cannot change the approved plan.
production_freeze_bundle() {
  local source="$1" destination="$2"
  [[ -d "$source" && ! -L "$source" && -d "$destination" && ! -L "$destination" ]] || {
    echo "release bundle freeze requires ordinary directories" >&2; return 1;
  }
  python3 - "$source" "$destination" <<'PY'
import hashlib,json,os,re,stat,sys
source,destination=sys.argv[1:]
directory=os.open(source,os.O_RDONLY|os.O_DIRECTORY|os.O_NOFOLLOW)
def path_parts(name,top_level=False):
 if not isinstance(name,str): raise SystemExit('release artifact path is not a string')
 parts=name.split('/')
 if not parts or any(not re.fullmatch(r'[A-Za-z0-9][A-Za-z0-9._-]{0,127}',part) for part in parts): raise SystemExit(f'unsafe release artifact path: {name}')
 if top_level and len(parts)!=1: raise SystemExit(f'unsafe top-level release artifact path: {name}')
 return parts
def read_regular(name,limit,top_level=False):
 parts=path_parts(name,top_level)
 parent_fd=os.dup(directory)
 try:
  for part in parts[:-1]:
   next_fd=os.open(part,os.O_RDONLY|os.O_DIRECTORY|os.O_NOFOLLOW,dir_fd=parent_fd)
   os.close(parent_fd); parent_fd=next_fd
  fd=os.open(parts[-1],os.O_RDONLY|os.O_NOFOLLOW,dir_fd=parent_fd)
  try:
   info=os.fstat(fd)
   if not stat.S_ISREG(info.st_mode) or info.st_size>limit: raise SystemExit(f'invalid release artifact: {name}')
   chunks=[]; remaining=limit+1
   while remaining:
    chunk=os.read(fd,min(1024*1024,remaining))
    if not chunk: break
    chunks.append(chunk); remaining-=len(chunk)
   value=b''.join(chunks)
   if len(value)>limit: raise SystemExit(f'release artifact exceeds size limit: {name}')
   return value
  finally: os.close(fd)
 finally: os.close(parent_fd)
def write_regular(name,value):
 parts=path_parts(name)
 parent=destination
 for part in parts[:-1]:
  parent=os.path.join(parent,part)
  os.makedirs(parent,mode=0o700,exist_ok=True)
 fd=os.open(os.path.join(destination,*parts),os.O_WRONLY|os.O_CREAT|os.O_EXCL|os.O_NOFOLLOW,0o400)
 try:
  view=memoryview(value)
  while view:
   written=os.write(fd,view)
   if written<=0: raise SystemExit(f'short write while freezing release artifact: {name}')
   view=view[written:]
  os.fsync(fd)
 finally: os.close(fd)
try:
 manifest_bytes=read_regular('release-manifest.json',4*1024*1024,True)
 manifest=json.loads(manifest_bytes)
 artifacts=manifest.get('artifacts')
 if not isinstance(artifacts,list) or len(artifacts)>32: raise SystemExit('release manifest artifact count is invalid')
 files={'release-manifest.json':manifest_bytes}
 for entry in artifacts:
  if not isinstance(entry,dict) or set(entry)!={'path','sha256'}: raise SystemExit('invalid release artifact entry')
  name=entry['path']; expected=entry['sha256']
  path_parts(name,True)
  if name in files or not isinstance(expected,str) or not re.fullmatch(r'[0-9a-fA-F]{64}',expected): raise SystemExit(f'invalid duplicate release artifact: {name}')
  limit=256*1024*1024 if name in {'bridge-canister.wasm','bridge-runtime.bin','bsns-creation.bin','bsns-runtime.bin'} else 16*1024*1024
  value=read_regular(name,limit,True)
  if hashlib.sha256(value).hexdigest().lower()!=expected.lower(): raise SystemExit(f'release artifact hash mismatch while freezing: {name}')
  files[name]=value
 rpc=json.loads(files.get('rpc-e2e.json',b'{}'))
 scenarios=rpc.get('scenarios',{})
 if not isinstance(scenarios,dict) or len(scenarios)>16: raise SystemExit('invalid RPC rehearsal scenarios')
 raw_paths=set(); raw_count=0; raw_total=0
 for scenario in scenarios.values():
  if scenario is None: continue
  if not isinstance(scenario,dict) or not isinstance(scenario.get('artifacts'),list): raise SystemExit('invalid RPC rehearsal artifact references')
  for reference in scenario['artifacts']:
   if not isinstance(reference,dict): raise SystemExit('invalid RPC rehearsal artifact reference')
   name=reference.get('path'); expected=reference.get('sha256')
   parts=path_parts(name)
   if parts[0]!='artifacts' or not isinstance(expected,str) or not re.fullmatch(r'[0-9a-f]{64}',expected): raise SystemExit(f'invalid RPC rehearsal artifact reference: {name}')
   raw_count+=1
   if raw_count>128 or name in raw_paths: raise SystemExit('RPC rehearsal raw artifact references exceed the safe bound or contain duplicates')
   raw_paths.add(name)
   value=read_regular(name,16*1024*1024)
   raw_total+=len(value)
   if raw_total>64*1024*1024: raise SystemExit('RPC rehearsal raw artifacts exceed the cumulative size limit')
   if hashlib.sha256(value).hexdigest()!=expected: raise SystemExit(f'RPC rehearsal artifact hash mismatch while freezing: {name}')
   if name in files and files[name]!=value: raise SystemExit(f'conflicting RPC rehearsal artifact reference: {name}')
   files[name]=value
 for name,value in files.items(): write_regular(name,value)
 for current,dirs,_ in os.walk(destination,topdown=False):
  for name in dirs: os.chmod(os.path.join(current,name),0o500)
 destination_fd=os.open(destination,os.O_RDONLY|os.O_DIRECTORY|os.O_NOFOLLOW)
 try: os.fsync(destination_fd)
 finally: os.close(destination_fd)
 os.chmod(destination,0o500)
finally: os.close(directory)
PY
}

# Freeze one externally supplied receipt through a no-follow descriptor before
# validation so a later path swap cannot change the bytes authorized for use.
production_freeze_receipt() {
  local source="$1" destination="$2" label="${3:-receipt}"
  python3 - "$source" "$destination" "$label" <<'PY'
import os,stat,sys
source,destination,label=sys.argv[1:]
fd=os.open(source,os.O_RDONLY|os.O_NOFOLLOW)
try:
 info=os.fstat(fd)
 if not stat.S_ISREG(info.st_mode) or info.st_size>16*1024*1024:
  raise SystemExit(f'{label} is not a bounded regular file')
 chunks=[]
 while True:
  chunk=os.read(fd,1024*1024)
  if not chunk: break
  chunks.append(chunk)
 data=b''.join(chunks)
finally: os.close(fd)
out=os.open(destination,os.O_WRONLY|os.O_CREAT|os.O_EXCL|os.O_NOFOLLOW,0o400)
try:
 view=memoryview(data)
 while view:
  written=os.write(out,view)
  if written<=0: raise SystemExit(f'short write while freezing {label}')
  view=view[written:]
 os.fsync(out)
finally: os.close(out)
PY
}

production_validate_gate_b_source_chain() {
  local source_root="$1" bundle="$2"
  local gate_a_revision gate_a_tree upgrade_revision upgrade_tree current_revision current_tree
  local actual_gate_a_tree actual_upgrade_tree actual_current_tree
  [[ -f "$bundle/post-gate-a-policy-transition.json" \
    && ! -L "$bundle/post-gate-a-policy-transition.json" ]] || {
    echo "Gate B policy transition is missing or unsafe" >&2
    return 1
  }
  read -r gate_a_revision gate_a_tree upgrade_revision upgrade_tree current_revision current_tree < <(
    python3 -c '
import json,sys
t=json.load(open(sys.argv[1],encoding="utf-8"))
print(t.get("from_source_revision",""),t.get("from_source_tree_sha256",""),t.get("upgrade_source_revision",""),t.get("upgrade_source_tree_sha256",""),t.get("to_source_revision",""),t.get("to_source_tree_sha256",""))
' "$bundle/post-gate-a-policy-transition.json"
  )
  [[ "$gate_a_revision" =~ ^[0-9a-f]{40}$ \
    && "$upgrade_revision" =~ ^[0-9a-f]{40}$ \
    && "$current_revision" =~ ^[0-9a-f]{40}$ \
    && "$gate_a_tree" =~ ^[0-9a-fA-F]{64}$ \
    && "$upgrade_tree" =~ ^[0-9a-fA-F]{64}$ \
    && "$current_tree" =~ ^[0-9a-fA-F]{64}$ ]] || {
    echo "Gate B source chain metadata is malformed" >&2
    return 1
  }
  git -C "$source_root" cat-file -e "${gate_a_revision}^{commit}" 2>/dev/null \
    && git -C "$source_root" cat-file -e "${upgrade_revision}^{commit}" 2>/dev/null \
    && git -C "$source_root" cat-file -e "${current_revision}^{commit}" 2>/dev/null \
    && git -C "$source_root" merge-base --is-ancestor "$gate_a_revision" "$upgrade_revision" \
    && git -C "$source_root" merge-base --is-ancestor "$upgrade_revision" "$current_revision" || {
      echo "Gate B source chain is not Gate A to upgrade to current" >&2
      return 1
    }
  actual_gate_a_tree="$(git -C "$source_root" --attr-source="$gate_a_revision" archive --format=tar "$gate_a_revision" | shasum -a 256 | awk '{print tolower($1)}')"
  actual_upgrade_tree="$(git -C "$source_root" --attr-source="$upgrade_revision" archive --format=tar "$upgrade_revision" | shasum -a 256 | awk '{print tolower($1)}')"
  actual_current_tree="$(git -C "$source_root" --attr-source="$current_revision" archive --format=tar "$current_revision" | shasum -a 256 | awk '{print tolower($1)}')"
  [[ "$actual_gate_a_tree" == "$(printf '%s' "$gate_a_tree" | tr '[:upper:]' '[:lower:]')" \
    && "$actual_upgrade_tree" == "$(printf '%s' "$upgrade_tree" | tr '[:upper:]' '[:lower:]')" \
    && "$actual_current_tree" == "$(printf '%s' "$current_tree" | tr '[:upper:]' '[:lower:]')" ]] || {
    echo "Gate B source chain tree hash mismatch" >&2
    return 1
  }
}

production_validate_gate() {
  local mode="$1" bundle="$2" expected_hash="$3" canister_install_receipt="${4:-}"
  local completed_gate_a_receipt="${5:-}"
  local deployment_binding="${6:-}"
  local final_profile="${7:-}"
  local fee_cycles_measurements="${8:-}"
  local handover_seal_receipt="${5:-}"
  local handover_schedule_receipt="${6:-}"
  local handover_execute_receipt="${7:-}"
  local source_root target profile_bin output actual_hash revision tree manifest_revision manifest_tree
  local expected_relayer resolved_relayer bridge_canister refresh_output final_output
  source_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
  [[ -f "$bundle/release-manifest.json" ]] || { echo "release manifest is missing" >&2; return 1; }
  production_require_clean_source "$source_root" || return 1
  revision="$(git -C "$source_root" rev-parse HEAD)"
  tree="$(git -C "$source_root" archive HEAD | shasum -a 256 | awk '{print $1}')"
  read -r manifest_revision manifest_tree < <(python3 -c 'import json,sys;m=json.load(open(sys.argv[1]));print(m.get("source_revision",""),m.get("source_tree_sha256",""))' "$bundle/release-manifest.json")
  [[ "$revision" == "$manifest_revision" && "$tree" == "$(printf '%s' "$manifest_tree" | tr '[:upper:]' '[:lower:]')" ]] || { echo "release bundle is not bound to the fixed clean source" >&2; return 1; }
  [[ "$expected_hash" =~ ^[0-9a-fA-F]{64}$ ]] || { echo "invalid expected Gate manifest hash" >&2; return 1; }
  target="$(mktemp -d "${TMPDIR:-/tmp}/bridge-driver-validator.XXXXXX")"
  CARGO_TARGET_DIR="$target" cargo build --locked --quiet --release --manifest-path "$source_root/Cargo.toml" -p bridge-profile || { rm -rf "$target"; return 1; }
  profile_bin="$target/release/bridge-profile"
  if [[ "$mode" == gate-a ]]; then output="$("$profile_bin" validate-bundle --offline "$bundle")" || { rm -rf "$target"; return 1; }
  elif [[ "$mode" == gate-b-pre-seal || "$mode" == gate-b-live ]]; then output="$("$profile_bin" validate-bundle --offline --gate-b "$bundle")" || { rm -rf "$target"; return 1; }
  elif [[ "$mode" == handover ]]; then
    [[ -z "$canister_install_receipt" ]] || {
      rm -rf "$target"
      echo "handover mode does not accept an install receipt" >&2
      return 1
    }
    for receipt in "$handover_seal_receipt" "$handover_schedule_receipt" "$handover_execute_receipt"; do
      [[ -f "$receipt" && ! -L "$receipt" ]] || {
        rm -rf "$target"
        echo "controller handover requires seal, schedule, and execute receipts" >&2
        return 1
      }
    done
    output="$("$profile_bin" validate-production-handover-candidate \
      "$bundle" "$handover_seal_receipt" "$handover_schedule_receipt" \
      "$handover_execute_receipt")" || {
      rm -rf "$target"
      echo "controller handover activation lineage is invalid" >&2
      return 1
    }
    [[ "$output" =~ ^production_handover_candidate=pass[[:space:]]manifest_sha256=([0-9a-fA-F]{64})$ ]] || {
      rm -rf "$target"
      echo "controller handover candidate result is malformed" >&2
      return 1
    }
    actual_hash="${BASH_REMATCH[1]}"
  else rm -rf "$target"; echo "invalid production gate mode" >&2; return 1
  fi
  if [[ "$mode" == gate-a ]]; then
    [[ "$output" =~ ^gate_a=pass[[:space:]]authorizing=true[[:space:]]manifest_sha256=([0-9a-fA-F]{64})$ ]] || { rm -rf "$target"; echo "driver Gate A result is not authorizing" >&2; return 1; }
    actual_hash="${BASH_REMATCH[1]}"
  elif [[ "$mode" != handover ]]; then
    [[ "$output" =~ ^gate_b=pre_seal-pass[[:space:]]authorizing=seal[[:space:]]manifest_sha256=([0-9a-fA-F]{64})$ ]] || { rm -rf "$target"; echo "driver pre-seal Gate B result is malformed" >&2; return 1; }
    actual_hash="${BASH_REMATCH[1]}"
  fi
  [[ -n "$actual_hash" && "$(printf '%s' "$actual_hash" | tr '[:upper:]' '[:lower:]')" == "$(printf '%s' "$expected_hash" | tr '[:upper:]' '[:lower:]')" ]] || { rm -rf "$target"; echo "driver Gate manifest hash mismatch" >&2; return 1; }
  if [[ "$mode" == gate-b-pre-seal || "$mode" == gate-b-live || "$mode" == handover ]]; then
    production_validate_gate_b_source_chain "$source_root" "$bundle" || { rm -rf "$target"; return 1; }
  fi
  production_run_proof_gate "$source_root" "$manifest_revision" "$manifest_tree" || { rm -rf "$target"; return 1; }
  "$source_root/scripts/rebuild-release-artifacts.sh" \
    "$bundle" "$manifest_revision" "$manifest_tree" || { rm -rf "$target"; return 1; }
  if [[ "$mode" == gate-b-pre-seal ]]; then
    rm -rf "$target"
    return 0
  fi
  if [[ "$mode" == handover ]]; then
    "$profile_bin" verify-production-canister-handover \
      "$bundle" "$handover_seal_receipt" "$handover_schedule_receipt" \
      "$handover_execute_receipt" >/dev/null || {
      rm -rf "$target"
      echo "production Canister is not active, integral, and bound to the activation lineage" >&2
      return 1
    }
    if [[ -n "${BRIDGE_HANDOVER_VALIDATOR_BIN:-}" ]]; then
      [[ ! -e "$BRIDGE_HANDOVER_VALIDATOR_BIN" && ! -L "$BRIDGE_HANDOVER_VALIDATOR_BIN" ]] || {
        rm -rf "$target"; echo "handover validator output already exists or is a symlink" >&2; return 1;
      }
      cp "$profile_bin" "$BRIDGE_HANDOVER_VALIDATOR_BIN" || { rm -rf "$target"; return 1; }
      chmod 500 "$BRIDGE_HANDOVER_VALIDATOR_BIN" || { rm -rf "$target"; return 1; }
    fi
    rm -rf "$target"
    return 0
  fi
  if [[ "$mode" == gate-a ]]; then
    [[ -f "$canister_install_receipt" && ! -L "$canister_install_receipt" ]] || {
      rm -rf "$target"
      echo "Gate A requires the verified production Canister install receipt" >&2
      return 1
    }
    [[ -z "$completed_gate_a_receipt" && -z "$deployment_binding" \
      && -z "$final_profile" && -z "$fee_cycles_measurements" ]] || {
      rm -rf "$target"
      echo "Gate A mode rejects legacy controller handover inputs" >&2
      return 1
    }
    "$profile_bin" verify-production-canister-predeploy \
      "$bundle/profile.json" "$canister_install_receipt" >/dev/null || {
      rm -rf "$target"
      echo "live production Canister no longer matches the paused predeploy profile" >&2
      return 1
    }
    rm -rf "$target"
    return 0
  fi

  : "${BRIDGE_CONFIRMATION_RELAYER_IDENTITY:?missing confirmation relayer ICP identity name}"
  : "${BRIDGE_ACTIVATION_PHASE:?missing activation phase}"
  command -v icp >/dev/null || { rm -rf "$target"; echo "icp is required" >&2; return 1; }
  production_require_clean_source "$source_root" || { rm -rf "$target"; return 1; }
  revision="$(git -C "$source_root" rev-parse HEAD)"
  tree="$(git -C "$source_root" archive HEAD | shasum -a 256 | awk '{print $1}')"
  [[ "$revision" == "$manifest_revision" && "$tree" == "$(printf '%s' "$manifest_tree" | tr '[:upper:]' '[:lower:]')" ]] || { rm -rf "$target"; echo "source changed during Gate B validation" >&2; return 1; }
  read -r expected_relayer bridge_canister < <(python3 -c 'import json,sys;p=json.load(open(sys.argv[1],encoding="utf-8"));print(p.get("confirmation_relayer_principal",""),p.get("bridge_canister_id",""))' "$bundle/profile.json")
  [[ -n "$expected_relayer" && -n "$bridge_canister" ]] || { rm -rf "$target"; echo "Gate B profile is missing confirmation relayer or Bridge Canister identity" >&2; return 1; }
  resolved_relayer="$(icp identity principal --identity "$BRIDGE_CONFIRMATION_RELAYER_IDENTITY")" || { rm -rf "$target"; echo "failed to resolve confirmation relayer ICP identity" >&2; return 1; }
  [[ "$resolved_relayer" == "$expected_relayer" ]] || { rm -rf "$target"; echo "confirmation relayer ICP identity differs from the approved profile" >&2; return 1; }

  refresh_output="$(mktemp "${TMPDIR:-/tmp}/bridge-attestation-refresh.XXXXXX")"
  if ! icp canister call "$bridge_canister" refresh_activation_attestation '()' \
    -n ic --identity "$BRIDGE_CONFIRMATION_RELAYER_IDENTITY" --json >"$refresh_output" 2>&1; then
    echo "activation attestation refresh returned an ambiguous failure; checking the authenticated live postcondition" >&2
  fi
  rm -f "$refresh_output"
  final_output="$("$profile_bin" verify-live "$BRIDGE_ACTIVATION_PHASE" "$bundle")" || { rm -rf "$target"; return 1; }
  [[ "$final_output" =~ ^gate_b=live-pass[[:space:]]authorizing=${BRIDGE_ACTIVATION_PHASE}[[:space:]]manifest_sha256=([0-9a-fA-F]{64})$ ]] || { rm -rf "$target"; echo "final live Gate B result is malformed" >&2; return 1; }
  actual_hash="${BASH_REMATCH[1]}"
  [[ "$(printf '%s' "$actual_hash" | tr '[:upper:]' '[:lower:]')" == "$(printf '%s' "$expected_hash" | tr '[:upper:]' '[:lower:]')" ]] || { rm -rf "$target"; echo "final Gate B manifest hash mismatch" >&2; return 1; }
  if [[ "$BRIDGE_ACTIVATION_PHASE" == execute ]]; then
    : "${BRIDGE_PRIOR_SCHEDULE_RECEIPT:?missing prior schedule receipt}"
    : "${BRIDGE_OPERATIONAL_CONFIG_SEAL_RECEIPT:?missing operational config seal receipt}"
    "$profile_bin" verify-controller-schedule-receipt-live "$bundle" \
      "$BRIDGE_OPERATIONAL_CONFIG_SEAL_RECEIPT" "$BRIDGE_PRIOR_SCHEDULE_RECEIPT" \
      >/dev/null || { rm -rf "$target"; return 1; }
  elif [[ "$BRIDGE_ACTIVATION_PHASE" != schedule ]]; then
    rm -rf "$target"
    echo "invalid activation phase" >&2
    return 1
  fi
  production_require_clean_source "$source_root" || { rm -rf "$target"; return 1; }
  revision="$(git -C "$source_root" rev-parse HEAD)"
  tree="$(git -C "$source_root" archive HEAD | shasum -a 256 | awk '{print $1}')"
  [[ "$revision" == "$manifest_revision" && "$tree" == "$(printf '%s' "$manifest_tree" | tr '[:upper:]' '[:lower:]')" ]] || { rm -rf "$target"; echo "source changed after final Gate B live verification" >&2; return 1; }
  rm -rf "$target"
}

production_render_release_inputs() {
  local bundle="$1" output="$2" source_root target profile_bin
  source_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
  target="$(mktemp -d "${TMPDIR:-/tmp}/bridge-driver-renderer.XXXXXX")"
  CARGO_TARGET_DIR="$target" cargo build --locked --quiet --release --manifest-path "$source_root/Cargo.toml" -p bridge-profile || { rm -rf "$target"; return 1; }
  profile_bin="$target/release/bridge-profile"
  "$profile_bin" render-release-inputs "$bundle/profile.json" "$output" >/dev/null || { rm -rf "$target"; return 1; }
  rm -rf "$target"
  [[ -f "$output/canister-init.json" && -f "$output/contract-constructor-args.json" && -f "$output/release-inputs-manifest.json" ]] || { echo "fixed profile renderer did not produce every release input" >&2; return 1; }
}
