#!/usr/bin/env bash
# Execute one untrusted PR check in a fresh process namespace with read-only inputs.
set -euo pipefail

SOURCE_ROOT="$(cd "${1:?missing candidate source}" && pwd)"
POLICY_ROOT="$(cd "${2:?missing trusted policy}" && pwd)"
MODE="${3:?missing CI mode}"
IMAGE="${BRIDGE_TRUSTED_PR_IMAGE:-kinic-bridge-trusted-pr:local}"

case "$MODE" in
  rust-fast|rust-integration|contracts-fast|contracts-coverage|proofs|ui-fast|ui-e2e|real|icp) ;;
  *) echo "unapproved trusted PR mode: $MODE" >&2; exit 2 ;;
esac

[[ -d "$SOURCE_ROOT/.git" && ! -L "$SOURCE_ROOT" ]] || { echo "candidate source must be a checkout" >&2; exit 1; }
[[ -d "$POLICY_ROOT/scripts" && ! -L "$POLICY_ROOT/scripts" ]] || { echo "trusted policy is invalid" >&2; exit 1; }
[[ -d "$POLICY_ROOT/node_modules" && ! -L "$POLICY_ROOT/node_modules" ]] \
  || { echo "trusted workspace dependencies are missing" >&2; exit 1; }
[[ -d "$POLICY_ROOT/ui/node_modules" && ! -L "$POLICY_ROOT/ui/node_modules" ]] \
  || { echo "trusted UI dependencies are missing" >&2; exit 1; }
[[ "$(git -C "$SOURCE_ROOT" rev-parse HEAD)" == "${BRIDGE_EXPECTED_HEAD_SHA:?missing expected head SHA}" ]] \
  || { echo "candidate checkout SHA mismatch" >&2; exit 1; }
# shellcheck source=/dev/null
source "$POLICY_ROOT/scripts/trusted-pr-mountpoints.sh"

SCRATCH="$(mktemp -d "${RUNNER_TEMP:-/tmp}/bridge-pr-${MODE}.XXXXXX")"
cleanup() {
  local exit_status=$?
  bridge_cleanup_mountpoints || [[ "$exit_status" -ne 0 ]] || exit_status=1
  rm -rf "$SCRATCH"
  trap - EXIT
  exit "$exit_status"
}
trap cleanup EXIT
mkdir -p "$SCRATCH/home" "$SCRATCH/tmp" "$SCRATCH/target" "$SCRATCH/contracts-out" \
  "$SCRATCH/contracts-cache" "$SCRATCH/ui-dist" "$SCRATCH/ui-results" \
  "$SCRATCH/ui-tsbuildinfo" "$SCRATCH/e2e-runtime" "$SCRATCH/proof-output" \
  "$SCRATCH/lean-lake" "$SCRATCH/icp-cache" "$SCRATCH/empty-tools" \
  "$SCRATCH/home/.svm" "$SCRATCH/home/.elan/toolchains" \
  "$SCRATCH/home/.local/share/icp-cli/pkg" "$SCRATCH/home/.config"
chmod -R 0777 "$SCRATCH"
chmod 0555 "$SCRATCH/empty-tools"

bridge_prepare_mountpoint "$SOURCE_ROOT" scripts
for path in node_modules ui/node_modules target contracts/out contracts/cache \
  ui/dist ui/test-results .tools; do
  bridge_prepare_candidate_mountpoint "$SOURCE_ROOT" "$path"
done

WRITABLE_UI_MOUNTS=()
case "$MODE" in
  ui-fast|ui-e2e|real)
    bridge_prepare_mountpoint "$POLICY_ROOT/ui/node_modules" .tmp
    WRITABLE_UI_MOUNTS+=(--mount "type=bind,src=$SCRATCH/ui-tsbuildinfo,dst=/workspace/ui/node_modules/.tmp")
    ;;
esac
if [[ "$MODE" == "real" ]]; then
  bridge_prepare_candidate_mountpoint "$SOURCE_ROOT" ui/.e2e-runtime
  WRITABLE_UI_MOUNTS+=(--mount "type=bind,src=$SCRATCH/e2e-runtime,dst=/workspace/ui/.e2e-runtime")
fi

WRITABLE_BUILD_MOUNTS=()
if [[ "$MODE" == "proofs" ]]; then
  bridge_prepare_candidate_mountpoint "$SOURCE_ROOT" verification/output
  bridge_prepare_candidate_mountpoint "$SOURCE_ROOT" verification/lean/.lake
  WRITABLE_BUILD_MOUNTS+=(
    --mount "type=bind,src=$SCRATCH/proof-output,dst=/workspace/verification/output"
    --mount "type=bind,src=$SCRATCH/lean-lake,dst=/workspace/verification/lean/.lake"
  )
fi
if [[ "$MODE" == "icp" ]]; then
  bridge_prepare_candidate_mountpoint "$SOURCE_ROOT" .icp/cache
  WRITABLE_BUILD_MOUNTS+=(--mount "type=bind,src=$SCRATCH/icp-cache,dst=/workspace/.icp/cache")
fi

