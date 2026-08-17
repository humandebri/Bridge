#!/usr/bin/env bash
# Execute one untrusted PR check in a fresh process namespace with immutable inputs.
set -euo pipefail

SOURCE_ROOT="$(cd "${1:?missing candidate source}" && pwd)"
POLICY_ROOT="$(cd "${2:?missing trusted policy}" && pwd)"
MODE="${3:?missing CI mode}"
IMAGE="${BRIDGE_TRUSTED_PR_IMAGE:-kinic-bridge-trusted-pr:local}"

case "$MODE" in
  rust|contracts|proofs|ui|real|icp) ;;
  *) echo "unapproved trusted PR mode: $MODE" >&2; exit 2 ;;
esac

NEEDS_WORKSPACE_DEPS=0
NEEDS_UI_DEPS=0
case "$MODE" in
  rust|proofs) NEEDS_WORKSPACE_DEPS=1 ;;
esac
case "$MODE" in
  proofs|ui|real) NEEDS_UI_DEPS=1 ;;
esac

[[ -d "$SOURCE_ROOT/.git" && ! -L "$SOURCE_ROOT" ]] || { echo "candidate source must be a checkout" >&2; exit 1; }
[[ -d "$POLICY_ROOT/scripts" && ! -L "$POLICY_ROOT/scripts" ]] || { echo "trusted policy is invalid" >&2; exit 1; }
if [[ "$NEEDS_WORKSPACE_DEPS" -eq 1 ]]; then
  [[ -d "$POLICY_ROOT/node_modules" && ! -L "$POLICY_ROOT/node_modules" ]] \
    || { echo "trusted workspace dependencies are missing" >&2; exit 1; }
fi
if [[ "$NEEDS_UI_DEPS" -eq 1 ]]; then
  [[ -d "$POLICY_ROOT/ui/node_modules" && ! -L "$POLICY_ROOT/ui/node_modules" ]] \
    || { echo "trusted UI dependencies are missing" >&2; exit 1; }
fi
[[ "$(git -C "$SOURCE_ROOT" rev-parse HEAD)" == "${BRIDGE_EXPECTED_HEAD_SHA:?missing expected head SHA}" ]] \
  || { echo "candidate checkout SHA mismatch" >&2; exit 1; }

SCRATCH="$(mktemp -d "${RUNNER_TEMP:-/tmp}/bridge-pr-${MODE}.XXXXXX")"
trap 'rm -rf "$SCRATCH"' EXIT INT TERM
mkdir -p "$SCRATCH/home" "$SCRATCH/tmp" "$SCRATCH/target" "$SCRATCH/contracts-out" \
  "$SCRATCH/contracts-cache" "$SCRATCH/ui-dist" "$SCRATCH/ui-results" \
  "$SCRATCH/ui-tsbuildinfo" "$SCRATCH/e2e-runtime" "$SCRATCH/proof" "$SCRATCH/empty-tools"
chmod -R 0777 "$SCRATCH"
chmod 0555 "$SCRATCH/empty-tools"

CACHE_MOUNTS=()
if [[ "$MODE" == "real" ]]; then
  [[ -d "$POLICY_ROOT/ui/.e2e-cache" && ! -L "$POLICY_ROOT/ui/.e2e-cache" ]] \
    || { echo "trusted real-E2E artifact cache is missing" >&2; exit 1; }
  CACHE_MOUNTS+=(--mount "type=bind,src=$POLICY_ROOT/ui/.e2e-cache,dst=/workspace/ui/.e2e-cache,readonly")
fi

TOOL_MOUNTS=()
TOOL_PATHS=()
case "$MODE" in
  rust) TOOL_PATHS=(.cargo .rustup .local setup-pnpm) ;;
  contracts) TOOL_PATHS=(.foundry) ;;
  proofs) TOOL_PATHS=(.cargo .rustup .local .elan .foundry setup-pnpm) ;;
  ui) TOOL_PATHS=(setup-pnpm) ;;
  real) TOOL_PATHS=(.cargo .rustup .foundry setup-pnpm) ;;
  icp) TOOL_PATHS=(.cargo .rustup .local) ;;
