#!/usr/bin/env bash
# Exercise every nested /workspace mount below a read-only candidate checkout.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
IMAGE="${1:-${BRIDGE_TRUSTED_PR_IMAGE:-kinic-bridge-trusted-pr:local}}"
# shellcheck source=/dev/null
source "$ROOT/scripts/trusted-pr-mountpoints.sh"

FIXTURE="$(mktemp -d "${RUNNER_TEMP:-/tmp}/bridge-pr-mount-smoke.XXXXXX")"
cleanup() {
  local exit_status=$?
  bridge_cleanup_mountpoints || [[ "$exit_status" -ne 0 ]] || exit_status=1
  rm -rf "$FIXTURE"
  trap - EXIT
  exit "$exit_status"
}
trap cleanup EXIT

CANDIDATE="$FIXTURE/candidate"
POLICY="$FIXTURE/policy"
SCRATCH="$FIXTURE/scratch"
mkdir -p "$CANDIDATE/ui" "$CANDIDATE/contracts" "$CANDIDATE/verification/lean" \
  "$CANDIDATE/scripts" \
  "$POLICY/scripts" "$POLICY/node_modules" "$POLICY/ui/node_modules" \
  "$POLICY/ui/.e2e-cache" "$SCRATCH"
git -C "$CANDIDATE" init --quiet
printf 'fixture\n' >"$POLICY/scripts/fixture.txt"
printf 'candidate-added\n' >"$CANDIDATE/scripts/candidate-added.sh"
printf 'candidate-modified\n' >"$CANDIDATE/scripts/fixture.txt"
git -C "$CANDIDATE" add scripts/candidate-added.sh scripts/fixture.txt
: >"$POLICY/scripts/candidate-added.sh"
printf 'artifact\n' >"$POLICY/ui/.e2e-cache/ledger.wasm.gz"

bridge_prepare_mountpoint "$CANDIDATE" scripts
for path in node_modules ui/node_modules target contracts/out contracts/cache \
  verification/output verification/lean/.lake .icp/cache ui/dist ui/test-results \
  ui/.e2e-cache ui/.e2e-runtime .tools; do
  bridge_prepare_candidate_mountpoint "$CANDIDATE" "$path"
done
bridge_prepare_mountpoint "$POLICY/ui/node_modules" .tmp
bridge_prepare_mountpoint "$POLICY/ui/node_modules" .vite-temp

for path in target contracts-out contracts-cache proof-output lean-lake icp-cache \
  ui-dist ui-results ui-tsbuildinfo ui-vite-temp e2e-runtime empty-tools; do
  mkdir -p "$SCRATCH/$path"
done
chmod -R 0777 "$SCRATCH"
chmod 0555 "$SCRATCH/empty-tools"

docker run --rm \
  --user "$(id -u):$(id -g)" \
  --read-only \
  --network none \
  --cap-drop ALL \
  --security-opt no-new-privileges \
  --mount "type=bind,src=$CANDIDATE,dst=/workspace,readonly" \
  --mount "type=bind,src=$POLICY/scripts,dst=/workspace/scripts,readonly" \
  --mount "type=bind,src=$CANDIDATE/scripts/candidate-added.sh,dst=/workspace/scripts/candidate-added.sh,readonly" \
  --mount "type=bind,src=$CANDIDATE/scripts,dst=/scratch/candidate-scripts,readonly" \
  --mount "type=bind,src=$POLICY/node_modules,dst=/workspace/node_modules,readonly" \
  --mount "type=bind,src=$POLICY/ui/node_modules,dst=/workspace/ui/node_modules,readonly" \
  --mount "type=bind,src=$SCRATCH/ui-tsbuildinfo,dst=/workspace/ui/node_modules/.tmp" \
  --mount "type=bind,src=$SCRATCH/ui-vite-temp,dst=/workspace/ui/node_modules/.vite-temp" \
  --mount "type=bind,src=$SCRATCH/target,dst=/workspace/target" \
  --mount "type=bind,src=$SCRATCH/contracts-out,dst=/workspace/contracts/out" \
  --mount "type=bind,src=$SCRATCH/contracts-cache,dst=/workspace/contracts/cache" \
  --mount "type=bind,src=$SCRATCH/proof-output,dst=/workspace/verification/output" \
  --mount "type=bind,src=$SCRATCH/lean-lake,dst=/workspace/verification/lean/.lake" \
  --mount "type=bind,src=$SCRATCH/icp-cache,dst=/workspace/.icp/cache" \
  --mount "type=bind,src=$SCRATCH/ui-dist,dst=/workspace/ui/dist" \
  --mount "type=bind,src=$SCRATCH/ui-results,dst=/workspace/ui/test-results" \
  --mount "type=bind,src=$POLICY/ui/.e2e-cache,dst=/workspace/ui/.e2e-cache,readonly" \
  --mount "type=bind,src=$SCRATCH/e2e-runtime,dst=/workspace/ui/.e2e-runtime" \
  --mount "type=bind,src=$SCRATCH/empty-tools,dst=/workspace/.tools,readonly" \
  "$IMAGE" /bin/bash -ceu '
    git -C /workspace status --short >/dev/null
    touch /workspace/target/write
    touch /workspace/contracts/out/write
    touch /workspace/contracts/cache/write
    touch /workspace/verification/output/write
    touch /workspace/verification/lean/.lake/write
    touch /workspace/.icp/cache/write
    touch /workspace/ui/dist/write
    touch /workspace/ui/test-results/write
    touch /workspace/ui/node_modules/.tmp/write
    touch /workspace/ui/node_modules/.vite-temp/write
    touch /workspace/ui/.e2e-runtime/write
    test -f /workspace/ui/.e2e-cache/ledger.wasm.gz
    test -f /workspace/scripts/candidate-added.sh
    grep -qx fixture /workspace/scripts/fixture.txt
    test -f /scratch/candidate-scripts/candidate-added.sh
    grep -qx candidate-modified /scratch/candidate-scripts/fixture.txt
    ! touch /workspace/candidate-write
    ! touch /workspace/node_modules/dependency-write
    ! touch /workspace/.tools/tool-write
  '

bridge_cleanup_mountpoints
test ! -e "$CANDIDATE/target"
test ! -e "$CANDIDATE/node_modules"
test ! -e "$CANDIDATE/ui/node_modules"
test ! -e "$POLICY/ui/node_modules/.tmp"
