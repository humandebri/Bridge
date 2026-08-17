#!/usr/bin/env bash
# Prepare nested bind-mount destinations without following candidate-controlled links.

if ! declare -p BRIDGE_CREATED_MOUNTPOINTS >/dev/null 2>&1; then
  BRIDGE_CREATED_MOUNTPOINTS=()
fi

bridge_prepare_mountpoint() {
  local root="${1:?missing mountpoint root}"
  local relative="${2:?missing relative mountpoint}"
  local component current
  local -a components

  [[ -d "$root" && ! -L "$root" ]] \
    || { echo "mountpoint root must be a real directory: $root" >&2; return 1; }
  case "$relative" in
    /*|.|..|*/.|*/..|*//*|"")
      echo "invalid relative mountpoint: $relative" >&2
      return 1
      ;;
  esac

  root="$(cd "$root" && pwd -P)"
  IFS='/' read -r -a components <<<"$relative"
  current="$root"
  for component in "${components[@]}"; do
    [[ -n "$component" && "$component" != "." && "$component" != ".." ]] \
      || { echo "invalid mountpoint component: $relative" >&2; return 1; }
    current="$current/$component"
    if [[ -L "$current" ]]; then
      echo "mountpoint component must not be a symlink: $current" >&2
      return 1
    fi
    if [[ -e "$current" ]]; then
      [[ -d "$current" ]] \
        || { echo "mountpoint component must be a directory: $current" >&2; return 1; }
      continue
    fi
    mkdir -- "$current"
    BRIDGE_CREATED_MOUNTPOINTS+=("$current")
  done
}

bridge_prepare_candidate_mountpoint() {
  local root="${1:?missing candidate root}"
  local relative="${2:?missing candidate mountpoint}"
  local tracked

  tracked="$(git -C "$root" ls-files -- "$relative")" \
    || { echo "cannot inspect candidate mountpoint: $relative" >&2; return 1; }
  if [[ -n "$tracked" ]]; then
    echo "candidate mountpoint hides tracked content: $relative" >&2
    return 1
  fi
  bridge_prepare_mountpoint "$root" "$relative"
}

bridge_cleanup_mountpoints() {
  local index path status=0

  for ((index=${#BRIDGE_CREATED_MOUNTPOINTS[@]} - 1; index >= 0; index--)); do
    path="${BRIDGE_CREATED_MOUNTPOINTS[$index]}"
    if [[ -L "$path" || ! -d "$path" ]]; then
      echo "created mountpoint changed before cleanup: $path" >&2
      status=1
      continue
    fi
    if ! rmdir -- "$path"; then
      echo "created mountpoint is not empty; preserving it: $path" >&2
      status=1
    fi
  done
  BRIDGE_CREATED_MOUNTPOINTS=()
  return "$status"
}
