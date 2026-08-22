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
  "$SCRATCH/contracts-cache" "$SCRATCH/contracts-staging-out" "$SCRATCH/contracts-staging-cache" "$SCRATCH/ui-dist" \
  "$SCRATCH/ui-results" "$SCRATCH/ui-tsbuildinfo" "$SCRATCH/ui-vite-temp" "$SCRATCH/ui-vite" \
  "$SCRATCH/e2e-runtime" "$SCRATCH/proof-output" "$SCRATCH/lean-lake" "$SCRATCH/smt-out" "$SCRATCH/smt-cache" \
  "$SCRATCH/icp-cache" "$SCRATCH/empty-tools" \
  "$SCRATCH/home/.svm" "$SCRATCH/home/.elan/toolchains" \
  "$SCRATCH/home/.local/share/icp-cli/pkg" "$SCRATCH/home/.config"
chmod -R 0777 "$SCRATCH"
chmod 0555 "$SCRATCH/empty-tools"

bridge_prepare_mountpoint "$SOURCE_ROOT" scripts
while IFS= read -r candidate_script; do
  [[ -f "$SOURCE_ROOT/$candidate_script" ]] || continue
  [[ -e "$POLICY_ROOT/$candidate_script" ]] && continue
  mkdir -p "$(dirname "$POLICY_ROOT/$candidate_script")"
  cp -p "$SOURCE_ROOT/$candidate_script" "$POLICY_ROOT/$candidate_script"
  chmod +x "$POLICY_ROOT/$candidate_script"
done < <(git -C "$SOURCE_ROOT" ls-files scripts/)
for path in node_modules ui/node_modules target contracts/out contracts/cache \
  contracts/out-staging contracts/cache-staging \
  ui/dist ui/test-results .tools; do
  bridge_prepare_candidate_mountpoint "$SOURCE_ROOT" "$path"
done

WRITABLE_UI_MOUNTS=()
case "$MODE" in
  proofs|ui-fast|ui-e2e|real)
    bridge_prepare_mountpoint "$POLICY_ROOT/ui/node_modules" .tmp
    bridge_prepare_mountpoint "$POLICY_ROOT/ui/node_modules" .vite-temp
    bridge_prepare_mountpoint "$POLICY_ROOT/ui/node_modules" .vite
    WRITABLE_UI_MOUNTS+=(--mount "type=bind,src=$SCRATCH/ui-tsbuildinfo,dst=/workspace/ui/node_modules/.tmp")
    WRITABLE_UI_MOUNTS+=(--mount "type=bind,src=$SCRATCH/ui-vite-temp,dst=/workspace/ui/node_modules/.vite-temp")
    WRITABLE_UI_MOUNTS+=(--mount "type=bind,src=$SCRATCH/ui-vite,dst=/workspace/ui/node_modules/.vite")
    ;;
esac
if [[ "$MODE" == "real" ]]; then
  bridge_prepare_candidate_mountpoint "$SOURCE_ROOT" ui/.e2e-runtime
  WRITABLE_UI_MOUNTS+=(--mount "type=bind,src=$SCRATCH/e2e-runtime,dst=/workspace/ui/.e2e-runtime")
  pic_pkg="$(find "$POLICY_ROOT/ui/node_modules/.pnpm" -maxdepth 4 -type d \
    -path '*/@dfinity+pic@*/node_modules/@dfinity/pic' 2>/dev/null | head -n 1)"
  if [[ -n "$pic_pkg" ]]; then
    pic_scratch="$SCRATCH/pic-package"
    mkdir -p "$pic_scratch"
    cp -a "$pic_pkg/." "$pic_scratch/"
    chmod 0755 "$pic_scratch/pocket-ic" 2>/dev/null || true
    pic_rel="${pic_pkg#"$POLICY_ROOT/"}"
    WRITABLE_UI_MOUNTS+=(--mount "type=bind,src=$pic_scratch,dst=/workspace/$pic_rel")
  else
    echo "trusted real-E2E @dfinity/pic package is missing" >&2
    exit 1
  fi
fi

WRITABLE_BUILD_MOUNTS=()
if [[ "$MODE" == "proofs" ]]; then
  bridge_prepare_candidate_mountpoint "$SOURCE_ROOT" verification/output
  bridge_prepare_candidate_mountpoint "$SOURCE_ROOT" verification/lean/.lake
  bridge_prepare_candidate_mountpoint "$SOURCE_ROOT" verification/smt/out
  bridge_prepare_candidate_mountpoint "$SOURCE_ROOT" verification/smt/cache
  WRITABLE_BUILD_MOUNTS+=(
    --mount "type=bind,src=$SCRATCH/proof-output,dst=/workspace/verification/output"
    --mount "type=bind,src=$SCRATCH/lean-lake,dst=/workspace/verification/lean/.lake"
    --mount "type=bind,src=$SCRATCH/smt-out,dst=/workspace/verification/smt/out"
    --mount "type=bind,src=$SCRATCH/smt-cache,dst=/workspace/verification/smt/cache"
  )
fi
if [[ "$MODE" == "icp" ]]; then
  bridge_prepare_candidate_mountpoint "$SOURCE_ROOT" .icp/cache
  WRITABLE_BUILD_MOUNTS+=(--mount "type=bind,src=$SCRATCH/icp-cache,dst=/workspace/.icp/cache")
fi
if [[ "$MODE" == "real" ]]; then
  bridge_prepare_candidate_mountpoint "$SOURCE_ROOT" contracts/out-staging
  bridge_prepare_candidate_mountpoint "$SOURCE_ROOT" contracts/cache-staging
  WRITABLE_BUILD_MOUNTS+=(
    --mount "type=bind,src=$SCRATCH/contracts-staging-out,dst=/workspace/contracts/out-staging"
    --mount "type=bind,src=$SCRATCH/contracts-staging-cache,dst=/workspace/contracts/cache-staging"
  )
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
if [[ "$MODE" == "proofs" ]]; then
  [[ -d /home/runner/.elan/toolchains && ! -L /home/runner/.elan/toolchains ]] \
    || { echo "trusted Lean toolchains are missing" >&2; exit 1; }
  TOOL_MOUNTS+=(--mount "type=bind,src=/home/runner/.elan/toolchains,dst=/scratch/home/.elan/toolchains,readonly")
fi
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
  --mount "type=bind,src=$SOURCE_ROOT/scripts/plan007/generate-local-e2e.mjs,dst=/workspace/scripts/plan007/generate-local-e2e.mjs,readonly" \
  --mount "type=bind,src=$SOURCE_ROOT/scripts/plan007/test-generate-local-e2e.mjs,dst=/workspace/scripts/plan007/test-generate-local-e2e.mjs,readonly" \
  --mount "type=bind,src=$SOURCE_ROOT/scripts,dst=/scratch/candidate-scripts,readonly" \
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
  --env BRIDGE_CANDIDATE_SCRIPTS=/scratch/candidate-scripts \
  --env PLAYWRIGHT_BROWSERS_PATH=/home/runner/.cache/ms-playwright \
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
