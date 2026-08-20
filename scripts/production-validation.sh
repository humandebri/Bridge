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
import os, sys
staged, target = sys.argv[1:]
with open(staged, 'rb') as stream:
    os.fsync(stream.fileno())
os.replace(staged, target)
parent = os.path.dirname(os.path.abspath(target)) or '.'
fd = os.open(parent, os.O_RDONLY)
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
directory=os.open(source,os.O_RDONLY|os.O_DIRECTORY)
def read_regular(name,limit):
 if not re.fullmatch(r'[A-Za-z0-9][A-Za-z0-9._-]{0,127}',name): raise SystemExit(f'unsafe release artifact path: {name}')
 fd=os.open(name,os.O_RDONLY|os.O_NOFOLLOW,dir_fd=directory)
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
try:
 manifest_bytes=read_regular('release-manifest.json',4*1024*1024)
 manifest=json.loads(manifest_bytes)
 artifacts=manifest.get('artifacts')
 if not isinstance(artifacts,list): raise SystemExit('release manifest artifacts are missing')
 files={'release-manifest.json':manifest_bytes}
 for entry in artifacts:
  if not isinstance(entry,dict) or set(entry)!={'path','sha256'}: raise SystemExit('invalid release artifact entry')
  name=entry['path']; expected=entry['sha256']
  if name in files or not isinstance(expected,str) or not re.fullmatch(r'[0-9a-fA-F]{64}',expected): raise SystemExit(f'invalid duplicate release artifact: {name}')
  value=read_regular(name,256*1024*1024)
  if hashlib.sha256(value).hexdigest().lower()!=expected.lower(): raise SystemExit(f'release artifact hash mismatch while freezing: {name}')
  files[name]=value
 for name,value in files.items():
  fd=os.open(os.path.join(destination,name),os.O_WRONLY|os.O_CREAT|os.O_EXCL|os.O_NOFOLLOW,0o400)
  try:
   os.write(fd,value); os.fsync(fd)
  finally: os.close(fd)
 destination_fd=os.open(destination,os.O_RDONLY|os.O_DIRECTORY)
 try: os.fsync(destination_fd)
 finally: os.close(destination_fd)
 os.chmod(destination,0o500)
finally: os.close(directory)
PY
}

production_validate_gate() {
  local mode="$1" bundle="$2" expected_hash="$3"
  local source_root target profile_bin output actual_hash revision tree manifest_revision manifest_tree
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
  elif [[ "$mode" == gate-b ]]; then output="$("$profile_bin" verify-live "$bundle")" || { rm -rf "$target"; return 1; }
  else rm -rf "$target"; echo "invalid production gate mode" >&2; return 1
  fi
  rm -rf "$target"
  if [[ "$mode" == gate-a ]]; then
    [[ "$output" =~ ^gate_a=pass[[:space:]]authorizing=true[[:space:]]manifest_sha256=([0-9a-fA-F]{64})$ ]] || { echo "driver Gate A result is not authorizing" >&2; return 1; }
    actual_hash="${BASH_REMATCH[1]}"
  else
    actual_hash="$(printf '%s\n' "$output" | sed -nE 's/.*manifest_sha256=([0-9a-fA-F]{64}).*/\1/p' | tail -n 1)"
  fi
  [[ -n "$actual_hash" && "$(printf '%s' "$actual_hash" | tr '[:upper:]' '[:lower:]')" == "$(printf '%s' "$expected_hash" | tr '[:upper:]' '[:lower:]')" ]] || { echo "driver Gate manifest hash mismatch" >&2; return 1; }
  production_run_proof_gate "$source_root" "$manifest_revision" "$manifest_tree" || return 1
  "$source_root/scripts/rebuild-release-artifacts.sh" \
    "$bundle" "$manifest_revision" "$manifest_tree"
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
