#!/usr/bin/env bash
# Install checksum-pinned tools that are not provided by setup actions.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
MODE="${1:-all}"
BIN_DIR="$HOME/.local/bin"
mkdir -p "$BIN_DIR"

install_lean_toolchain() {
  local toolchain attempt
  toolchain="$(tr -d '\r\n' <"$ROOT/lean-toolchain")"
  if [[ -z "$toolchain" ]]; then
    echo "lean-toolchain is empty" >&2
    return 1
  fi

  if [[ -x "$HOME/.elan/toolchains/$toolchain/bin/lean" ]]; then
    echo "Lean toolchain already installed: $toolchain" >&2
    return 0
  fi

  for attempt in 1 2 3; do
    if "$HOME/.elan/bin/elan" toolchain install "$toolchain"; then
      return 0
    fi
    echo "Lean toolchain installation failed (attempt $attempt/3): $toolchain" >&2
    if [[ "$attempt" -eq 3 ]]; then
      return 1
    fi
    sleep 2
  done
}

install_didc() {
  if [[ -x "$BIN_DIR/didc" ]] && [[ "$("$BIN_DIR/didc" --version 2>/dev/null)" == "didc 0.5.4" ]]; then
    echo "didc already installed; skipping download" >&2
    return 0
  fi
  curl --proto '=https' --tlsv1.2 -LsSf \
    -o "$BIN_DIR/didc" \
    https://github.com/dfinity/candid/releases/download/2025-12-18/didc-linux64
  echo "32693c76d9c6fe0f273f2c1ebf7e48ba3c383e925e5afd17dd264a06aed9fbcc  $BIN_DIR/didc" \
    | sha256sum --check
  chmod +x "$BIN_DIR/didc"
  test "$("$BIN_DIR/didc" --version)" = "didc 0.5.4"
}

install_icp() {
  if command -v icp >/dev/null 2>&1 && [[ "$(icp --version 2>/dev/null)" == "icp 1.0.2" ]]; then
    echo "icp already installed; skipping download" >&2
    return 0
  fi
  local installer="/tmp/icp-cli-installer.sh"
  curl --proto '=https' --tlsv1.2 -LsSf \
    -o "$installer" \
    https://github.com/dfinity/icp-cli/releases/download/v1.0.2/icp-cli-installer.sh
  echo "e7e29ec7a99c854264477f8dcede86670ff3a854f035b6fac40d0c891b9cf70e  $installer" \
    | sha256sum --check
  ICP_CLI_DISABLE_UPDATE=1 ICP_CLI_NO_MODIFY_PATH=1 sh "$installer" --quiet
}

install_ic_wasm() {
  local installer="/tmp/ic-wasm-installer.sh"
  curl --proto '=https' --tlsv1.2 -LsSf \
    -o "$installer" \
    https://github.com/dfinity/ic-wasm/releases/download/0.10.0/ic-wasm-installer.sh
  echo "00c361c9c1d53ef464660c0e414cbaf50b602e21f16811fe4134077deaaecabb  $installer" \
    | sha256sum --check
  IC_WASM_NO_MODIFY_PATH=1 sh "$installer" --quiet
  test "$(ic-wasm --version)" = "ic-wasm 0.10.0"
}

install_solc() {
  local version="0.8.36"
  local release="solc-linux-amd64-v0.8.36+commit.8a079791"
  local svm_dir="$HOME/.svm/$version"
  local solc="$svm_dir/solc-$version"

  mkdir -p "$svm_dir"
  curl --proto '=https' --tlsv1.2 -LsSf \
    -o "$solc" \
    "https://binaries.soliditylang.org/linux-amd64/$release"
  echo "c8d35afdddc3cd2743ee88b8f25e0fecd16e2bdd5f2120f37e52cd9cc45ae0e6  $solc" \
    | sha256sum --check
  chmod +x "$solc"
  printf '%s\n' "$version" >"$HOME/.svm/.global-version"
  test "$("$solc" --version | sed -n 's/^Version: //p')" = "0.8.36+commit.8a079791.Linux.g++"
}

