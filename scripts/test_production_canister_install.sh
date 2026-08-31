#!/usr/bin/env bash
set -euo pipefail
ROOT="$(CDPATH='' cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd -P)"
T="$(mktemp -d "${TMPDIR:-/tmp}/bridge-production-install-test.XXXXXX")"
T="$(CDPATH='' cd -- "$T" && pwd -P)"
trap 'rm -rf "$T"' EXIT
mkdir -p "$T/source/scripts" "$T/source/.icp/data/mappings" "$T/source/canister/bridge-canister" "$T/bin" "$T/out"
cp "$ROOT/scripts/production-canister-install.sh" "$ROOT/scripts/production-validation.sh" \
  "$ROOT/scripts/test_production_canister_install.sh" "$T/source/scripts/"
chmod +x "$T/source/scripts/production-canister-install.sh"
printf 'service : () -> {}\n' >"$T/source/canister/bridge-canister/bridge.did"
printf '{"bridge-canister":"aaaaa-aa"}\n' >"$T/source/.icp/data/mappings/production.ids.json"
printf '[workspace]\nmembers=[]\n' >"$T/source/Cargo.toml"
printf '# fixture\n' >"$T/source/Cargo.lock"
printf '[toolchain]\nchannel = "1.97.0"\n' >"$T/source/rust-toolchain.toml"
git -C "$T/source" init -q
git -C "$T/source" config user.email bridge-test@example.invalid
git -C "$T/source" config user.name bridge-test
git -C "$T/source" add .
git -C "$T/source" commit -qm 'production install fixture'
INSTALLED_REVISION="$(git -C "$T/source" rev-parse HEAD)"
INSTALLED_TREE="$(git -C "$T/source" archive HEAD | shasum -a 256 | awk '{print $1}')"
git -C "$T/source" commit --allow-empty -qm 'equivalent ancestor source'
EQUIVALENT_REVISION="$(git -C "$T/source" rev-parse HEAD)"
printf wasm >"$T/bridge-canister.wasm"
WASM_SHA256="$(shasum -a 256 "$T/bridge-canister.wasm" | awk '{print $1}')"
printf '{"schema_version":2,"environment":"production","source_revision":"%s","source_tree_sha256":"%s","bridge_canister_id":"aaaaa-aa","bridge_canister_wasm_sha256":"%s","init":{}}\n' \
  "$INSTALLED_REVISION" "$INSTALLED_TREE" "$WASM_SHA256" >"$T/installed-plan.json"
printf '{"schema_version":2,"environment":"production","source_revision":"%s","source_tree_sha256":"%s","bridge_canister_id":"aaaaa-aa","bridge_canister_wasm_sha256":"%s","init":{}}\n' \
  "$EQUIVALENT_REVISION" "$INSTALLED_TREE" "$WASM_SHA256" >"$T/equivalent-plan.json"
FIXTURE_PLAN_SHA256="$(printf '%064d' 1)"
FIXTURE_INIT_CANDID_SHA256="$(printf '%064d' 2)"
FIXTURE_PLAN_FILE_SHA256="$(shasum -a 256 "$T/installed-plan.json" | awk '{print $1}')"
RECOVERY_RECEIPT="$T/out/recovery.json"
python3 - "$T/source/scripts/production-canister-install.sh" \
  "$INSTALLED_REVISION" "$INSTALLED_TREE" "$FIXTURE_PLAN_SHA256" \
  "$FIXTURE_PLAN_FILE_SHA256" "$FIXTURE_INIT_CANDID_SHA256" \
  "$WASM_SHA256" "$RECOVERY_RECEIPT" <<'PY'
import re, sys
path, revision, tree, plan, plan_file, candid, wasm, receipt = sys.argv[1:]
values = {
    "RECOVERY_INSTALLED_SOURCE_REVISION": revision,
    "RECOVERY_INSTALLED_SOURCE_TREE_SHA256": tree,
    "RECOVERY_PLAN_SHA256": plan,
    "RECOVERY_PLAN_FILE_SHA256": plan_file,
    "RECOVERY_INIT_CANDID_SHA256": candid,
    "RECOVERY_WASM_SHA256": wasm,
    "RECOVERY_CANISTER_ID": "aaaaa-aa",
    "RECOVERY_INSTALLER_PRINCIPAL": "aaaaa-aa",
    "RECOVERY_RECEIPT_PATH": receipt,
}
text = open(path, encoding="utf-8").read()
for name, value in values.items():
    text, count = re.subn(rf'^{name}="[^"]+"$', f'{name}="{value}"', text, flags=re.MULTILINE)
    if count != 1:
        raise SystemExit(f"failed to bind fixture {name}")
open(path, "w", encoding="utf-8").write(text)
PY
printf '# reviewed recovery-only source change\n' >>"$T/source/scripts/test_production_canister_install.sh"
git -C "$T/source" add scripts/production-canister-install.sh scripts/test_production_canister_install.sh
git -C "$T/source" commit -qm 'review post-install recovery'
REVISION="$(git -C "$T/source" rev-parse HEAD)"
TREE="$(git -C "$T/source" archive HEAD | shasum -a 256 | awk '{print $1}')"
printf '{"schema_version":2,"environment":"production","source_revision":"%s","source_tree_sha256":"%s","bridge_canister_id":"aaaaa-aa","bridge_canister_wasm_sha256":"%s","init":{}}\n' \
  "$REVISION" "$TREE" "$WASM_SHA256" >"$T/plan.json"

cat >"$T/bin/cargo" <<'SH'
#!/usr/bin/env bash
if [[ "${1:-}" == --version ]]; then printf 'cargo 1.97.0 (fixture)\n'; exit 0; fi
[[ -z "${RUSTC_WRAPPER+x}" && -z "${CC_wasm32_wasip1+x}" ]]
fixture_root="$(CDPATH='' cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd -P)"
manifest=""
while [[ "$#" -gt 0 ]]; do
  if [[ "$1" == --manifest-path && "$#" -ge 2 ]]; then manifest="$2"; shift 2; else shift; fi
