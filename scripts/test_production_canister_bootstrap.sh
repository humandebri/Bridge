#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
T="$(mktemp -d "${TMPDIR:-/tmp}/bridge-bootstrap-test.XXXXXX")"
trap 'rm -rf "$T"' EXIT
mkdir -p "$T/source/scripts" "$T/source/.icp/data/mappings" "$T/bin"
cp "$ROOT/scripts/production-canister-bootstrap.sh" "$T/source/scripts/"
touch "$T/source/icp.yaml"
chmod +x "$T/source/scripts/production-canister-bootstrap.sh"

export TRACE="$T/trace"
export TEST_ROOT="$T/source"
export TEST_CANISTER_ID="2vxsx-fae"
export EXPECTED_SUBNET="pzp6e-ekpqk-3c5x7-2h6so-njoeq-mt45d-h3h6c-q3mxf-vpeez-fez7a-iae"
cat >"$T/bin/icp" <<'SH'
#!/usr/bin/env bash
printf 'icp %s\n' "$*" >>"$TRACE"
if [[ "$1" == --version ]]; then
  echo "icp ${ICP_TEST_VERSION:-1.0.2}"
elif [[ "$*" == canister\ create\ bridge-canister* ]]; then
  [[ " $* " == *" -e production "* && " $* " == *" --subnet $EXPECTED_SUBNET "* ]]
  mkdir -p "$TEST_ROOT/.icp/data/mappings"
  printf '{"bridge-canister":"%s"}\n' "$TEST_CANISTER_ID" >"$TEST_ROOT/.icp/data/mappings/production.ids.json"
elif [[ "$*" == canister\ status\ bridge-canister* ]]; then
  echo "${STATUS_ID:-$TEST_CANISTER_ID}"
elif [[ "$*" == *"get_subnet_for_canister"* ]]; then
  printf '(opt principal "%s")\n' "${REGISTRY_SUBNET:-$EXPECTED_SUBNET}"
else
  echo "unexpected icp invocation: $*" >&2
  exit 1
fi
SH
chmod +x "$T/bin/icp"
export PATH="$T/bin:$PATH"

run_bootstrap() {
  BRIDGE_ICP_IDENTITY=production "$T/source/scripts/production-canister-bootstrap.sh"
}

printf '{}\n' >"$T/source/.icp/data/mappings/production.ids.json"
: >"$TRACE"
run_bootstrap >/dev/null
rg -q "canister create bridge-canister -e production --subnet $EXPECTED_SUBNET --identity production" "$TRACE"
rg -q 'canister status bridge-canister -e production -i --identity production' "$TRACE"
rg -q "get_subnet_for_canister.*$TEST_CANISTER_ID" "$TRACE"

: >"$TRACE"
run_bootstrap >/dev/null
if rg -q 'canister create' "$TRACE"; then
  echo "bootstrap recreated an existing production Canister" >&2
  exit 1
fi

if REGISTRY_SUBNET=aaaaa-aa run_bootstrap >/dev/null 2>&1; then
  echo "bootstrap accepted a wrong subnet" >&2
  exit 1
fi

if STATUS_ID=aaaaa-aa run_bootstrap >/dev/null 2>&1; then
  echo "bootstrap accepted a mapping/status mismatch" >&2
  exit 1
fi

if ICP_TEST_VERSION=1.0.3 run_bootstrap >/dev/null 2>&1; then
  echo "bootstrap accepted an unreviewed icp-cli version" >&2
  exit 1
fi

echo "production Canister bootstrap tests passed"
