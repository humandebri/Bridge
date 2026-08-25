#!/usr/bin/env bash
# Install the reviewed official Solidity 0.8.36 binary for Certora.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DESTINATION="$ROOT/.tools/bin/certora-solc"
VERSION="0.8.36"
RELEASE_ROOT="https://github.com/argotorg/solidity/releases/download/v$VERSION"

os="$(uname -s)"
arch="$(uname -m)"
case "$os/$arch" in
  Darwin/arm64|Darwin/x86_64)
    asset="solc-macos"
    expected="d4abcf0b3e24b7948ddfd64c374d26c3214648717777790ecb936979054a129d"
    ;;
  Linux/x86_64)
    asset="solc-static-linux"
    expected="c8d35afdddc3cd2743ee88b8f25e0fecd16e2bdd5f2120f37e52cd9cc45ae0e6"
    ;;
  Linux/aarch64|Linux/arm64)
    asset="solc-static-linux-arm"
    expected="3907c0ad4e33650ced040473d7bd7d9837a7675bd40628236f9d97f9ab3657c5"
    ;;
  *)
    echo "unsupported Certora solc platform: $os/$arch" >&2
    exit 1
    ;;
esac

if [[ -x "$DESTINATION" ]] \
  && "$DESTINATION" --version 2>/dev/null | rg -q "Version: $VERSION"; then
  exit 0
fi

temporary="$(mktemp "${TMPDIR:-/tmp}/certora-solc.XXXXXX")"
trap 'rm -f "$temporary"' EXIT
curl --fail --location --retry 3 --proto '=https' --tlsv1.2 \
  "$RELEASE_ROOT/$asset" --output "$temporary"
if command -v sha256sum >/dev/null 2>&1; then
  actual="$(sha256sum "$temporary" | cut -d ' ' -f 1)"
else
  actual="$(shasum -a 256 "$temporary" | cut -d ' ' -f 1)"
fi
if [[ "$actual" != "$expected" ]]; then
  echo "Solidity $VERSION checksum mismatch: $actual" >&2
  exit 1
fi
mkdir -p "$(dirname "$DESTINATION")"
install -m 0755 "$temporary" "$DESTINATION"
"$DESTINATION" --version | rg -q "Version: $VERSION"
