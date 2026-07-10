#!/usr/bin/env bash
# Repository tool gate: reject drift from the Phase 0 compiler and verifier versions.
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

require_output rustc "1.93.0" "$RUST_VERSION_PATTERN"
require_output icp "0.2.7" "$ICP_VERSION_PATTERN"
require_output forge "1.7.1" "$FOUNDRY_VERSION_PATTERN"
require_output anvil "1.7.1" "$ANVIL_VERSION_PATTERN"
require_output z3 "4.15.4" "$Z3_VERSION_PATTERN"
require_output verus "0.2026.05.05.d03e906" "$VERUS_VERSION_PATTERN"