CACHE_MOUNTS=()
if [[ "$MODE" == "real" ]]; then
  [[ -d "$POLICY_ROOT/ui/.e2e-cache" && ! -L "$POLICY_ROOT/ui/.e2e-cache" ]] \
    || { echo "trusted real-E2E artifact cache is missing" >&2; exit 1; }
  bridge_prepare_candidate_mountpoint "$SOURCE_ROOT" ui/.e2e-cache
  CACHE_MOUNTS+=(--mount "type=bind,src=$POLICY_ROOT/ui/.e2e-cache,dst=/workspace/ui/.e2e-cache,readonly")
fi

TOOL_MOUNTS=()
for tool_path in .cargo .rustup .local .elan .foundry setup-pnpm; do
  [[ -d "/home/runner/$tool_path" ]] || { echo "trusted tool path is missing: $tool_path" >&2; exit 1; }
  TOOL_MOUNTS+=(--mount "type=bind,src=/home/runner/$tool_path,dst=/home/runner/$tool_path,readonly")
done
[[ -x /home/runner/.svm/0.8.36/solc-0.8.36 ]] \
  || { echo "trusted Solidity compiler is missing" >&2; exit 1; }
TOOL_MOUNTS+=(--mount "type=bind,src=/home/runner/.svm,dst=/scratch/home/.svm,readonly")
[[ -d /home/runner/.elan/toolchains && ! -L /home/runner/.elan/toolchains ]] \
  || { echo "trusted Lean toolchains are missing" >&2; exit 1; }
TOOL_MOUNTS+=(--mount "type=bind,src=/home/runner/.elan/toolchains,dst=/scratch/home/.elan/toolchains,readonly")
[[ -d /home/runner/.local/share/icp-cli/pkg && ! -L /home/runner/.local/share/icp-cli/pkg ]] \
  || { echo "trusted ICP package cache is missing" >&2; exit 1; }
cp -R /home/runner/.local/share/icp-cli/pkg/. "$SCRATCH/home/.local/share/icp-cli/pkg/"
if [[ -d /home/runner/.cache/ms-playwright ]]; then
  TOOL_MOUNTS+=(--mount "type=bind,src=/home/runner/.cache/ms-playwright,dst=/home/runner/.cache/ms-playwright,readonly")
fi

docker run --rm \
  --user "$(id -u):$(id -g)" \
  --read-only \
  --network none \
  --pids-limit 2048 \
  --cap-drop ALL \
  --security-opt no-new-privileges \
  --mount "type=bind,src=$SOURCE_ROOT,dst=/workspace,readonly" \
  --mount "type=bind,src=$POLICY_ROOT/scripts,dst=/workspace/scripts,readonly" \
  --mount "type=bind,src=$POLICY_ROOT/node_modules,dst=/workspace/node_modules,readonly" \
  --mount "type=bind,src=$POLICY_ROOT/ui/node_modules,dst=/workspace/ui/node_modules,readonly" \
  "${WRITABLE_UI_MOUNTS[@]}" \
  --mount "type=bind,src=$SCRATCH/target,dst=/workspace/target" \
  --mount "type=bind,src=$SCRATCH/contracts-out,dst=/workspace/contracts/out" \
  --mount "type=bind,src=$SCRATCH/contracts-cache,dst=/workspace/contracts/cache" \
  "${WRITABLE_BUILD_MOUNTS[@]}" \
  --mount "type=bind,src=$SCRATCH/ui-dist,dst=/workspace/ui/dist" \
  --mount "type=bind,src=$SCRATCH/ui-results,dst=/workspace/ui/test-results" \
  --mount "type=bind,src=$SCRATCH/empty-tools,dst=/workspace/.tools,readonly" \
  --mount "type=bind,src=$SCRATCH/home,dst=/scratch/home" \
  --mount "type=bind,src=$SCRATCH/tmp,dst=/scratch/tmp" \
  "${TOOL_MOUNTS[@]}" \
  --mount type=bind,src=/opt/hostedtoolcache,dst=/opt/hostedtoolcache,readonly \
  "${CACHE_MOUNTS[@]}" \
  --env CI=true \
  --env BRIDGE_TRUSTED_DEPS_READY=1 \
  --env PNPM_CONFIG_VERIFY_DEPS_BEFORE_RUN=false \
  --env HOME=/scratch/home \
  --env TMPDIR=/scratch/tmp \
  --env CARGO_HOME=/home/runner/.cargo \
  --env CARGO_NET_OFFLINE=true \
  --env RUSTUP_HOME=/home/runner/.rustup \
  --env FOUNDRY_OFFLINE=true \
  --env ELAN_HOME=/scratch/home/.elan \
  --env XDG_DATA_HOME=/scratch/home/.local/share \
  --env XDG_CONFIG_HOME=/scratch/home/.config \
  --env ICP_CLI_DISABLE_UPDATE=1 \
  --env ICP_TELEMETRY_DISABLED=1 \
  --env PATH="${PATH:?}" \
  --workdir /workspace \
  "$IMAGE" \
  /workspace/scripts/ci-local.sh "$MODE"
