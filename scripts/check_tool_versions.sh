#!/usr/bin/env bash
# Repository tool gate: reject drift from the pinned compiler and verifier versions.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# shellcheck source=ci_guards.sh
source "$ROOT/scripts/ci_guards.sh"

require_output() {
  local command_name="$1"
  local expected_version="$2"
  local expected_pattern="$3"
  local actual

  if ! command -v "$command_name" >/dev/null 2>&1; then
    echo "missing required tool: $command_name" >&2
    return 1
  fi

  actual="$($command_name --version 2>&1)"
  if ! output_has_matching_line "$actual" "$expected_pattern"; then
    echo "$command_name version mismatch: expected $expected_version" >&2
    echo "$actual" >&2
    return 1
  fi
}

require_git_revision() {
  local dependency_path="$1"
  local expected_revision="$2"
  local actual_revision

  if [[ ! -e "$dependency_path/.git" ]]; then
    echo "missing required git dependency: $dependency_path" >&2
    return 1
  fi

  actual_revision="$(git -C "$dependency_path" rev-parse HEAD 2>/dev/null)"
  if [[ "$actual_revision" != "$expected_revision" ]]; then
    echo "dependency revision mismatch: $dependency_path" >&2
    echo "expected $expected_revision, found $actual_revision" >&2
    return 1
  fi
  if ! git -C "$dependency_path" diff --quiet || ! git -C "$dependency_path" diff --cached --quiet; then
    echo "dependency has uncommitted changes: $dependency_path" >&2
    return 1
  fi
}

require_output rustc "1.97.0" "$RUST_VERSION_PATTERN"
require_output icp "1.0.2" "$ICP_VERSION_PATTERN"
require_output forge "1.7.1" "$FOUNDRY_VERSION_PATTERN"
require_output anvil "1.7.1" "$ANVIL_VERSION_PATTERN"
require_output z3 "4.16.0" "$Z3_VERSION_PATTERN"
require_output verus "0.2026.07.05.49b8806" "$VERUS_VERSION_PATTERN"
require_output lean "4.30.0" "$LEAN_VERSION_PATTERN"
require_output node "24.14.0" "$NODE_VERSION_PATTERN"
require_output pnpm "11.0.8" "$PNPM_VERSION_PATTERN"
require_git_revision \
  "$ROOT/contracts/lib/openzeppelin-contracts" \
  "5fd1781b1454fd1ef8e722282f86f9293cacf256"
