#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
DEPLOYER_SERVICE="${BASE_SEPOLIA_DEPLOYER_KEYCHAIN_SERVICE:-kinic-base-sepolia-experiment-keystore}"
SIGNER_SERVICE="${BASE_SEPOLIA_SIGNER_KEYCHAIN_SERVICE:-kinic-base-sepolia-bridge-signer-keystore}"
CANCELLER_SERVICE="${BASE_SEPOLIA_CANCELLER_KEYCHAIN_SERVICE:-kinic-base-sepolia-canceller-keystore}"
ACCOUNT="${BASE_SEPOLIA_KEYCHAIN_ACCOUNT:-$USER}"

umask 077
temp_dir="$(mktemp -d "${TMPDIR:-/tmp}/kinic-base-sepolia.XXXXXX")"
trap 'rm -rf "$temp_dir"' EXIT INT TERM

security find-generic-password -a "$ACCOUNT" -s "$DEPLOYER_SERVICE" -w >"$temp_dir/deployer-password"
security find-generic-password -a "$ACCOUNT" -s "$SIGNER_SERVICE" -w >"$temp_dir/signer-password"
security find-generic-password -a "$ACCOUNT" -s "$CANCELLER_SERVICE" -w >"$temp_dir/canceller-password"

export BASE_SEPOLIA_DEPLOYER_PASSWORD_FILE="$temp_dir/deployer-password"
export BASE_SEPOLIA_SIGNER_PASSWORD_FILE="$temp_dir/signer-password"
export BASE_SEPOLIA_CANCELLER_PASSWORD_FILE="$temp_dir/canceller-password"
exec "$SCRIPT_DIR/experiment.sh" "$@"