esac
for tool_path in "${TOOL_PATHS[@]}"; do
  [[ -d "/home/runner/$tool_path" ]] || { echo "trusted tool path is missing: $tool_path" >&2; exit 1; }
  TOOL_MOUNTS+=(--mount "type=bind,src=/home/runner/$tool_path,dst=/home/runner/$tool_path,readonly")
done
if [[ -d /home/runner/.cache/ms-playwright ]]; then
  TOOL_MOUNTS+=(--mount "type=bind,src=/home/runner/.cache/ms-playwright,dst=/home/runner/.cache/ms-playwright,readonly")
fi

DEPENDENCY_MOUNTS=()
if [[ "$NEEDS_WORKSPACE_DEPS" -eq 1 ]]; then
  DEPENDENCY_MOUNTS+=(--mount "type=bind,src=$POLICY_ROOT/node_modules,dst=/workspace/node_modules,readonly")
fi
if [[ "$NEEDS_UI_DEPS" -eq 1 ]]; then
  DEPENDENCY_MOUNTS+=(--mount "type=bind,src=$POLICY_ROOT/ui/node_modules,dst=/workspace/ui/node_modules,readonly")
fi

WRITABLE_UI_MOUNTS=()
if [[ "$NEEDS_UI_DEPS" -eq 1 ]]; then
  WRITABLE_UI_MOUNTS+=(--mount "type=bind,src=$SCRATCH/ui-tsbuildinfo,dst=/workspace/ui/node_modules/.tmp")
fi
if [[ "$MODE" == "real" ]]; then
  WRITABLE_UI_MOUNTS+=(--mount "type=bind,src=$SCRATCH/e2e-runtime,dst=/workspace/ui/.e2e-runtime")
fi

docker run --rm \
  --read-only \
  --network none \
  --pids-limit 2048 \
  --cap-drop ALL \
  --security-opt no-new-privileges \
  --mount "type=bind,src=$SOURCE_ROOT,dst=/workspace,readonly" \
  --mount "type=bind,src=$POLICY_ROOT/scripts,dst=/workspace/scripts,readonly" \
  "${DEPENDENCY_MOUNTS[@]}" \
  --mount "type=bind,src=$SCRATCH/target,dst=/workspace/target" \
  --mount "type=bind,src=$SCRATCH/contracts-out,dst=/workspace/contracts/out" \
  --mount "type=bind,src=$SCRATCH/contracts-cache,dst=/workspace/contracts/cache" \
  "${WRITABLE_UI_MOUNTS[@]}" \
  --mount "type=bind,src=$SCRATCH/ui-dist,dst=/workspace/ui/dist" \
  --mount "type=bind,src=$SCRATCH/ui-results,dst=/workspace/ui/test-results" \
  --mount "type=bind,src=$SCRATCH/empty-tools,dst=/workspace/.tools,readonly" \
  --mount "type=bind,src=$SCRATCH/home,dst=/scratch/home" \
  --mount "type=bind,src=$SCRATCH/tmp,dst=/scratch/tmp" \
  --mount "type=bind,src=$SCRATCH/proof,dst=/scratch/proof" \
  "${TOOL_MOUNTS[@]}" \
  "${CACHE_MOUNTS[@]}" \
  --mount type=bind,src=/opt/hostedtoolcache,dst=/opt/hostedtoolcache,readonly \
  --env CI=true \
  --env BRIDGE_TRUSTED_DEPS_READY=1 \
  --env BRIDGE_CLAIM_REPORT=/scratch/proof/claim-report.json \
  --env PROOF_RECEIPT=/scratch/proof/proof-receipt.json \
  --env HOME=/scratch/home \
  --env TMPDIR=/scratch/tmp \
  --env CARGO_HOME=/home/runner/.cargo \
  --env RUSTUP_HOME=/home/runner/.rustup \
  --env PATH="${PATH:?}" \
  --workdir /workspace \
  "$IMAGE" \
  /workspace/scripts/ci-local.sh "$MODE"
