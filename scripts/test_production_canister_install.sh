#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
T="$(mktemp -d "${TMPDIR:-/tmp}/bridge-production-install-test.XXXXXX")"
trap 'rm -rf "$T"' EXIT
mkdir -p "$T/source/scripts" "$T/source/.icp/data/mappings" "$T/source/canister/bridge-canister" "$T/bin" "$T/out"
cp "$ROOT/scripts/production-canister-install.sh" "$ROOT/scripts/production-validation.sh" "$T/source/scripts/"
chmod +x "$T/source/scripts/production-canister-install.sh"
printf 'service : () -> {}\n' >"$T/source/canister/bridge-canister/bridge.did"
printf '{"bridge-canister":"aaaaa-aa"}\n' >"$T/source/.icp/data/mappings/production.ids.json"
printf '[workspace]\nmembers=[]\n' >"$T/source/Cargo.toml"
printf '# fixture\n' >"$T/source/Cargo.lock"
git -C "$T/source" init -q
git -C "$T/source" config user.email bridge-test@example.invalid
git -C "$T/source" config user.name bridge-test
git -C "$T/source" add .
git -C "$T/source" commit -qm 'production install fixture'
REVISION="$(git -C "$T/source" rev-parse HEAD)"
TREE="$(git -C "$T/source" archive HEAD | shasum -a 256 | awk '{print $1}')"
printf wasm >"$T/bridge-canister.wasm"
WASM_SHA256="$(shasum -a 256 "$T/bridge-canister.wasm" | awk '{print $1}')"
printf '{"schema_version":2,"environment":"production","source_revision":"%s","source_tree_sha256":"%s","bridge_canister_id":"aaaaa-aa","bridge_canister_wasm_sha256":"%s","init":{}}\n' \
  "$REVISION" "$TREE" "$WASM_SHA256" >"$T/plan.json"

cat >"$T/bin/cargo" <<'SH'
#!/usr/bin/env bash
mkdir -p "$CARGO_TARGET_DIR/release"
cp "$PROFILE_FIXTURE" "$CARGO_TARGET_DIR/release/bridge-profile"
chmod +x "$CARGO_TARGET_DIR/release/bridge-profile"
SH
cat >"$T/profile-fixture" <<'SH'
#!/usr/bin/env bash
case "$1" in
  validate-production-canister-plan) exit 0 ;;
  render-production-canister-inputs)
    mkdir -p "$3"; printf bin >"$3/canister-init.bin"; printf '{}\n' >"$3/production-canister-install-inputs.json" ;;
  storage-validation-complete|storage-checksum-complete) printf 'true\n' ;;
  write-production-canister-receipt)
    printf '{"schema_version":3,"runtime_binding":{"expected_bridge_signer":"0x1111111111111111111111111111111111111111"},"governance_operator":"0x2222222222222222222222222222222222222222","runtime_administrator":"0x3333333333333333333333333333333333333333","independent_canceller":"0x4444444444444444444444444444444444444444"}\n' >"${14}" ;;
  *) echo "unexpected bridge-profile command: $*" >&2; exit 90 ;;
esac
SH
chmod +x "$T/bin/cargo" "$T/profile-fixture"

cat >"$T/bin/icp" <<'SH'
#!/usr/bin/env bash
printf 'icp %s\n' "$*" >>"$TRACE"
if [[ "$1" == --version ]]; then echo 'icp 1.0.2'; exit 0; fi
if [[ "$1 $2" == 'identity principal' ]]; then echo aaaaa-aa; exit 0; fi
if [[ "$1 $2" == 'canister status' ]]; then
  if [[ -e "$INSTALLED_MARKER" ]]; then
    printf '{"controllers":["aaaaa-aa"],"module_hash":"%s"}\n' "$WASM_SHA256"
  else
    printf '{"controllers":["aaaaa-aa"],"module_hash":null}\n'
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
if [[ "$1 $2" == 'canister call' ]]; then printf '4449444c0000\n'; exit 0; fi
echo "unexpected icp command: $*" >&2
exit 91
SH
chmod +x "$T/bin/icp"

export TRACE="$T/trace" PROFILE_FIXTURE="$T/profile-fixture" INSTALLED_MARKER="$T/installed" WASM_SHA256
if PATH="$T/bin:$PATH" INSTALL_FAIL=true BRIDGE_ICP_IDENTITY=production \
  "$T/source/scripts/production-canister-install.sh" --plan "$T/plan.json" \
  --wasm "$T/bridge-canister.wasm" --receipt "$T/out/failed.json" >/dev/null 2>&1; then
  echo "production Canister installer accepted an ambiguous install failure" >&2
  exit 1
fi
[[ -f "$T/out/failed.json.reservation" && ! -e "$T/out/failed.json" ]]
rm "$INSTALLED_MARKER"
PATH="$T/bin:$PATH" BRIDGE_ICP_IDENTITY=production \
  "$T/source/scripts/production-canister-install.sh" --plan "$T/plan.json" \
  --wasm "$T/bridge-canister.wasm" --receipt "$T/out/receipt.json" >/dev/null
[[ -s "$T/out/receipt.json" ]]
python3 - "$T/out/receipt.json" <<'PY'
import json, sys
value = json.load(open(sys.argv[1], encoding="utf-8"))
assert value["schema_version"] == 3
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
! grep -Eq 'reinstall|--mode auto' "$TRACE"

rm "$T/out/receipt.json"
if PATH="$T/bin:$PATH" BRIDGE_ICP_IDENTITY=production \
  "$T/source/scripts/production-canister-install.sh" --plan "$T/plan.json" \
  --wasm "$T/bridge-canister.wasm" --receipt "$T/out/receipt.json" >/dev/null 2>&1; then
  echo "production Canister installer accepted an already-installed module" >&2
  exit 1
fi
[[ "$(grep -c 'canister install ' "$TRACE")" -eq 2 ]]
