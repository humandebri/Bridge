#!/usr/bin/env bash
# Tool version gate tests: retain command diagnostics and bound Lean retries.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# shellcheck source=check_tool_versions.sh
source "$ROOT/scripts/check_tool_versions.sh"

TEST_TMP_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/bridge-tool-version-gate.XXXXXX")"
trap 'rm -rf "$TEST_TMP_ROOT"' EXIT
export TOOL_VERSION_RETRY_DELAY_SECONDS=0

fake_lean_retry() {
  local attempts
  attempts="$(cat "$TEST_TMP_ROOT/retry-attempts" 2>/dev/null || echo 0)"
  attempts=$((attempts + 1))
  printf '%s\n' "$attempts" >"$TEST_TMP_ROOT/retry-attempts"
  if [[ "$attempts" -eq 1 ]]; then
    echo "temporary Lean download failure" >&2
    return 75
  fi
  echo "Lean (version 4.30.0, test-platform, Release)"
}

retry_output="$(require_output fake_lean_retry "4.30.0" "$LEAN_VERSION_PATTERN" 3 2>&1)"
if [[ "$(cat "$TEST_TMP_ROOT/retry-attempts")" -ne 2 ]] \
  || ! grep -Fq "attempt 1/3, exit 75" <<<"$retry_output" \
  || ! grep -Fq "temporary Lean download failure" <<<"$retry_output"; then
  echo "Lean version retry did not retain the first failure diagnostic" >&2
  exit 1
fi

fake_lean_failure() {
  local attempts
  attempts="$(cat "$TEST_TMP_ROOT/failure-attempts" 2>/dev/null || echo 0)"
  attempts=$((attempts + 1))
  printf '%s\n' "$attempts" >"$TEST_TMP_ROOT/failure-attempts"
  echo "download stdout"
  echo "download stderr" >&2
  return 23
}

set +e
failure_output="$(require_output fake_lean_failure "4.30.0" "$LEAN_VERSION_PATTERN" 3 2>&1)"
failure_status=$?
set -e
if [[ "$failure_status" -eq 0 ]] \
  || [[ "$(cat "$TEST_TMP_ROOT/failure-attempts")" -ne 3 ]] \
  || ! grep -Fq "attempt 3/3, exit 23" <<<"$failure_output" \
  || ! grep -Fq "download stdout" <<<"$failure_output" \
  || ! grep -Fq "download stderr" <<<"$failure_output"; then
  echo "Lean version retry did not fail closed with complete diagnostics" >&2
  exit 1
fi

fake_lean_mismatch() {
  local attempts
  attempts="$(cat "$TEST_TMP_ROOT/mismatch-attempts" 2>/dev/null || echo 0)"
  printf '%s\n' "$((attempts + 1))" >"$TEST_TMP_ROOT/mismatch-attempts"
  echo "Lean (version 4.30.1, test-platform, Release)"
}

set +e
mismatch_output="$(require_output fake_lean_mismatch "4.30.0" "$LEAN_VERSION_PATTERN" 3 2>&1)"
mismatch_status=$?
set -e
if [[ "$mismatch_status" -eq 0 ]] \
  || [[ "$(cat "$TEST_TMP_ROOT/mismatch-attempts")" -ne 1 ]] \
  || ! grep -Fq "version mismatch: expected 4.30.0" <<<"$mismatch_output"; then
  echo "Lean version mismatch was retried or accepted" >&2
  exit 1
fi

echo "tool version gate tests passed"
