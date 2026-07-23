#!/usr/bin/env bash
# Shared fail-closed evidence validation for fixed production drivers.

production_validate_gate() {
  local mode="$1" bundle="$2" expected_hash="$3"
  local source_root target profile_bin output actual_hash revision tree manifest_revision manifest_tree
  source_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
  [[ -d "$source_root/.git" && -f "$bundle/release-manifest.json" ]] || { echo "release source or manifest is missing" >&2; return 1; }
  git -C "$source_root" diff --quiet --exit-code || { echo "release source has unstaged changes" >&2; return 1; }
  git -C "$source_root" diff --cached --quiet --exit-code || { echo "release source has staged changes" >&2; return 1; }
  [[ -z "$(git -C "$source_root" ls-files --others --exclude-standard)" ]] || { echo "release source has untracked files" >&2; return 1; }
  revision="$(git -C "$source_root" rev-parse HEAD)"
  tree="$(git -C "$source_root" archive HEAD | shasum -a 256 | awk '{print $1}')"
  read -r manifest_revision manifest_tree < <(python3 -c 'import json,sys;m=json.load(open(sys.argv[1]));print(m.get("source_revision",""),m.get("source_tree_sha256",""))' "$bundle/release-manifest.json")
  [[ "$revision" == "$manifest_revision" && "$tree" == "$(printf '%s' "$manifest_tree" | tr '[:upper:]' '[:lower:]')" ]] || { echo "release bundle is not bound to the fixed clean source" >&2; return 1; }
  [[ "$expected_hash" =~ ^[0-9a-fA-F]{64}$ ]] || { echo "invalid expected Gate manifest hash" >&2; return 1; }
  target="$(mktemp -d "${TMPDIR:-/tmp}/bridge-driver-validator.XXXXXX")"
  CARGO_TARGET_DIR="$target" cargo build --locked --quiet --release --manifest-path "$source_root/Cargo.toml" -p bridge-profile || { rm -rf "$target"; return 1; }
  profile_bin="$target/release/bridge-profile"
  if [[ "$mode" == gate-a ]]; then output="$("$profile_bin" validate-bundle --offline "$bundle")" || { rm -rf "$target"; return 1; }
  elif [[ "$mode" == gate-b ]]; then output="$("$profile_bin" verify-live "$bundle")" || { rm -rf "$target"; return 1; }
  else rm -rf "$target"; echo "invalid production gate mode" >&2; return 1
  fi
  rm -rf "$target"
  actual_hash="$(printf '%s\n' "$output" | sed -nE 's/.*manifest_sha256=([0-9a-fA-F]{64}).*/\1/p' | tail -n 1)"
  [[ -n "$actual_hash" && "$(printf '%s' "$actual_hash" | tr '[:upper:]' '[:lower:]')" == "$(printf '%s' "$expected_hash" | tr '[:upper:]' '[:lower:]')" ]] || { echo "driver Gate manifest hash mismatch" >&2; return 1; }
}

production_render_release_inputs() {
  local bundle="$1" output="$2" source_root target profile_bin
  source_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
  target="$(mktemp -d "${TMPDIR:-/tmp}/bridge-driver-renderer.XXXXXX")"
  CARGO_TARGET_DIR="$target" cargo build --locked --quiet --release --manifest-path "$source_root/Cargo.toml" -p bridge-profile || { rm -rf "$target"; return 1; }
  profile_bin="$target/release/bridge-profile"
  "$profile_bin" render-release-inputs "$bundle/profile.json" "$output" >/dev/null || { rm -rf "$target"; return 1; }
  rm -rf "$target"
  [[ -f "$output/canister-init.json" && -f "$output/contract-constructor-args.json" && -f "$output/release-inputs-manifest.json" ]] || { echo "fixed profile renderer did not produce every release input" >&2; return 1; }
}