install_ci_tools() {
  install_didc
  install_icp
  # Ensure the mount destinations the isolation image expects exist even when
  # the proof toolchain (Lean/Verus/Z3) is intentionally not installed.
  mkdir -p "$HOME/.elan/toolchains"
}

install_proof_tools() {
  local z3_archive="/tmp/z3.zip"
  local verus_archive="/tmp/verus.zip"
  local elan_installer="/tmp/elan-init.sh"

  install_ci_tools
  # Bound the apt mirror refresh so a slow or unreachable mirror cannot stall
  # the whole toolchain for a bounded-grace retry loop; ripgrep is required by
  # the repository guards and the pinned isolation image already installs it.
  if ! command -v rg >/dev/null 2>&1; then
    sudo apt-get update \
      -o Acquire::Retries=2 \
      -o Acquire::http::Timeout=20 \
      -o Acquire::https::Timeout=20
    sudo apt-get install --yes ripgrep
  else
    echo "ripgrep already installed; skipping apt refresh" >&2
  fi

  if [[ ! -x "$BIN_DIR/z3" ]]; then
    curl --proto '=https' --tlsv1.2 -LsSf \
      -o "$z3_archive" \
      https://github.com/Z3Prover/z3/releases/download/z3-4.16.0/z3-4.16.0-x64-glibc-2.39.zip
    echo "7288c49a5bd6dbafd7b0b0d1f65956b91672da24b08f09242919af159be3418e  $z3_archive" \
      | sha256sum --check
    unzip -q "$z3_archive" -d "$HOME/.local/z3"
    ln -sf "$(find "$HOME/.local/z3" -type f -path '*/bin/z3' -print -quit)" "$BIN_DIR/z3"
  else
    echo "z3 already installed; skipping download" >&2
  fi

  if ! rustup toolchain list | rg -q '^1\.96\.0'; then
    rustup toolchain install 1.96.0 --profile minimal
  else
    echo "Rust 1.96.0 toolchain already installed; skipping" >&2
  fi

  if [[ ! -x "$BIN_DIR/verus" ]]; then
    curl --proto '=https' --tlsv1.2 -LsSf \
      -o "$verus_archive" \
      https://github.com/verus-lang/verus/releases/download/release/0.2026.07.05.49b8806/verus-0.2026.07.05.49b8806-x86-linux.zip
    echo "cb4fe7db423fdda5e9aa77b2c3e632f8a618b6a991509283aae591f0a914d34c  $verus_archive" \
      | sha256sum --check
    unzip -q "$verus_archive" -d "$HOME/.local/verus"
    ln -sf "$(find "$HOME/.local/verus" -type f -name verus -print -quit)" "$BIN_DIR/verus"
  else
    echo "verus already installed; skipping download" >&2
  fi

  if [[ ! -x "$HOME/.elan/bin/elan" ]]; then
    curl --proto '=https' --tlsv1.2 -LsSf \
      -o "$elan_installer" \
      https://raw.githubusercontent.com/leanprover/elan/6737edca3d2ca3dbaa1b47b87769b48b420633ae/elan-init.sh
    echo "a620ff1641616222c8d37c54845492004bb84d6877cdbc944dd65c1aa685bf53  $elan_installer" \
      | sha256sum --check
    sh "$elan_installer" -y --default-toolchain none
  else
    echo "elan already installed; skipping installer" >&2
  fi
  install_lean_toolchain
}

case "$MODE" in
  didc)
    install_didc
    ;;
  icp)
    install_icp
    install_ic_wasm
    ;;
  solc)
    install_solc
    ;;
  ci)
    install_ci_tools
    install_ic_wasm
    install_solc
    ;;
  proof)
    install_proof_tools
    ;;
  all)
    install_proof_tools
    install_ic_wasm
    install_solc
    ;;
  *)
    echo "usage: $0 {didc|icp|solc|ci|proof|all}" >&2
    exit 2
    ;;
esac

{
  echo "$BIN_DIR"
  if [[ "$MODE" == "proof" || "$MODE" == "all" ]]; then
    echo "$HOME/.elan/bin"
  fi
} >>"${GITHUB_PATH:?GITHUB_PATH is required}"