done
[[ -f "$manifest" && "$manifest" != "$fixture_root/source"/* ]]
printf '%s\n' "$manifest" >>"$fixture_root/cargo-manifests"
mkdir -p "$CARGO_TARGET_DIR/release"
cp "$fixture_root/profile-fixture" "$CARGO_TARGET_DIR/release/bridge-profile"
chmod +x "$CARGO_TARGET_DIR/release/bridge-profile"
SH
cat >"$T/bin/rustc" <<'SH'
#!/usr/bin/env bash
[[ "${1:-}" == --version ]]
printf 'rustc 1.97.0 (fixture)\n'
SH
cat >"$T/profile-fixture" <<'SH'
#!/usr/bin/env bash
case "$1" in
  validate-production-canister-plan) exit 0 ;;
  render-production-canister-inputs)
    mkdir -p "$3"
    printf bin >"$3/canister-init.bin"
    printf '{"plan_sha256":"%064d","init_candid_sha256":"%064d"}\n' 1 2 \
      >"$3/production-canister-install-inputs.json"
    ;;
  storage-validation-complete)
    [[ "${FORCE_VALIDATION_DECODE_FAILURE:-false}" != true ]] || exit 41
    [[ "${FORCE_VALIDATION_MALFORMED:-false}" != true ]] || { printf 'unknown\n'; exit 0; }
    [[ "$2" == validation-complete ]] && printf 'true\n' || printf 'false\n'
    ;;
  storage-checksum-complete)
    [[ "${FORCE_CHECKSUM_DECODE_FAILURE:-false}" != true ]] || exit 42
    [[ "${FORCE_CHECKSUM_MALFORMED:-false}" != true ]] || { printf 'unknown\n'; exit 0; }
    [[ "$2" == checksum-complete ]] && printf 'true\n' || printf 'false\n'
    ;;
  write-production-canister-receipt)
    "$REAL_PYTHON3" -I - "$2" "${14}" "$FIXTURE_PLAN_SHA256" \
      "$FIXTURE_INIT_CANDID_SHA256" <<'PY'
import json, sys
plan_path, output, plan_sha256, init_candid_sha256 = sys.argv[1:]
plan = json.load(open(plan_path, encoding="utf-8"))
value = {
    "schema_version": 3,
    "plan_sha256": plan_sha256,
    "plan": plan,
    "source_revision": plan["source_revision"],
    "source_tree_sha256": plan["source_tree_sha256"],
    "init_candid_sha256": init_candid_sha256,
    "runtime_binding": {"expected_bridge_signer": "0x1111111111111111111111111111111111111111"},
    "governance_operator": "0x2222222222222222222222222222222222222222",
    "runtime_administrator": "0x3333333333333333333333333333333333333333",
    "independent_canceller": "0x4444444444444444444444444444444444444444",
}
with open(output, "w", encoding="utf-8") as handle:
    json.dump(value, handle, sort_keys=True, separators=(",", ":"))
    handle.write("\n")
PY
    touch "$RECEIPT_WRITTEN_MARKER"
    if [[ "${MUTATE_SOURCE_ON_RECEIPT_WRITE:-false}" == true && ! -e "$MUTATED_MARKER" ]]; then
      printf '# concurrent mutation before publish\n' >>"$MUTATION_TARGET"
      touch "$MUTATED_MARKER"
    fi
    ;;
  *) echo "unexpected bridge-profile command: $*" >&2; exit 90 ;;
esac
SH
chmod +x "$T/bin/cargo" "$T/bin/rustc" "$T/profile-fixture"

cat >"$T/bin/icp" <<'SH'
#!/usr/bin/env bash
printf 'icp %s\n' "$*" >>"$TRACE"
if [[ "$1" == --version ]]; then echo 'icp 1.0.2'; exit 0; fi
project_root=""
arguments=("$@")
for ((index = 0; index < ${#arguments[@]}; index++)); do
  if [[ "${arguments[$index]}" == --project-root-override \
    && $((index + 1)) -lt ${#arguments[@]} ]]; then
    project_root="${arguments[$((index + 1))]}"
    break
  fi
done
project_root_real="$(CDPATH='' cd -- "$project_root" 2>/dev/null && pwd -P)"
source_root_real="$(CDPATH='' cd -- "$SOURCE_FIXTURE_ROOT" && pwd -P)"
[[ -n "$project_root_real" && "$(pwd -P)" == "$project_root_real" \
  && "$project_root_real" != "$source_root_real" \
  && "$(dirname -- "$(dirname -- "$project_root_real")")" \
    == "$ICP_PROJECT_PARENT" ]] || {
  echo "icp project call was not executed from its project root" >&2
  exit 92
}
mkdir -p "$project_root_real/.icp/cache"
touch "$project_root_real/.icp/cache/fake-cli-write"
printf '%s\n' "$project_root_real" >>"$ICP_PROJECT_TRACE"
if [[ "$1 $2" == 'identity principal' ]]; then
  [[ "${IDENTITY_DRIFT:-false}" != true ]] && echo aaaaa-aa || echo bbbbb-bb
  exit 0
fi
if [[ "$1 $2" == 'canister status' ]]; then
  controller=aaaaa-aa
  [[ "${CONTROLLER_DRIFT:-false}" != true ]] || controller=bbbbb-bb
  if [[ "${LIVE_CONTROLLER_DRIFT_AFTER_RECEIPT_WRITE:-false}" == true \
    && -e "$RECEIPT_WRITTEN_MARKER" ]]; then controller=bbbbb-bb; fi
  if [[ "${LIVE_CONTROLLER_DRIFT_AFTER_PUBLICATION:-false}" == true \
    && -e "$LIVE_PUBLISHED_RECEIPT" ]]; then controller=bbbbb-bb; fi
  if [[ -e "$INSTALLED_MARKER" ]]; then
    module="$WASM_SHA256"
    [[ "${MODULE_DRIFT:-false}" != true ]] || module="$(printf '%064d' 9)"
    if [[ "${LIVE_MODULE_DRIFT_AFTER_RECEIPT_WRITE:-false}" == true \
      && -e "$RECEIPT_WRITTEN_MARKER" ]]; then module="$(printf '%064d' 8)"; fi
    if [[ "${LIVE_MODULE_DRIFT_AFTER_PUBLICATION:-false}" == true \
      && -e "$LIVE_PUBLISHED_RECEIPT" ]]; then module="$(printf '%064d' 7)"; fi
    printf '{"controllers":["%s"],"module_hash":"%s"}\n' "$controller" "$module"
  else
    printf '{"controllers":["%s"],"module_hash":null}\n' "$controller"
  fi
  exit 0
fi
if [[ "$1 $2" == 'canister install' ]]; then
  [[ " $* " == *' --mode install '* && " $* " == *' --args-format bin '* ]]
  [[ " $* " != *' reinstall '* && " $* " != *' --mode auto '* ]]
  touch "$INSTALLED_MARKER"
  [[ "${INSTALL_FAIL:-false}" != true ]] || exit 42
  exit 0
fi
if [[ "$1 $2" == 'canister call' ]]; then
  if [[ "${MUTATE_SOURCE_ON_CALL:-false}" == true && ! -e "$MUTATED_MARKER" ]]; then
    printf '# concurrent mutation\n' >>"$MUTATION_TARGET"
    touch "$MUTATED_MARKER"
  fi
  case " $* " in
    *' start_storage_validation '*) printf 'validation-pending\n' ;;
    *' continue_storage_validation '*)
      [[ " $* " == *' (100 : nat16) '* ]]
      printf 'validation-complete\n'
      ;;
    *' refresh_storage_checksum '*)
      [[ " $* " == *' (4194304 : nat64) '* ]]
      if [[ "${CHECKSUM_PENDING_ONCE:-false}" == true \
        && ! -e "$CHECKSUM_PENDING_MARKER" ]]; then
        touch "$CHECKSUM_PENDING_MARKER"
        printf 'checksum-pending\n'
      else
        printf 'checksum-complete\n'
      fi
      ;;
    *) printf '4449444c0000\n' ;;
  esac
  exit 0
fi
echo "unexpected icp command: $*" >&2
exit 91
SH
chmod +x "$T/bin/icp"

REAL_PYTHON3="$(command -v python3)"
cat >"$T/bin/python3" <<'SH'
#!/usr/bin/env bash
if [[ "${MUTATE_AFTER_RECOVERY_EVIDENCE:-false}" == true \
  && -e "$PUBLISHED_RECOVERY_EVIDENCE" && ! -e "$MUTATED_MARKER" ]]; then
  printf '# concurrent mutation after recovery evidence publication\n' >>"$MUTATION_TARGET"
  touch "$MUTATED_MARKER"
fi
if [[ "${SIMULATE_FOREIGN_OUTPUT_ANCESTOR:-false}" == true && "$#" -eq 5 \
  && "$1" == -I && "$2" == -S && "$3" == - \
  && "$5" == "$FOREIGN_OWNER_OUTPUT" ]]; then
  hook="$PYTHON_HOOK_DIR/foreign-owner-hook.$$"
  awk '
    /^    cursor_info = os\.stat\(cursor, follow_symlinks=False\)$/ {
      print
      print "    if cursor == os.path.dirname(real_parent):"
      print "        class ForeignStat: pass"
      print "        foreign = ForeignStat()"
      print "        foreign.st_dev = cursor_info.st_dev"
      print "        foreign.st_ino = cursor_info.st_ino"
      print "        foreign.st_mode = cursor_info.st_mode"
      print "        foreign.st_uid = os.getuid() + 1"
      print "        cursor_info = foreign"
      next
    }
    { print }
  ' >"$hook"
  touch "$FOREIGN_OWNER_HOOK_MARKER"
  exec "$REAL_PYTHON3" -I -S "$hook" "$4" "$5"
fi
if [[ "${MUTATE_STAGED_ON_LINK:-false}" == true && "$#" -eq 6 \
  && "$1" == -I && "$2" == -S && "$3" == - && "$5" == "$PUBLISH_TARGET" \
  && "$6" =~ ^[0-9a-f]{64}$ \
  && ! -e "$MUTATED_STAGE_MARKER" ]]; then
  hook="$PYTHON_HOOK_DIR/publish-hook.$$"
  awk '
    /^        os.link\(staged_name, target_name,/ {
      print "        os.rename(staged_name, staged_name + \".original\", src_dir_fd=directory, dst_dir_fd=directory)"
      print "        replacement = os.open(staged_name, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600, dir_fd=directory)"
      print "        os.write(replacement, b\"substituted staged artifact\\n\")"
      print "        os.close(replacement)"
    }
    { print }
  ' >"$hook"
  touch "$MUTATED_STAGE_MARKER"
  exec "$REAL_PYTHON3" -I -S "$hook" "$4" "$5" "$6"
fi
exec "$REAL_PYTHON3" "$@"
SH
chmod +x "$T/bin/python3"

export TRACE="$T/trace" PROFILE_FIXTURE="$T/profile-fixture" INSTALLED_MARKER="$T/installed" WASM_SHA256
export MUTATED_MARKER="$T/mutated" MUTATION_TARGET="$T/source/scripts/test_production_canister_install.sh"
export MUTATED_STAGE_MARKER="$T/mutated-stage" REAL_PYTHON3
export SOURCE_FIXTURE_ROOT="$T/source" CARGO_MANIFEST_TRACE="$T/cargo-manifests"
export ICP_PROJECT_PARENT="$T/out" ICP_PROJECT_TRACE="$T/icp-project-roots"
export RECEIPT_WRITTEN_MARKER="$T/receipt-written" PYTHON_HOOK_DIR="$T"
export FOREIGN_OWNER_HOOK_MARKER="$T/foreign-owner-hooked"
export CHECKSUM_PENDING_MARKER="$T/checksum-pending-once"
export FIXTURE_PLAN_SHA256 FIXTURE_INIT_CANDID_SHA256

if PATH="$T/bin:$PATH" BRIDGE_ICP_IDENTITY=production \
  "$T/source/scripts/production-canister-install.sh" --plan "$T/plan.json" \
  --wasm "$T/bridge-canister.wasm" --receipt "$T/source/receipt-inside.json" \
  >/dev/null 2>&1; then
  echo "production Canister installer accepted a receipt inside its source checkout" >&2
  exit 1
fi
[[ ! -e "$T/source/receipt-inside.json.reservation" ]]

CASE_ALIAS="$(python3 -I - "$T/source" <<'PY'
import os, sys
root = sys.argv[1]
alias = os.path.join(os.path.dirname(root), os.path.basename(root).swapcase())
if os.path.exists(alias) and os.path.samefile(root, alias) and alias != root:
    print(alias)
PY
)"
if [[ -n "$CASE_ALIAS" ]]; then
  if PATH="$T/bin:$PATH" BRIDGE_ICP_IDENTITY=production \
    "$T/source/scripts/production-canister-install.sh" --plan "$T/plan.json" \
    --wasm "$T/bridge-canister.wasm" --receipt "$CASE_ALIAS/receipt-inside-case.json" \
    >/dev/null 2>&1; then
    echo "production Canister installer accepted a case-aliased receipt inside its checkout" >&2
    exit 1
  fi
  [[ ! -e "$T/source/receipt-inside-case.json.reservation" ]]
fi

if [[ -d /Volumes/KINGSTON/KINIC/bridge-production-prep-d85b7ce ]]; then
  MOUNT_OUTPUT="$(mount)"
  case "$MOUNT_OUTPUT" in
    *' on /Volumes/KINGSTON '*'noowners'*)
      if PATH="$T/bin:$PATH" BRIDGE_ICP_IDENTITY=production \
        "$T/source/scripts/production-canister-install.sh" --plan "$T/plan.json" \
        --wasm "$T/bridge-canister.wasm" \
        --receipt "/Volumes/KINGSTON/KINIC/bridge-production-prep-d85b7ce/.ownership-test-$$.json" \
        >"$T/noowners.log" 2>&1; then
        echo "production Canister installer accepted a filesystem that ignores ownership" >&2
        exit 1
      fi
      grep -q 'filesystem must enforce file ownership' "$T/noowners.log"
      ;;
  esac
fi

export FOREIGN_OWNER_OUTPUT="$T/out/foreign-owner.json"
if PATH="$T/bin:$PATH" SIMULATE_FOREIGN_OUTPUT_ANCESTOR=true \
  BRIDGE_ICP_IDENTITY=production \
  "$T/source/scripts/production-canister-install.sh" --plan "$T/plan.json" \
  --wasm "$T/bridge-canister.wasm" --receipt "$FOREIGN_OWNER_OUTPUT" \
  >"$T/foreign-owner.log" 2>&1; then
  echo "production Canister installer accepted an ancestor owned by another user" >&2
  exit 1
fi
[[ -f "$FOREIGN_OWNER_HOOK_MARKER" \
  && ! -e "$FOREIGN_OWNER_OUTPUT.reservation" ]]
grep -q 'receipt ancestors must be owned by root or the executor' \
  "$T/foreign-owner.log"
rm -f "$FOREIGN_OWNER_HOOK_MARKER"
unset FOREIGN_OWNER_OUTPUT

git clone -q "$T/source" "$T/commondir-source"
printf '%s\n' "$T/source/.git" >"$T/commondir-source/.git/commondir"
if PATH="$T/bin:$PATH" BRIDGE_ICP_IDENTITY=production \
  "$T/commondir-source/scripts/production-canister-install.sh" --plan "$T/plan.json" \
  --wasm "$T/bridge-canister.wasm" --receipt "$T/out/commondir.json" \
  >/dev/null 2>&1; then
  echo "production Canister installer accepted an external Git common directory" >&2
  exit 1
fi
[[ ! -e "$T/out/commondir.json.reservation" ]]

mkdir -p "$T/source/.git/objects/info"
printf '%s\n' "$T/clean-alternate-objects" >"$T/source/.git/objects/info/alternates"
if PATH="$T/bin:$PATH" BRIDGE_ICP_IDENTITY=production \
  "$T/source/scripts/production-canister-install.sh" --plan "$T/plan.json" \
  --wasm "$T/bridge-canister.wasm" --receipt "$T/out/alternates.json" \
  >/dev/null 2>&1; then
  echo "production Canister installer accepted a Git object alternate" >&2
  exit 1
fi
[[ ! -e "$T/out/alternates.json.reservation" ]]
rm "$T/source/.git/objects/info/alternates"

mkdir -p "$T/source/tools/bridge-profile"
printf 'fn main() {}\n' >"$T/source/tools/bridge-profile/build.rs"
cp "$T/source/.git/info/exclude" "$T/exclude.backup"
printf 'tools/bridge-profile/build.rs\n' >>"$T/source/.git/info/exclude"
if PATH="$T/bin:$PATH" BRIDGE_ICP_IDENTITY=production \
  "$T/source/scripts/production-canister-install.sh" --plan "$T/plan.json" \
  --wasm "$T/bridge-canister.wasm" --receipt "$T/out/ignored-build-input.json" \
  >/dev/null 2>&1; then
  echo "production Canister installer accepted an ignored build input" >&2
  exit 1
fi
[[ ! -e "$T/out/ignored-build-input.json.reservation" ]]
cp "$T/exclude.backup" "$T/source/.git/info/exclude"
rm "$T/source/tools/bridge-profile/build.rs"
rmdir "$T/source/tools/bridge-profile" "$T/source/tools"

git clone -q "$T/source" "$T/clean-source"
printf '# hidden by inherited Git worktree override\n' >>"$T/source/Cargo.lock"
if PATH="$T/bin:$PATH" GIT_DIR="$T/clean-source/.git" GIT_WORK_TREE="$T/clean-source" \
  BRIDGE_ICP_IDENTITY=production \
  "$T/source/scripts/production-canister-install.sh" --plan "$T/plan.json" \
  --wasm "$T/bridge-canister.wasm" --receipt "$T/out/git-override.json" \
  >/dev/null 2>&1; then
  echo "production Canister installer trusted an inherited alternate Git worktree" >&2
  exit 1
fi
[[ ! -e "$T/out/git-override.json.reservation" ]]
git -C "$T/source" checkout -q -- Cargo.lock

git -C "$T/source" update-index --assume-unchanged Cargo.lock
if PATH="$T/bin:$PATH" BRIDGE_ICP_IDENTITY=production \
  "$T/source/scripts/production-canister-install.sh" --plan "$T/plan.json" \
  --wasm "$T/bridge-canister.wasm" --receipt "$T/out/assume-unchanged.json" \
  >/dev/null 2>&1; then
  echo "production Canister installer accepted an assume-unchanged index entry" >&2
  exit 1
fi
[[ ! -e "$T/out/assume-unchanged.json.reservation" ]]
git -C "$T/source" update-index --no-assume-unchanged Cargo.lock

git -C "$T/source" update-index --skip-worktree Cargo.lock
if PATH="$T/bin:$PATH" BRIDGE_ICP_IDENTITY=production \
  "$T/source/scripts/production-canister-install.sh" --plan "$T/plan.json" \
  --wasm "$T/bridge-canister.wasm" --receipt "$T/out/skip-worktree.json" \
  >/dev/null 2>&1; then
  echo "production Canister installer accepted a skip-worktree index entry" >&2
  exit 1
fi
[[ ! -e "$T/out/skip-worktree.json.reservation" ]]
git -C "$T/source" update-index --no-skip-worktree Cargo.lock

cat >"$T/bin/fsmonitor-hook" <<'SH'
#!/usr/bin/env bash
[[ "$1" == 2 ]]
printf 'test-token\0'
SH
chmod +x "$T/bin/fsmonitor-hook"
git -C "$T/source" config core.fsmonitor "$T/bin/fsmonitor-hook"
git -C "$T/source" update-index --fsmonitor
git -C "$T/source" update-index --fsmonitor-valid Cargo.lock
python3 - "$T/source/.git/index" <<'PY'
import sys
if b"FSMN" not in open(sys.argv[1], "rb").read():
    raise SystemExit(1)
PY
if PATH="$T/bin:$PATH" BRIDGE_ICP_IDENTITY=production \
  "$T/source/scripts/production-canister-install.sh" --plan "$T/plan.json" \
  --wasm "$T/bridge-canister.wasm" --receipt "$T/out/fsmonitor-valid.json" \
  >/dev/null 2>&1; then
  echo "production Canister installer accepted fsmonitor index state" >&2
  exit 1
fi
[[ ! -e "$T/out/fsmonitor-valid.json.reservation" ]] || {
  echo "failed to establish the fsmonitor-valid test precondition" >&2
  exit 1
}
git -C "$T/source" config --unset core.fsmonitor
git -C "$T/source" update-index --no-fsmonitor-valid Cargo.lock
git -C "$T/source" update-index --no-fsmonitor

if PATH="$T/bin:$PATH" INSTALL_FAIL=true BRIDGE_ICP_IDENTITY=production \
  "$T/source/scripts/production-canister-install.sh" --plan "$T/plan.json" \
  --wasm "$T/bridge-canister.wasm" --receipt "$T/out/failed.json" \
  >"$T/install-failure.log" 2>&1; then
  echo "production Canister installer accepted an ambiguous install failure" >&2
  exit 1
fi
[[ -f "$T/out/failed.json.reservation" && ! -e "$T/out/failed.json" ]]
[[ -e "$INSTALLED_MARKER" ]] || {
  cat "$T/install-failure.log" >&2
  echo "ambiguous install failure did not reach the install call" >&2
  exit 1
}
rm "$INSTALLED_MARKER"
CONTINUE_CALLS_BEFORE="$(grep -c 'continue_storage_validation' "$TRACE" || true)"
if PATH="$T/bin:$PATH" FORCE_VALIDATION_DECODE_FAILURE=true BRIDGE_ICP_IDENTITY=production \
  "$T/source/scripts/production-canister-install.sh" --plan "$T/plan.json" \
  --wasm "$T/bridge-canister.wasm" --receipt "$T/out/validation-failed.json" >/dev/null 2>&1; then
  echo "production Canister installer retried an invalid storage validation response" >&2
  exit 1
fi
[[ -f "$T/out/validation-failed.json.reservation" && ! -e "$T/out/validation-failed.json" ]]
[[ "$(grep -c 'continue_storage_validation' "$TRACE" || true)" == "$CONTINUE_CALLS_BEFORE" ]]
rm "$INSTALLED_MARKER"
if PATH="$T/bin:$PATH" MUTATE_SOURCE_ON_RECEIPT_WRITE=true \
  MUTATION_TARGET="$T/source/scripts/production-canister-install.sh" \
  BRIDGE_ICP_IDENTITY=production \
  "$T/source/scripts/production-canister-install.sh" --plan "$T/plan.json" \
  --wasm "$T/bridge-canister.wasm" --receipt "$T/out/normal-source-mutated.json" \
  >/dev/null 2>&1; then
  echo "production Canister installer published a receipt after its source changed" >&2
  exit 1
fi
[[ -f "$T/out/normal-source-mutated.json.reservation" \
  && ! -e "$T/out/normal-source-mutated.json" ]]
git -C "$T/source" checkout -q -- scripts/production-canister-install.sh
rm -f "$MUTATED_MARKER" "$INSTALLED_MARKER"
if PATH="$T/bin:$PATH" MUTATE_STAGED_ON_LINK=true \
  PUBLISH_TARGET="$T/out/normal-staged-mutated.json" \
  BRIDGE_ICP_IDENTITY=production \
  "$T/source/scripts/production-canister-install.sh" --plan "$T/plan.json" \
  --wasm "$T/bridge-canister.wasm" --receipt "$T/out/normal-staged-mutated.json" \
  >"$T/normal-stage-race.log" 2>&1; then
  echo "production Canister installer published a substituted staged receipt" >&2
  exit 1
fi
[[ -f "$MUTATED_STAGE_MARKER" ]]
grep -q 'published artifact is not the verified staged inode' \
  "$T/normal-stage-race.log"
[[ -f "$T/out/normal-staged-mutated.json.reservation" \
  && ! -e "$T/out/normal-staged-mutated.json" ]]
rm -f "$MUTATED_STAGE_MARKER" "$INSTALLED_MARKER"
rm -f "$RECEIPT_WRITTEN_MARKER"
if PATH="$T/bin:$PATH" LIVE_MODULE_DRIFT_AFTER_RECEIPT_WRITE=true \
  BRIDGE_ICP_IDENTITY=production \
  "$T/source/scripts/production-canister-install.sh" --plan "$T/plan.json" \
  --wasm "$T/bridge-canister.wasm" --receipt "$T/out/normal-publication-drift.json" \
  >/dev/null 2>&1; then
  echo "production Canister installer accepted module drift before receipt publication" >&2
  exit 1
fi
[[ -f "$T/out/normal-publication-drift.json.reservation" \
  && ! -e "$T/out/normal-publication-drift.json" ]]
rm -f "$T/out/normal-publication-drift.json.reservation" \
  "$INSTALLED_MARKER" "$RECEIPT_WRITTEN_MARKER"

if PATH="$T/bin:$PATH" LIVE_CONTROLLER_DRIFT_AFTER_PUBLICATION=true \
  LIVE_PUBLISHED_RECEIPT="$T/out/normal-cleanup-drift.json" \
  BRIDGE_ICP_IDENTITY=production \
  "$T/source/scripts/production-canister-install.sh" --plan "$T/plan.json" \
  --wasm "$T/bridge-canister.wasm" --receipt "$T/out/normal-cleanup-drift.json" \
  >/dev/null 2>&1; then
  echo "production Canister installer removed its reservation after cleanup-time controller drift" >&2
  exit 1
fi
[[ -s "$T/out/normal-cleanup-drift.json" \
  && -f "$T/out/normal-cleanup-drift.json.reservation" ]]
INSTALL_CALLS_BEFORE_PARTIAL_REPLAY="$(grep -c 'canister install ' "$TRACE")"
if PATH="$T/bin:$PATH" BRIDGE_ICP_IDENTITY=production \
  "$T/source/scripts/production-canister-install.sh" --plan "$T/plan.json" \
  --wasm "$T/bridge-canister.wasm" --receipt "$T/out/normal-cleanup-drift.json" \
  >/dev/null 2>&1; then
  echo "production Canister installer replayed a receipt-plus-reservation partial state" >&2
  exit 1
fi
[[ "$(grep -c 'canister install ' "$TRACE")" == "$INSTALL_CALLS_BEFORE_PARTIAL_REPLAY" ]]
rm -f "$T/out/normal-cleanup-drift.json" "$T/out/normal-cleanup-drift.json.reservation" \
  "$INSTALLED_MARKER" "$RECEIPT_WRITTEN_MARKER"

PATH="$T/bin:$PATH" RUSTC_WRAPPER=/hostile/rustc-wrapper \
  CC_wasm32_wasip1=/hostile/wasm-cc BRIDGE_ICP_IDENTITY=production \
  "$T/source/scripts/production-canister-install.sh" --plan "$T/plan.json" \
  --wasm "$T/bridge-canister.wasm" --receipt "$T/out/receipt.json" >/dev/null
[[ -s "$T/out/receipt.json" ]]
[[ ! -e "$T/out/receipt.json.reservation" ]]
NORMAL_INSTALL_CALLS="$(grep -c 'canister install ' "$TRACE")"
python3 -I - "$T/out/receipt.json" "$REVISION" "$TREE" \
  "$FIXTURE_PLAN_SHA256" <<'PY'
import json, sys
value = json.load(open(sys.argv[1], encoding="utf-8"))
assert value["schema_version"] == 3
assert value["source_revision"] == sys.argv[2]
assert value["source_tree_sha256"] == sys.argv[3]
assert value["plan_sha256"] == sys.argv[4]
roles = [
    value["runtime_binding"]["expected_bridge_signer"],
    value["governance_operator"],
    value["runtime_administrator"],
    value["independent_canceller"],
]
assert len(set(roles)) == 4
assert all(len(role) == 42 and role.startswith("0x") for role in roles)
PY
grep -q 'canister install aaaaa-aa -n ic --mode install' "$TRACE"
grep -q 'continue_storage_validation (100 : nat16)' "$TRACE"
grep -q 'refresh_storage_checksum (4194304 : nat64)' "$TRACE"
if grep -Eq 'continue_storage_validation \(1000 : nat16\)|refresh_storage_checksum \(10485760 : nat64\)' "$TRACE"; then
  echo "production Canister installer used an unsupported maintenance chunk" >&2
  exit 1
fi
if grep -Eq 'reinstall|--mode auto' "$TRACE"; then
  echo "production Canister installer used a forbidden install mode" >&2
  exit 1
fi

rm "$T/out/receipt.json"
if PATH="$T/bin:$PATH" BRIDGE_ICP_IDENTITY=production \
  "$T/source/scripts/production-canister-install.sh" --plan "$T/plan.json" \
  --wasm "$T/bridge-canister.wasm" --receipt "$T/out/receipt.json" >/dev/null 2>&1; then
  echo "production Canister installer accepted an already-installed module" >&2
  exit 1
fi
[[ "$(grep -c 'canister install ' "$TRACE")" == "$NORMAL_INSTALL_CALLS" ]]

make_recovery_reservation() {
  [[ ! -e "$1" && ! -e "$1.reservation" \
    && ! -e "$1.recovery.json" && ! -e "$1.recovery.json.reservation" ]]
  : >"$1.reservation"
  chmod 600 "$1.reservation"
}

clear_failed_recovery() {
  rm -f "$1" "$1.reservation" "$1.recovery.json" "$1.recovery.json.reservation"
}

INSTALL_CALLS_BEFORE_RECOVERY="$(grep -c 'canister install ' "$TRACE")"
make_recovery_reservation "$T/out/alternate-recovery.json"
if PATH="$T/bin:$PATH" BRIDGE_ICP_IDENTITY=production \
  "$T/source/scripts/production-canister-install.sh" --resume-post-install \
  --plan "$T/installed-plan.json" --wasm "$T/bridge-canister.wasm" \
  --receipt "$T/out/alternate-recovery.json" >/dev/null 2>&1; then
  echo "post-install recovery accepted an alternate incident receipt path" >&2
  exit 1
fi
[[ -f "$T/out/alternate-recovery.json.reservation" \
  && ! -e "$RECOVERY_RECEIPT.recovery.json.reservation" ]]
clear_failed_recovery "$T/out/alternate-recovery.json"

REPLACE_SOURCE="$(printf replace-source | git -C "$T/source" hash-object -w --stdin)"
REPLACE_TARGET="$(printf replace-target | git -C "$T/source" hash-object -w --stdin)"
git -C "$T/source" replace "$REPLACE_SOURCE" "$REPLACE_TARGET"
make_recovery_reservation "$RECOVERY_RECEIPT"
if PATH="$T/bin:$PATH" BRIDGE_ICP_IDENTITY=production \
  "$T/source/scripts/production-canister-install.sh" --resume-post-install \
  --plan "$T/installed-plan.json" --wasm "$T/bridge-canister.wasm" \
  --receipt "$RECOVERY_RECEIPT" >/dev/null 2>&1; then
  echo "post-install recovery accepted a Git replacement object" >&2
  exit 1
fi
[[ -f "$RECOVERY_RECEIPT.reservation" \
  && ! -e "$RECOVERY_RECEIPT.recovery.json.reservation" ]]
git -C "$T/source" replace -d "$REPLACE_SOURCE" >/dev/null
clear_failed_recovery "$RECOVERY_RECEIPT"

make_recovery_reservation "$RECOVERY_RECEIPT"
if PATH="$T/bin:$PATH" BRIDGE_ICP_IDENTITY=production \
  "$T/source/scripts/production-canister-install.sh" --resume-post-install \
  --plan "$T/equivalent-plan.json" --wasm "$T/bridge-canister.wasm" \
  --receipt "$RECOVERY_RECEIPT" >/dev/null 2>&1; then
  echo "post-install recovery accepted an unapproved equivalent-tree ancestor plan" >&2
  exit 1
fi
[[ -f "$RECOVERY_RECEIPT.reservation" \
  && ! -e "$RECOVERY_RECEIPT.recovery.json.reservation" ]]
clear_failed_recovery "$RECOVERY_RECEIPT"

make_recovery_reservation "$RECOVERY_RECEIPT"
if PATH="$T/bin:$PATH" IDENTITY_DRIFT=true BRIDGE_ICP_IDENTITY=production \
  "$T/source/scripts/production-canister-install.sh" --resume-post-install \
  --plan "$T/installed-plan.json" --wasm "$T/bridge-canister.wasm" \
  --receipt "$RECOVERY_RECEIPT" >/dev/null 2>&1; then
  echo "post-install recovery accepted a substituted installer principal" >&2
  exit 1
fi
[[ -f "$RECOVERY_RECEIPT.reservation" \
  && ! -e "$RECOVERY_RECEIPT.recovery.json.reservation" ]]
clear_failed_recovery "$RECOVERY_RECEIPT"

if PATH="$T/bin:$PATH" BRIDGE_ICP_IDENTITY=production \
  "$T/source/scripts/production-canister-install.sh" --resume-post-install \
  --plan "$T/installed-plan.json" --wasm "$T/bridge-canister.wasm" \
  --receipt "$RECOVERY_RECEIPT" >/dev/null 2>&1; then
  echo "post-install recovery accepted a missing canonical reservation" >&2
  exit 1
fi
[[ ! -e "$RECOVERY_RECEIPT.recovery.json.reservation" ]]

printf x >"$RECOVERY_RECEIPT.reservation"
chmod 600 "$RECOVERY_RECEIPT.reservation"
if PATH="$T/bin:$PATH" BRIDGE_ICP_IDENTITY=production \
  "$T/source/scripts/production-canister-install.sh" --resume-post-install \
  --plan "$T/installed-plan.json" --wasm "$T/bridge-canister.wasm" \
  --receipt "$RECOVERY_RECEIPT" >/dev/null 2>&1; then
  echo "post-install recovery accepted a non-empty canonical reservation" >&2
  exit 1
fi
clear_failed_recovery "$RECOVERY_RECEIPT"

: >"$T/out/reservation-target"
chmod 600 "$T/out/reservation-target"
ln -s "$T/out/reservation-target" "$RECOVERY_RECEIPT.reservation"
if PATH="$T/bin:$PATH" BRIDGE_ICP_IDENTITY=production \
  "$T/source/scripts/production-canister-install.sh" --resume-post-install \
  --plan "$T/installed-plan.json" --wasm "$T/bridge-canister.wasm" \
  --receipt "$RECOVERY_RECEIPT" >/dev/null 2>&1; then
  echo "post-install recovery followed a reservation symlink" >&2
  exit 1
fi
rm "$RECOVERY_RECEIPT.reservation" "$T/out/reservation-target"

make_recovery_reservation "$RECOVERY_RECEIPT"
: >"$RECOVERY_RECEIPT.recovery.json.reservation"
chmod 600 "$RECOVERY_RECEIPT.recovery.json.reservation"
if PATH="$T/bin:$PATH" BRIDGE_ICP_IDENTITY=production \
  "$T/source/scripts/production-canister-install.sh" --resume-post-install \
  --plan "$T/installed-plan.json" --wasm "$T/bridge-canister.wasm" \
  --receipt "$RECOVERY_RECEIPT" >/dev/null 2>&1; then
  echo "post-install recovery accepted a concurrent recovery reservation" >&2
  exit 1
fi
clear_failed_recovery "$RECOVERY_RECEIPT"

make_recovery_reservation "$RECOVERY_RECEIPT"
if PATH="$T/bin:$PATH" MODULE_DRIFT=true BRIDGE_ICP_IDENTITY=production \
  "$T/source/scripts/production-canister-install.sh" --resume-post-install \
  --plan "$T/installed-plan.json" --wasm "$T/bridge-canister.wasm" \
  --receipt "$RECOVERY_RECEIPT" >/dev/null 2>&1; then
  echo "post-install recovery accepted module drift" >&2
  exit 1
fi
[[ -f "$RECOVERY_RECEIPT.reservation" \
  && -f "$RECOVERY_RECEIPT.recovery.json.reservation" ]]
clear_failed_recovery "$RECOVERY_RECEIPT"

make_recovery_reservation "$RECOVERY_RECEIPT"
if PATH="$T/bin:$PATH" CONTROLLER_DRIFT=true BRIDGE_ICP_IDENTITY=production \
  "$T/source/scripts/production-canister-install.sh" --resume-post-install \
  --plan "$T/installed-plan.json" --wasm "$T/bridge-canister.wasm" \
  --receipt "$RECOVERY_RECEIPT" >/dev/null 2>&1; then
  echo "post-install recovery accepted controller drift" >&2
  exit 1
fi
[[ -f "$RECOVERY_RECEIPT.reservation" \
  && -f "$RECOVERY_RECEIPT.recovery.json.reservation" ]]
clear_failed_recovery "$RECOVERY_RECEIPT"

rm -f "$RECEIPT_WRITTEN_MARKER"
make_recovery_reservation "$RECOVERY_RECEIPT"
if PATH="$T/bin:$PATH" LIVE_MODULE_DRIFT_AFTER_RECEIPT_WRITE=true \
  BRIDGE_ICP_IDENTITY=production \
  "$T/source/scripts/production-canister-install.sh" --resume-post-install \
  --plan "$T/installed-plan.json" --wasm "$T/bridge-canister.wasm" \
  --receipt "$RECOVERY_RECEIPT" >/dev/null 2>&1; then
  echo "post-install recovery accepted module drift before artifact publication" >&2
  exit 1
fi
[[ ! -e "$RECOVERY_RECEIPT" && ! -e "$RECOVERY_RECEIPT.recovery.json" ]]
[[ -f "$RECOVERY_RECEIPT.reservation" \
  && -f "$RECOVERY_RECEIPT.recovery.json.reservation" ]]
rm -f "$RECEIPT_WRITTEN_MARKER"
clear_failed_recovery "$RECOVERY_RECEIPT"

make_recovery_reservation "$RECOVERY_RECEIPT"
if PATH="$T/bin:$PATH" LIVE_CONTROLLER_DRIFT_AFTER_PUBLICATION=true \
  LIVE_PUBLISHED_RECEIPT="$RECOVERY_RECEIPT" BRIDGE_ICP_IDENTITY=production \
  "$T/source/scripts/production-canister-install.sh" --resume-post-install \
  --plan "$T/installed-plan.json" --wasm "$T/bridge-canister.wasm" \
  --receipt "$RECOVERY_RECEIPT" >/dev/null 2>&1; then
  echo "post-install recovery removed reservations after cleanup-time controller drift" >&2
  exit 1
fi
[[ -s "$RECOVERY_RECEIPT" && -s "$RECOVERY_RECEIPT.recovery.json" ]]
[[ -f "$RECOVERY_RECEIPT.reservation" \
  && -f "$RECOVERY_RECEIPT.recovery.json.reservation" ]]
TRACE_LINES_BEFORE_PARTIAL_REPLAY="$(wc -l <"$TRACE" | tr -d ' ')"
if PATH="$T/bin:$PATH" BRIDGE_ICP_IDENTITY=production \
  "$T/source/scripts/production-canister-install.sh" --resume-post-install \
  --plan "$T/installed-plan.json" --wasm "$T/bridge-canister.wasm" \
  --receipt "$RECOVERY_RECEIPT" >/dev/null 2>&1; then
  echo "post-install recovery replayed a receipt-plus-evidence partial state" >&2
  exit 1
fi
[[ "$(wc -l <"$TRACE" | tr -d ' ')" == "$TRACE_LINES_BEFORE_PARTIAL_REPLAY" ]]
clear_failed_recovery "$RECOVERY_RECEIPT"

CONTINUE_CALLS_BEFORE="$(grep -c 'continue_storage_validation' "$TRACE" || true)"
make_recovery_reservation "$RECOVERY_RECEIPT"
if PATH="$T/bin:$PATH" FORCE_VALIDATION_DECODE_FAILURE=true BRIDGE_ICP_IDENTITY=production \
  "$T/source/scripts/production-canister-install.sh" --resume-post-install \
  --plan "$T/installed-plan.json" --wasm "$T/bridge-canister.wasm" \
  --receipt "$RECOVERY_RECEIPT" >/dev/null 2>&1; then
  echo "post-install recovery accepted a validation decoder failure" >&2
  exit 1
fi
[[ "$(grep -c 'continue_storage_validation' "$TRACE" || true)" == "$CONTINUE_CALLS_BEFORE" ]]
[[ -f "$RECOVERY_RECEIPT.reservation" \
  && -f "$RECOVERY_RECEIPT.recovery.json.reservation" ]]
clear_failed_recovery "$RECOVERY_RECEIPT"

make_recovery_reservation "$RECOVERY_RECEIPT"
if PATH="$T/bin:$PATH" FORCE_VALIDATION_MALFORMED=true BRIDGE_ICP_IDENTITY=production \
  "$T/source/scripts/production-canister-install.sh" --resume-post-install \
  --plan "$T/installed-plan.json" --wasm "$T/bridge-canister.wasm" \
  --receipt "$RECOVERY_RECEIPT" >/dev/null 2>&1; then
  echo "post-install recovery accepted malformed validation output" >&2
  exit 1
fi
clear_failed_recovery "$RECOVERY_RECEIPT"

CHECKSUM_CALLS_BEFORE="$(grep -c 'refresh_storage_checksum' "$TRACE" || true)"
make_recovery_reservation "$RECOVERY_RECEIPT"
if PATH="$T/bin:$PATH" FORCE_CHECKSUM_DECODE_FAILURE=true BRIDGE_ICP_IDENTITY=production \
  "$T/source/scripts/production-canister-install.sh" --resume-post-install \
  --plan "$T/installed-plan.json" --wasm "$T/bridge-canister.wasm" \
  --receipt "$RECOVERY_RECEIPT" >/dev/null 2>&1; then
  echo "post-install recovery accepted a checksum decoder failure" >&2
  exit 1
fi
[[ "$(grep -c 'refresh_storage_checksum' "$TRACE" || true)" -eq $((CHECKSUM_CALLS_BEFORE + 1)) ]]
[[ -f "$RECOVERY_RECEIPT.reservation" \
  && -f "$RECOVERY_RECEIPT.recovery.json.reservation" ]]
clear_failed_recovery "$RECOVERY_RECEIPT"

make_recovery_reservation "$RECOVERY_RECEIPT"
if PATH="$T/bin:$PATH" FORCE_CHECKSUM_MALFORMED=true BRIDGE_ICP_IDENTITY=production \
  "$T/source/scripts/production-canister-install.sh" --resume-post-install \
  --plan "$T/installed-plan.json" --wasm "$T/bridge-canister.wasm" \
  --receipt "$RECOVERY_RECEIPT" >/dev/null 2>&1; then
  echo "post-install recovery accepted malformed checksum output" >&2
  exit 1
fi
clear_failed_recovery "$RECOVERY_RECEIPT"

make_recovery_reservation "$RECOVERY_RECEIPT"
mkdir -p "$T/hostile-python"
cat >"$T/hostile-python/hashlib.py" <<'PY'
import os
open(os.environ["HOSTILE_IMPORT_MARKER"], "w").write("hashlib imported\n")
raise RuntimeError("hostile hashlib imported")
PY
cat >"$T/hostile-python/json.py" <<'PY'
import os
open(os.environ["HOSTILE_IMPORT_MARKER"], "w").write("json imported\n")
raise RuntimeError("hostile json imported")
PY
CHECKSUM_CALLS_BEFORE_SUCCESS="$(grep -c 'refresh_storage_checksum' "$TRACE" || true)"
(
  CDPATH='' cd -- "$T/hostile-python"
  PATH="$T/bin:$PATH" PYTHONPATH="$T/hostile-python" \
    CHECKSUM_PENDING_ONCE=true \
    HOSTILE_IMPORT_MARKER="$T/hostile-imported" BRIDGE_ICP_IDENTITY=production \
    "$T/source/scripts/production-canister-install.sh" --resume-post-install \
    --plan "$T/installed-plan.json" --wasm "$T/bridge-canister.wasm" \
    --receipt "$RECOVERY_RECEIPT" >/dev/null
)
[[ ! -e "$T/hostile-imported" ]]
[[ -s "$ICP_PROJECT_TRACE" ]]
if grep -Fxq "$SOURCE_FIXTURE_ROOT" "$ICP_PROJECT_TRACE"; then
  echo "production Canister installer exposed its source checkout as mutable ICP state" >&2
  exit 1
fi
[[ -e "$CHECKSUM_PENDING_MARKER" ]]
[[ "$(grep -c 'refresh_storage_checksum' "$TRACE" || true)" \
  -eq $((CHECKSUM_CALLS_BEFORE_SUCCESS + 2)) ]]
[[ -s "$RECOVERY_RECEIPT" && -s "$RECOVERY_RECEIPT.recovery.json" ]]
[[ ! -e "$RECOVERY_RECEIPT.reservation" \
  && ! -e "$RECOVERY_RECEIPT.recovery.json.reservation" ]]
[[ "$(grep -c 'canister install ' "$TRACE")" == "$INSTALL_CALLS_BEFORE_RECOVERY" ]]
EXPECTED_DRIVER_SHA256="$(git -C "$T/source" cat-file blob \
  "$REVISION:scripts/production-canister-install.sh" | shasum -a 256 | awk '{print $1}')"
python3 -I - "$RECOVERY_RECEIPT" "$RECOVERY_RECEIPT.recovery.json" \
  "$INSTALLED_REVISION" "$INSTALLED_TREE" "$REVISION" "$TREE" \
  "$EXPECTED_DRIVER_SHA256" <<'PY'
import hashlib, json, re, sys
(
    receipt_path, evidence_path, installed_revision, installed_tree,
    recovery_revision, recovery_tree, expected_driver_sha256,
) = sys.argv[1:]
receipt = open(receipt_path, "rb").read()
evidence = json.load(open(evidence_path, encoding="utf-8"))
receipt_value = json.loads(receipt)
assert receipt_value["source_revision"] == installed_revision
assert receipt_value["source_tree_sha256"] == installed_tree
assert evidence["schema_version"] == 1
assert evidence["kind"] == "production-canister-post-install-recovery"
assert evidence["installed_source_revision"] == installed_revision
assert evidence["installed_source_tree_sha256"] == installed_tree
assert evidence["recovery_source_revision"] == recovery_revision
assert evidence["recovery_source_tree_sha256"] == recovery_tree
assert evidence["recovery_driver_sha256"] == expected_driver_sha256
assert evidence["storage_validation_max_rows"] == 100
assert evidence["storage_checksum_max_bytes"] == 4194304
assert evidence["pre_publication_controllers"] == ["aaaaa-aa"]
assert evidence["pre_publication_module_sha256"] == evidence["bridge_canister_wasm_sha256"]
assert evidence["receipt_sha256"] == hashlib.sha256(receipt).hexdigest()
assert set(evidence["response_sha256"]) == {
    "initialize_public_config", "storage_validation", "storage_checksum",
    "runtime_binding", "operational_config", "control_plane_addresses",
    "bridge_status", "production_lifecycle", "storage_integrity", "management_status",
    "pre_publication_management_status",
}
assert all(re.fullmatch(r"[0-9a-f]{64}", value) for value in evidence["response_sha256"].values())
PY
TRACE_LINES_BEFORE_SUCCESS_REPLAY="$(wc -l <"$TRACE" | tr -d ' ')"
if PATH="$T/bin:$PATH" BRIDGE_ICP_IDENTITY=production \
  "$T/source/scripts/production-canister-install.sh" --resume-post-install \
  --plan "$T/installed-plan.json" --wasm "$T/bridge-canister.wasm" \
  --receipt "$RECOVERY_RECEIPT" >/dev/null 2>&1; then
  echo "completed post-install recovery was replayed at its canonical path" >&2
  exit 1
fi
[[ "$(wc -l <"$TRACE" | tr -d ' ')" == "$TRACE_LINES_BEFORE_SUCCESS_REPLAY" ]]
make_recovery_reservation "$T/out/alternate-after-success.json"
if PATH="$T/bin:$PATH" BRIDGE_ICP_IDENTITY=production \
  "$T/source/scripts/production-canister-install.sh" --resume-post-install \
  --plan "$T/installed-plan.json" --wasm "$T/bridge-canister.wasm" \
  --receipt "$T/out/alternate-after-success.json" >/dev/null 2>&1; then
  echo "completed post-install recovery was replayed through an alternate receipt path" >&2
  exit 1
fi
clear_failed_recovery "$T/out/alternate-after-success.json"
clear_failed_recovery "$RECOVERY_RECEIPT"

make_recovery_reservation "$RECOVERY_RECEIPT"
if PATH="$T/bin:$PATH" MUTATE_SOURCE_ON_CALL=true BRIDGE_ICP_IDENTITY=production \
  "$T/source/scripts/production-canister-install.sh" --resume-post-install \
  --plan "$T/installed-plan.json" --wasm "$T/bridge-canister.wasm" \
  --receipt "$RECOVERY_RECEIPT" >/dev/null 2>&1; then
  echo "post-install recovery wrote evidence after its source changed" >&2
  exit 1
fi
[[ ! -e "$RECOVERY_RECEIPT" && ! -e "$RECOVERY_RECEIPT.recovery.json" ]]
[[ -f "$RECOVERY_RECEIPT.reservation" \
  && -f "$RECOVERY_RECEIPT.recovery.json.reservation" ]]
git -C "$T/source" checkout -q -- scripts/test_production_canister_install.sh
rm -f "$MUTATED_MARKER"
clear_failed_recovery "$RECOVERY_RECEIPT"

make_recovery_reservation "$RECOVERY_RECEIPT"
if PATH="$T/bin:$PATH" MUTATE_SOURCE_ON_RECEIPT_WRITE=true \
  MUTATION_TARGET="$T/source/scripts/production-canister-install.sh" \
  BRIDGE_ICP_IDENTITY=production \
  "$T/source/scripts/production-canister-install.sh" --resume-post-install \
  --plan "$T/installed-plan.json" --wasm "$T/bridge-canister.wasm" \
  --receipt "$RECOVERY_RECEIPT" >/dev/null 2>&1; then
  echo "post-install recovery published artifacts after its source changed" >&2
  exit 1
fi
[[ ! -e "$RECOVERY_RECEIPT" \
  && ! -e "$RECOVERY_RECEIPT.recovery.json" ]]
[[ -f "$RECOVERY_RECEIPT.reservation" \
  && -f "$RECOVERY_RECEIPT.recovery.json.reservation" ]]
git -C "$T/source" checkout -q -- scripts/production-canister-install.sh
rm -f "$MUTATED_MARKER"
clear_failed_recovery "$RECOVERY_RECEIPT"

make_recovery_reservation "$RECOVERY_RECEIPT"
if PATH="$T/bin:$PATH" MUTATE_AFTER_RECOVERY_EVIDENCE=true \
  PUBLISHED_RECOVERY_EVIDENCE="$RECOVERY_RECEIPT.recovery.json" \
  MUTATION_TARGET="$T/source/scripts/production-canister-install.sh" \
  BRIDGE_ICP_IDENTITY=production \
  "$T/source/scripts/production-canister-install.sh" --resume-post-install \
  --plan "$T/installed-plan.json" --wasm "$T/bridge-canister.wasm" \
  --receipt "$RECOVERY_RECEIPT" >/dev/null 2>&1; then
  echo "post-install recovery published a receipt after evidence-time source drift" >&2
  exit 1
fi
[[ ! -e "$RECOVERY_RECEIPT" \
  && -s "$RECOVERY_RECEIPT.recovery.json" ]]
[[ -f "$RECOVERY_RECEIPT.reservation" \
  && -f "$RECOVERY_RECEIPT.recovery.json.reservation" ]]
TRACE_LINES_BEFORE_EVIDENCE_REPLAY="$(wc -l <"$TRACE" | tr -d ' ')"
if PATH="$T/bin:$PATH" BRIDGE_ICP_IDENTITY=production \
  "$T/source/scripts/production-canister-install.sh" --resume-post-install \
  --plan "$T/installed-plan.json" --wasm "$T/bridge-canister.wasm" \
  --receipt "$RECOVERY_RECEIPT" >/dev/null 2>&1; then
  echo "post-install recovery replayed an evidence-only terminal state" >&2
  exit 1
fi
[[ "$(wc -l <"$TRACE" | tr -d ' ')" == "$TRACE_LINES_BEFORE_EVIDENCE_REPLAY" ]]
git -C "$T/source" checkout -q -- scripts/production-canister-install.sh
rm -f "$MUTATED_MARKER"
clear_failed_recovery "$RECOVERY_RECEIPT"

make_recovery_reservation "$RECOVERY_RECEIPT"
if PATH="$T/bin:$PATH" MUTATE_STAGED_ON_LINK=true \
  PUBLISH_TARGET="$RECOVERY_RECEIPT.recovery.json" \
  BRIDGE_ICP_IDENTITY=production \
  "$T/source/scripts/production-canister-install.sh" --resume-post-install \
  --plan "$T/installed-plan.json" --wasm "$T/bridge-canister.wasm" \
  --receipt "$RECOVERY_RECEIPT" >"$T/evidence-stage-race.log" 2>&1; then
  echo "post-install recovery published a substituted staged evidence inode" >&2
  exit 1
fi
[[ -f "$MUTATED_STAGE_MARKER" ]]
grep -q 'published artifact is not the verified staged inode' \
  "$T/evidence-stage-race.log"
[[ ! -e "$RECOVERY_RECEIPT" && ! -e "$RECOVERY_RECEIPT.recovery.json" ]]
[[ -f "$RECOVERY_RECEIPT.reservation" \
  && -f "$RECOVERY_RECEIPT.recovery.json.reservation" ]]
rm -f "$MUTATED_STAGE_MARKER"
clear_failed_recovery "$RECOVERY_RECEIPT"

make_recovery_reservation "$RECOVERY_RECEIPT"
if PATH="$T/bin:$PATH" MUTATE_STAGED_ON_LINK=true \
  PUBLISH_TARGET="$RECOVERY_RECEIPT" \
  BRIDGE_ICP_IDENTITY=production \
  "$T/source/scripts/production-canister-install.sh" --resume-post-install \
  --plan "$T/installed-plan.json" --wasm "$T/bridge-canister.wasm" \
  --receipt "$RECOVERY_RECEIPT" >"$T/receipt-stage-race.log" 2>&1; then
  echo "post-install recovery published a substituted staged receipt inode" >&2
  exit 1
fi
[[ -f "$MUTATED_STAGE_MARKER" ]]
grep -q 'published artifact is not the verified staged inode' \
  "$T/receipt-stage-race.log"
[[ ! -e "$RECOVERY_RECEIPT" && -s "$RECOVERY_RECEIPT.recovery.json" ]]
[[ -f "$RECOVERY_RECEIPT.reservation" \
  && -f "$RECOVERY_RECEIPT.recovery.json.reservation" ]]
rm -f "$MUTATED_STAGE_MARKER"
clear_failed_recovery "$RECOVERY_RECEIPT"

make_recovery_reservation "$RECOVERY_RECEIPT"
if PATH="$T/bin:$PATH" GIT_EXTERNAL_DIFF=/usr/bin/true \
  GIT_EXTERNAL_DIFF_TRUST_EXIT_CODE=true BRIDGE_ICP_IDENTITY=production \
  "$T/source/scripts/production-canister-install.sh" --resume-post-install \
  --plan "$T/installed-plan.json" --wasm "$T/bridge-canister.wasm" \
  --receipt "$RECOVERY_RECEIPT" >"$T/git-environment-rejected.log" 2>&1; then
  echo "post-install recovery accepted inherited external diff configuration" >&2
  exit 1
fi
grep -q 'rejects inherited GIT_EXTERNAL_DIFF' "$T/git-environment-rejected.log"
[[ -f "$RECOVERY_RECEIPT.reservation" \
  && ! -e "$RECOVERY_RECEIPT.recovery.json.reservation" ]]
clear_failed_recovery "$RECOVERY_RECEIPT"

printf '# unauthorized recovery source change\n' >>"$T/source/Cargo.lock"
git -C "$T/source" add Cargo.lock
git -C "$T/source" commit -qm 'change a payload build input'
CARGO_BUILDS_BEFORE_SOURCE_REJECTION="$(wc -l <"$CARGO_MANIFEST_TRACE" | tr -d ' ')"
make_recovery_reservation "$RECOVERY_RECEIPT"
if (CDPATH='' cd -- "$T/source/scripts" && PATH="$T/bin:$PATH" \
  BRIDGE_ICP_IDENTITY=production ./production-canister-install.sh --resume-post-install \
  --plan "$T/installed-plan.json" --wasm "$T/bridge-canister.wasm" \
  --receipt "$RECOVERY_RECEIPT" >"$T/payload-source-rejected.log" 2>&1); then
  echo "post-install recovery accepted a payload source change" >&2
  exit 1
fi
grep -q 'post-install recovery source changes Canister or release inputs' \
  "$T/payload-source-rejected.log"
[[ -f "$RECOVERY_RECEIPT.reservation" \
  && ! -e "$RECOVERY_RECEIPT.recovery.json.reservation" ]]
[[ "$(wc -l <"$CARGO_MANIFEST_TRACE" | tr -d ' ')" \
  == "$CARGO_BUILDS_BEFORE_SOURCE_REJECTION" ]]
[[ "$(grep -c 'canister install ' "$TRACE")" == "$INSTALL_CALLS_BEFORE_RECOVERY" ]]
