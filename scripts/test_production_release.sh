#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
RELEASE="$ROOT/scripts/production-release.sh"

bash -n "$RELEASE"
! grep -q 'production-canister-upgrade-receipt' "$RELEASE"
! grep -q 'post-gate-a-policy-transition' "$RELEASE"
! grep -q 'production_validate_gate_b_source_chain' "$RELEASE"
grep -q 'gate-a-receipt.json' "$RELEASE"
grep -q 'gate-a-profile.json' "$RELEASE"
grep -q 'validate-bundle --offline --gate-b' "$RELEASE"

echo 'production release current-state policy contract: pass'
