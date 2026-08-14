#!/usr/bin/env bash
# Execute one untrusted PR check in a fresh process namespace with immutable inputs.
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
[[ -d "$POLICY_ROOT/ui/node_modules" && ! -L "$POLICY_ROOT/ui/node_modules" ]] \
  || { echo "trusted UI dependencies are missing" >&2; exit 1; }
[[ "$(git -C "$SOURCE_ROOT" rev-parse HEAD)" == "${BRIDGE_EXPECTED_HEAD_SHA:?missing expected head SHA}" ]] \
  || { echo "candidate checkout SHA mismatch" >&2; exit 1; }

SCRATCH="$(mktemp -d "${RUNNER_TEMP:-/tmp}/bridge-pr-${MODE}.XXXXXX")"
trap 'rm -rf "$SCRATCH"' EXIT INT TERM
mkdir -p "$SCRATCH/home" "$SCRATCH/tmp" "$SCRATCH/target" "$SCRATCH/contracts-out" \
  "$SCRATCH/contracts-cache" "$SCRATCH/ui-dist" "$SCRATCH/ui-results" "$SCRATCH/local" \
  "$SCRATCH/empty-tools"
chmod -R 0777 "$SCRATCH"
chmod 0555 "$SCRATCH/empty-tools"

TOOL_MOUNTS=()
for tool_path in .cargo .rustup .local .elan .foundry setup-pnpm; do
  [[ -d "/home/runner/$tool_path" ]] || { echo "trusted tool path is missing: $tool_path" >&2; exit 1; }
  TOOL_MOUNTS+=(--mount "type=bind,src=/home/runner/$tool_path,dst=/home/runner/$tool_path,readonly")
done
if [[ -d /home/runner/.cache/ms-playwright ]]; then
  TOOL_MOUNTS+=(--mount "type=bind,src=/home/runner/.cache/ms-playwright,dst=/home/runner/.cache/ms-playwright,readonly")
fi

docker run --rm \
  --read-only \
  --network none \
  --pids-limit 2048 \
  --cap-drop ALL \
  --security-opt no-new-privileges \
  --mount "type=bind,src=$SOURCE_ROOT,dst=/workspace,readonly" \
  --mount "type=bind,src=$POLICY_ROOT/scripts,dst=/workspace/scripts,readonly" \
  --mount "type=bind,src=$POLICY_ROOT/ui/node_modules,dst=/workspace/ui/node_modules,readonly" \
  --mount "type=bind,src=$SCRATCH/target,dst=/workspace/target" \
  --mount "type=bind,src=$SCRATCH/contracts-out,dst=/workspace/contracts/out" \
  --mount "type=bind,src=$SCRATCH/contracts-cache,dst=/workspace/contracts/cache" \
  --mount "type=bind,src=$SCRATCH/ui-dist,dst=/workspace/ui/dist" \
  --mount "type=bind,src=$SCRATCH/ui-results,dst=/workspace/ui/test-results" \
  --mount "type=bind,src=$SCRATCH/local,dst=/workspace/.local" \
  --mount "type=bind,src=$SCRATCH/empty-tools,dst=/workspace/.tools,readonly" \
  --mount "type=bind,src=$SCRATCH/home,dst=/scratch/home" \
  --mount "type=bind,src=$SCRATCH/tmp,dst=/scratch/tmp" \
  "${TOOL_MOUNTS[@]}" \
  --mount type=bind,src=/opt/hostedtoolcache,dst=/opt/hostedtoolcache,readonly \
  --env CI=true \
  --env HOME=/scratch/home \
  --env TMPDIR=/scratch/tmp \
  --env CARGO_HOME=/home/runner/.cargo \
  --env RUSTUP_HOME=/home/runner/.rustup \
  --env PATH="${PATH:?}" \
  --workdir /workspace \
  "$IMAGE" \
  /workspace/scripts/ci-local.sh "$MODE"
