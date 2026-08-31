#!/usr/bin/env bash
# Install and initialize the fixed production Bridge Canister exactly once.
set -euo pipefail

ROOT="$(CDPATH='' cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd -P)"

# This one-shot recovery authorization is deliberately incident-specific. It
# binds the empty reservation left by the interrupted first production install
# to the exact reviewed plan and payload rather than trusting caller-supplied
# provenance.
RECOVERY_INSTALLED_SOURCE_REVISION="d85b7ce8c71e2f85faee0e97cc3cdd7c0eff7dcc"
RECOVERY_INSTALLED_SOURCE_TREE_SHA256="dad0d1ce7ec6b882a4fcab2d352ec36c44e68830d82b61f72c87ee631a5d4d32"
RECOVERY_PLAN_SHA256="1502cf12db494fb1825befb23bdbe546eab1b918cbc60a22eef3a102f3237846"
RECOVERY_PLAN_FILE_SHA256="3a5c2315f2cf3a638d7bd669e87f7226dd7b40d336ebf07d2fd3c7ee89e52950"
RECOVERY_INIT_CANDID_SHA256="af8161748938f474b326bd2bd3c54b594be2b79a63ec623fedd9261d2aa57abc"
RECOVERY_WASM_SHA256="746d8e2fe7115e519bc1257b0974251d51eaf647c8ff71e1d9c384b0753446bd"
RECOVERY_CANISTER_ID="lb5i5-ziaaa-aaaar-qcgwq-cai"
RECOVERY_INSTALLER_PRINCIPAL="lqfvd-m7ihy-e5dvc-gngvr-blzbt-pupeq-6t7ua-r7v4p-bvqjw-ea7gl-4qe"
RECOVERY_RECEIPT_PATH="/Volumes/KINGSTON/KINIC/bridge-production-prep-d85b7ce/production-canister-install-receipt.json"

source_git() {
  local global_option=""
  if [[ "${1:-}" == --attr-source=* ]]; then
    global_option="$1"
    shift
  fi
  /usr/bin/env -i HOME=/var/empty XDG_CONFIG_HOME=/var/empty \
    PATH=/usr/bin:/bin:/usr/sbin:/sbin LC_ALL=C GIT_TERMINAL_PROMPT=0 \
    GIT_NO_REPLACE_OBJECTS=1 GIT_NO_LAZY_FETCH=1 GIT_OPTIONAL_LOCKS=0 \
    GIT_CONFIG_GLOBAL=/dev/null \
    GIT_CONFIG_SYSTEM=/dev/null GIT_CONFIG_NOSYSTEM=1 GIT_ATTR_NOSYSTEM=1 \
    /usr/bin/git --no-optional-locks ${global_option:+"$global_option"} \
    -C "$ROOT" --git-dir="$ROOT/.git" --work-tree="$ROOT" \
    -c core.fsmonitor=false -c core.ignoreStat=false -c core.untrackedCache=false \
    -c core.trustctime=true -c core.checkStat=default \
    -c core.excludesFile=/dev/null -c core.attributesFile=/dev/null "$@"
}

project_icp() (
  [[ -n "${ICP_PROJECT_ROOT:-}" && -d "$ICP_PROJECT_ROOT" \
    && ! -L "$ICP_PROJECT_ROOT" ]] || {
    echo "production ICP project root is unavailable" >&2
    return 1
  }
  CDPATH='' cd -- "$ICP_PROJECT_ROOT" || return 1
  command icp "$@"
)

reject_git_environment_overrides() {
  local variable
  for variable in \
    GIT_DIR GIT_WORK_TREE GIT_INDEX_FILE GIT_COMMON_DIR \
    GIT_OBJECT_DIRECTORY GIT_ALTERNATE_OBJECT_DIRECTORIES GIT_NAMESPACE \
    GIT_NO_LAZY_FETCH GIT_OPTIONAL_LOCKS \
    GIT_SHALLOW_FILE GIT_GRAFT_FILE GIT_REPLACE_REF_BASE GIT_CONFIG \
    GIT_CONFIG_GLOBAL GIT_CONFIG_SYSTEM GIT_CONFIG_NOSYSTEM GIT_ATTR_NOSYSTEM \
    GIT_ATTR_SOURCE \
    GIT_CONFIG_COUNT \
    GIT_CONFIG_PARAMETERS GIT_EXEC_PATH GIT_IMPLICIT_WORK_TREE GIT_PREFIX \
    GIT_INTERNAL_SUPER_PREFIX GIT_EXTERNAL_DIFF GIT_EXTERNAL_DIFF_TRUST_EXIT_CODE \
    GIT_LITERAL_PATHSPECS GIT_GLOB_PATHSPECS GIT_NOGLOB_PATHSPECS \
    GIT_ICASE_PATHSPECS; do
    if declare -p "$variable" >/dev/null 2>&1; then
      echo "production source inspection rejects inherited $variable" >&2
      return 1
    fi
  done
}

reject_build_environment_overrides() {
  local variable
  while IFS= read -r variable; do
    case "$variable" in
      DYLD_*|LD_PRELOAD|LD_LIBRARY_PATH)
        echo "production validator execution rejects inherited $variable" >&2
        return 1
        ;;
    esac
  done < <(compgen -v)
}

reserve_output() {
  local target="$1" label="$2"
  [[ -n "$target" && ! -L "$target" && ! -e "$target" ]] || {
    echo "$label already exists or is a symlink" >&2
    return 1
  }
  python3 -I -S - "$target" "$label" <<'PY'
import os, sys
target, label = os.path.abspath(sys.argv[1]), sys.argv[2]
parent, name = os.path.dirname(target) or ".", os.path.basename(target)
directory = os.open(parent, os.O_RDONLY | getattr(os, "O_DIRECTORY", 0) | getattr(os, "O_NOFOLLOW", 0))
try:
    fd = os.open(name, os.O_WRONLY | os.O_CREAT | os.O_EXCL | getattr(os, "O_NOFOLLOW", 0), 0o600, dir_fd=directory)
    try:
        os.fchmod(fd, 0o600)
        os.fsync(fd)
    finally:
        os.close(fd)
    os.fsync(directory)
finally:
    os.close(directory)
PY
}

require_config_free_cargo_home() {
  python3 -I -S - "$1" <<'PY'
import os, sys
root = os.path.abspath(sys.argv[1])
for name in ("config", "config.toml"):
    path = os.path.join(root, name)
    if os.path.lexists(path):
        raise SystemExit("production validator build rejects Cargo home configuration")
PY
}

require_plain_git_index() {
  python3 -I -S - "$ROOT/.git" <<'PY'
import os, re, stat, sys
git_dir = sys.argv[1]
if any(re.fullmatch(r"sharedindex\.[0-9a-fA-F]{40,64}", name) for name in os.listdir(git_dir)):
    raise SystemExit("production source must not use a split Git index")
path = os.path.join(git_dir, "index")
fd = os.open(path, os.O_RDONLY | getattr(os, "O_NOFOLLOW", 0))
try:
    info = os.fstat(fd)
    if not stat.S_ISREG(info.st_mode) or info.st_size <= 0 or info.st_size > 128 * 1024 * 1024:
        raise SystemExit("production source Git index is not a bounded regular file")
    value = bytearray()
    while len(value) <= 128 * 1024 * 1024:
        chunk = os.read(fd, min(1024 * 1024, 128 * 1024 * 1024 + 1 - len(value)))
        if not chunk:
            break
        value.extend(chunk)
    if len(value) != info.st_size:
        raise SystemExit("production source Git index changed while it was inspected")
    if b"FSMN" in value:
        raise SystemExit("production source must not use Git fsmonitor index state")
finally:
    os.close(fd)
PY
}

require_clean_source_without_history_overrides() {
  local alternates assume_or_skip_flags common_dir commondir git_dir grafts http_alternates \
    ignored_or_dirty info_attributes local_overrides metadata replacement_refs shallow \
    source_root submodule_status worktree_config worktrees
  [[ -d "$ROOT/.git" && ! -L "$ROOT/.git" ]] || {
    echo "production source root must use its own ordinary .git directory" >&2
    return 1
  }
  reject_git_environment_overrides || return 1
  require_plain_git_index || return 1
  [[ -z "${GIT_REPLACE_REF_BASE:-}" ]] || {
    echo "production source must not override the Git replacement ref namespace" >&2
    return 1
  }
  commondir="$ROOT/.git/commondir"
  alternates="$ROOT/.git/objects/info/alternates"
  http_alternates="$ROOT/.git/objects/info/http-alternates"
  info_attributes="$ROOT/.git/info/attributes"
  worktree_config="$ROOT/.git/config.worktree"
  worktrees="$ROOT/.git/worktrees"
  shallow="$ROOT/.git/shallow"
  for metadata in "$commondir" "$alternates" "$http_alternates" \
    "$info_attributes" "$worktree_config" "$worktrees" "$shallow"; do
    [[ ! -e "$metadata" && ! -L "$metadata" ]] || {
      echo "production Git metadata uses an external or per-worktree override" >&2
      return 1
    }
  done
  source_root="$(source_git rev-parse --show-toplevel)" || return 1
  git_dir="$(source_git rev-parse --absolute-git-dir)" || return 1
  common_dir="$(source_git rev-parse --git-common-dir)" || return 1
  [[ "$source_root" == "$ROOT" && "$git_dir" == "$ROOT/.git" \
    && "$common_dir" == "$ROOT/.git" ]] || {
    echo "production Git metadata is not bound to the actual source root" >&2
    return 1
  }
  local_overrides="$(source_git config --local --no-includes --name-only --list \
    | awk '{ name=tolower($0); if (name ~ /^include\./ || name ~ /^includeif\./ \
      || name == "core.attributesfile" || name == "core.excludesfile" \
      || name == "core.worktree" || name ~ /^tar\./ \
      || name == "extensions.worktreeconfig" || name == "extensions.partialclone" \
      || name ~ /^remote\..*\.promisor$/ || name ~ /^remote\..*\.partialclonefilter$/) print }')" || return 1
  [[ -z "$local_overrides" ]] || {
    echo "production Git config contains source or object loading overrides" >&2
    return 1
  }
  replacement_refs="$(source_git for-each-ref --format='%(refname)' refs/replace)" || return 1
  grafts="$(source_git rev-parse --git-path info/grafts)" || return 1
  [[ -z "$replacement_refs" && ! -e "$grafts" && ! -L "$grafts" ]] || {
    echo "production source must not use Git replacement objects or grafts" >&2
    return 1
  }
  assume_or_skip_flags="$(source_git ls-files -v \
    | awk 'substr($0, 1, 1) ~ /^[a-zS]$/ { print }')" || return 1
  [[ -z "$assume_or_skip_flags" ]] || {
    echo "production source index contains hidden worktree state" >&2
    return 1
  }
  ignored_or_dirty="$(source_git status --porcelain=v1 --untracked-files=all \
    --ignored=matching --ignore-submodules=none)" || return 1
  [[ -z "$ignored_or_dirty" ]] || {
    echo "production source contains tracked, untracked, or ignored files outside its Git history" >&2
    return 1
  }
  submodule_status="$(source_git submodule status --recursive 2>/dev/null)" || {
    echo "failed to inspect production source submodules" >&2
    return 1
  }
  [[ -z "$(printf '%s\n' "$submodule_status" | sed -nE '/^[+-U]/p')" ]] || {
    echo "production source has an uninitialized or non-recorded submodule revision" >&2
    return 1
  }
  replacement_refs="$(source_git for-each-ref --format='%(refname)' refs/replace)" || return 1
  [[ -z "$replacement_refs" && ! -e "$grafts" && ! -L "$grafts" ]] || {
    echo "Git replacement state changed while production source was inspected" >&2
    return 1
  }
}

canonical_new_output_path() {
  python3 -I -S - "$ROOT" "$1" <<'PY'
import ctypes, os, stat, sys
root, requested = sys.argv[1:]
if not requested or os.path.basename(requested) in ("", ".", ".."):
    raise SystemExit("production receipt path is invalid")
root = os.path.realpath(root)
parent = os.path.abspath(os.path.dirname(requested) or ".")
real_parent = os.path.realpath(parent)
if not os.path.isdir(real_parent):
    raise SystemExit("production receipt parent must be an existing directory")
info = os.stat(real_parent, follow_symlinks=False)
if not stat.S_ISDIR(info.st_mode) or info.st_uid != os.getuid() or stat.S_IMODE(info.st_mode) & 0o022:
    raise SystemExit("production receipt parent must be owner-controlled and not group/other writable")
if sys.platform == "darwin":
    class Fsid(ctypes.Structure):
        _fields_ = [("value", ctypes.c_int32 * 2)]
    class Statfs(ctypes.Structure):
        _fields_ = [
            ("f_bsize", ctypes.c_uint32), ("f_iosize", ctypes.c_int32),
            ("f_blocks", ctypes.c_uint64), ("f_bfree", ctypes.c_uint64),
            ("f_bavail", ctypes.c_uint64), ("f_files", ctypes.c_uint64),
            ("f_ffree", ctypes.c_uint64), ("f_fsid", Fsid),
            ("f_owner", ctypes.c_uint32), ("f_type", ctypes.c_uint32),
            ("f_flags", ctypes.c_uint32), ("f_fssubtype", ctypes.c_uint32),
            ("f_fstypename", ctypes.c_char * 16),
            ("f_mntonname", ctypes.c_char * 1024),
            ("f_mntfromname", ctypes.c_char * 1024),
            ("f_flags_ext", ctypes.c_uint32), ("f_reserved", ctypes.c_uint32 * 7),
        ]
    mounted = Statfs()
    libc = ctypes.CDLL(None, use_errno=True)
    libc.statfs.argtypes = [ctypes.c_char_p, ctypes.POINTER(Statfs)]
    libc.statfs.restype = ctypes.c_int
    if libc.statfs(os.fsencode(real_parent), ctypes.byref(mounted)) != 0:
        error = ctypes.get_errno()
        raise OSError(error, os.strerror(error), real_parent)
    if mounted.f_flags & 0x00200000:
        raise SystemExit("production receipt filesystem must enforce file ownership")
candidate = os.path.join(real_parent, os.path.basename(requested))
root_info = os.stat(root)
cursor = real_parent
while True:
    cursor_info = os.stat(cursor, follow_symlinks=False)
    if (cursor_info.st_dev, cursor_info.st_ino) == (root_info.st_dev, root_info.st_ino):
        raise SystemExit("production receipt must be outside the source checkout")
    if cursor_info.st_uid not in (0, os.getuid()):
        raise SystemExit("production receipt ancestors must be owned by root or the executor")
    writable_by_others = stat.S_IMODE(cursor_info.st_mode) & 0o022
    root_sticky = cursor_info.st_uid == 0 and cursor_info.st_mode & stat.S_ISVTX
    if writable_by_others and not root_sticky:
        raise SystemExit("production receipt ancestors must prevent group/other path replacement")
    parent_cursor = os.path.dirname(cursor)
    if parent_cursor == cursor:
        break
    cursor = parent_cursor
print(candidate)
PY
}

reservation_identity() {
  python3 -I -S - "$1" <<'PY'
import os, stat, sys
fd = os.open(sys.argv[1], os.O_RDONLY | getattr(os, "O_NOFOLLOW", 0))
try:
    info = os.fstat(fd)
    if not stat.S_ISREG(info.st_mode) or info.st_size != 0 or stat.S_IMODE(info.st_mode) != 0o600:
        raise SystemExit("reservation must be an empty mode-0600 regular file")
    print(info.st_dev, info.st_ino)
finally:
    os.close(fd)
PY
}

publish_new_artifact() {
  python3 -I -S - "$1" "$2" "$3" <<'PY'
import hashlib, os, re, stat, sys
staged, target = map(os.path.abspath, sys.argv[1:3])
expected = sys.argv[3].lower()
if not re.fullmatch(r"[0-9a-f]{64}", expected):
    raise SystemExit("invalid expected artifact SHA-256")
parent = os.path.dirname(target) or "."
if os.path.dirname(staged) != parent or os.path.realpath(parent) != parent:
    raise SystemExit("staged and final artifacts must share one non-symlink directory")
staged_name, target_name = os.path.basename(staged), os.path.basename(target)
directory = os.open(parent, os.O_RDONLY | getattr(os, "O_DIRECTORY", 0) | getattr(os, "O_NOFOLLOW", 0))
def digest(fd):
    os.lseek(fd, 0, os.SEEK_SET)
    value = hashlib.sha256()
    while True:
        chunk = os.read(fd, 1024 * 1024)
        if not chunk:
            return value.hexdigest()
        value.update(chunk)
def remove_target_if_identity(identity):
    try:
        current = os.stat(target_name, dir_fd=directory, follow_symlinks=False)
    except FileNotFoundError:
        return
    if (current.st_dev, current.st_ino) == identity:
        os.unlink(target_name, dir_fd=directory)
        os.fsync(directory)
try:
    source = os.open(staged_name, os.O_RDONLY | getattr(os, "O_NOFOLLOW", 0), dir_fd=directory)
    try:
        source_info = os.fstat(source)
        if not stat.S_ISREG(source_info.st_mode) or digest(source) != expected:
            raise SystemExit("staged artifact differs from its reviewed SHA-256")
        try:
            os.stat(target_name, dir_fd=directory, follow_symlinks=False)
        except FileNotFoundError:
            pass
        else:
            raise SystemExit("final artifact already exists")
        current = os.stat(staged_name, dir_fd=directory, follow_symlinks=False)
        if (current.st_dev, current.st_ino) != (source_info.st_dev, source_info.st_ino):
            raise SystemExit("staged artifact path changed before publication")
        os.link(staged_name, target_name, src_dir_fd=directory, dst_dir_fd=directory, follow_symlinks=False)
        final = os.open(target_name, os.O_RDONLY | getattr(os, "O_NOFOLLOW", 0), dir_fd=directory)
        try:
            final_info = os.fstat(final)
            final_identity = (final_info.st_dev, final_info.st_ino)
            if ((final_info.st_dev, final_info.st_ino) != (source_info.st_dev, source_info.st_ino)
                    or not stat.S_ISREG(final_info.st_mode) or digest(final) != expected
                    or digest(source) != expected):
                remove_target_if_identity(final_identity)
                raise SystemExit("published artifact is not the verified staged inode")
            os.fsync(final)
        finally:
            os.close(final)
        os.fsync(directory)
        staged_current = os.stat(staged_name, dir_fd=directory, follow_symlinks=False)
        target_current = os.stat(target_name, dir_fd=directory, follow_symlinks=False)
        if ((staged_current.st_dev, staged_current.st_ino) != (source_info.st_dev, source_info.st_ino)
                or (target_current.st_dev, target_current.st_ino) != (source_info.st_dev, source_info.st_ino)):
            remove_target_if_identity((source_info.st_dev, source_info.st_ino))
            raise SystemExit("staged artifact path changed during publication")
        os.unlink(staged_name, dir_fd=directory)
        os.fsync(directory)
        print(source_info.st_dev, source_info.st_ino)
    finally:
        os.close(source)
finally:
    os.close(directory)
PY
}

verify_published_artifact() {
  python3 -I -S - "$1" "$2" "${3:-}" "${4:-}" <<'PY'
import hashlib, os, re, stat, sys
path, expected, expected_dev, expected_ino = sys.argv[1], sys.argv[2].lower(), *sys.argv[3:]
if not re.fullmatch(r"[0-9a-f]{64}", expected):
    raise SystemExit("invalid expected published SHA-256")
fd = os.open(path, os.O_RDONLY | getattr(os, "O_NOFOLLOW", 0))
try:
    info = os.fstat(fd)
    value = hashlib.sha256()
    while True:
        chunk = os.read(fd, 1024 * 1024)
        if not chunk: break
        value.update(chunk)
    if (not stat.S_ISREG(info.st_mode) or value.hexdigest() != expected
            or (expected_dev and (info.st_dev, info.st_ino) != (int(expected_dev), int(expected_ino)))):
        raise SystemExit("published artifact no longer matches its reviewed SHA-256")
    current = os.stat(path, follow_symlinks=False)
    if (current.st_dev, current.st_ino) != (info.st_dev, info.st_ino):
        raise SystemExit("published artifact path changed during verification")
    print(info.st_dev, info.st_ino)
finally:
    os.close(fd)
PY
}

remove_matching_reservation() {
  python3 -I -S - "$1" "$2" "$3" <<'PY'
import os, stat, sys
path, expected_dev, expected_ino = sys.argv[1], int(sys.argv[2]), int(sys.argv[3])
parent, name = os.path.dirname(os.path.abspath(path)) or ".", os.path.basename(path)
directory = os.open(parent, os.O_RDONLY | getattr(os, "O_DIRECTORY", 0) | getattr(os, "O_NOFOLLOW", 0))
try:
    fd = os.open(name, os.O_RDONLY | getattr(os, "O_NOFOLLOW", 0), dir_fd=directory)
    try:
        info = os.fstat(fd)
        if (not stat.S_ISREG(info.st_mode) or info.st_size != 0
                or stat.S_IMODE(info.st_mode) != 0o600
                or (info.st_dev, info.st_ino) != (expected_dev, expected_ino)):
            raise SystemExit("reservation identity changed before cleanup")
        current = os.stat(name, dir_fd=directory, follow_symlinks=False)
        if (current.st_dev, current.st_ino) != (expected_dev, expected_ino):
            raise SystemExit("reservation path changed before cleanup")
        os.unlink(name, dir_fd=directory)
        os.fsync(directory)
    finally:
        os.close(fd)
finally:
    os.close(directory)
PY
}

PLAN=""
WASM=""
RECEIPT=""
RESUME_POST_INSTALL=false
while [[ "$#" -gt 0 ]]; do
  case "$1" in
    --plan) [[ "$#" -ge 2 ]] || exit 2; PLAN="$2"; shift 2 ;;
    --wasm) [[ "$#" -ge 2 ]] || exit 2; WASM="$2"; shift 2 ;;
    --receipt) [[ "$#" -ge 2 ]] || exit 2; RECEIPT="$2"; shift 2 ;;
    --resume-post-install) RESUME_POST_INSTALL=true; shift ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
done

: "${BRIDGE_ICP_IDENTITY:?missing reviewed ICP CLI identity}"
[[ -f "$PLAN" && -f "$WASM" && -n "$RECEIPT" ]] || {
  echo "usage: BRIDGE_ICP_IDENTITY=NAME $0 --plan PLAN.json --wasm bridge-canister.wasm --receipt RECEIPT.json [--resume-post-install]" >&2
  exit 2
}
RECEIPT="$(canonical_new_output_path "$RECEIPT")"
RESERVATION="$RECEIPT.reservation"
RECOVERY_EVIDENCE="$RECEIPT.recovery.json"
RECOVERY_RESERVATION="$RECOVERY_EVIDENCE.reservation"
RESERVATION_DEVICE=""
RESERVATION_INODE=""
RECOVERY_RESERVATION_DEVICE=""
RECOVERY_RESERVATION_INODE=""
RECEIPT_DEVICE=""
RECEIPT_INODE=""
RECOVERY_EVIDENCE_DEVICE=""
RECOVERY_EVIDENCE_INODE=""
[[ ! -e "$RECEIPT" && ! -L "$RECEIPT" ]] || {
  echo "production Canister receipt already exists or is a symlink" >&2
  exit 2
}
if [[ "$RESUME_POST_INSTALL" == true ]]; then
  [[ "$RECEIPT" == "$RECOVERY_RECEIPT_PATH" ]] || {
    echo "post-install recovery requires the fixed incident receipt path" >&2
    exit 2
  }
  [[ ! -e "$RECOVERY_EVIDENCE" && ! -L "$RECOVERY_EVIDENCE" \
    && ! -e "$RECOVERY_RESERVATION" && ! -L "$RECOVERY_RESERVATION" ]] || {
    echo "post-install recovery evidence or reservation already exists" >&2
    exit 2
  }
else
  [[ ! -e "$RESERVATION" && ! -L "$RESERVATION" ]] || {
    echo "production Canister install reservation already exists or is a symlink" >&2
    exit 2
  }
fi
for tool in cargo icp python3 shasum rustc; do
  command -v "$tool" >/dev/null || { echo "$tool is required" >&2; exit 1; }
done
[[ "$(icp --version)" == "icp 1.0.2" ]] || {
  echo "production Canister install requires icp 1.0.2" >&2
  exit 1
}
require_clean_source_without_history_overrides
reject_build_environment_overrides
ACCOUNT_HOME="$(python3 -I -S - <<'PY'
import os, pwd
print(pwd.getpwuid(os.getuid()).pw_dir)
PY
)"
CARGO_HOME_PATH="$ACCOUNT_HOME/.cargo"
require_config_free_cargo_home "$CARGO_HOME_PATH"
require_config_free_cargo_home "/.cargo"
CARGO_BIN="$(command -v cargo)"
RUSTC_BIN="$(command -v rustc)"
CURRENT_REVISION="$(source_git rev-parse HEAD)"
if [[ "$RESUME_POST_INSTALL" == true ]]; then
  [[ "$CURRENT_REVISION" != "$RECOVERY_INSTALLED_SOURCE_REVISION" \
    && "$(source_git rev-parse --verify \
      "${RECOVERY_INSTALLED_SOURCE_REVISION}^{commit}")" \
      == "$RECOVERY_INSTALLED_SOURCE_REVISION" \
    && "$(source_git --attr-source="$RECOVERY_INSTALLED_SOURCE_REVISION" \
      archive --format=tar "$RECOVERY_INSTALLED_SOURCE_REVISION" \
      | shasum -a 256 | awk '{print $1}')" \
      == "$RECOVERY_INSTALLED_SOURCE_TREE_SHA256" ]] || {
    echo "post-install recovery source is not based on the authorized interrupted install" >&2
    exit 1
  }
  source_git merge-base --is-ancestor \
    "$RECOVERY_INSTALLED_SOURCE_REVISION" "$CURRENT_REVISION" || {
    echo "post-install recovery source does not descend from the installed source" >&2
    exit 1
  }
  source_git diff --quiet --no-ext-diff --ignore-submodules=none \
    "$RECOVERY_INSTALLED_SOURCE_REVISION..$CURRENT_REVISION" -- . \
    ':(exclude)scripts/production-canister-install.sh' \
    ':(exclude)scripts/test_production_canister_install.sh' \
    ':(exclude)docs/runbooks/operations.md' || {
    echo "post-install recovery source changes Canister or release inputs" >&2
    exit 1
  }
fi

OUTPUT_PARENT="$(dirname -- "$RECEIPT")"
WORK_ROOT="$(mktemp -d "$OUTPUT_PARENT/.bridge-canister-install.XXXXXX")"
TARGET="$WORK_ROOT/target"
INPUTS="$WORK_ROOT/inputs"
SOURCE_SNAPSHOT="$WORK_ROOT/source"
ICP_PROJECT_ROOT="$WORK_ROOT/icp-project"
SOURCE_ARCHIVE="$WORK_ROOT/source.tar"
mkdir -m 700 "$TARGET" "$INPUTS" "$SOURCE_SNAPSHOT" "$ICP_PROJECT_ROOT"
RECEIPT_TMP="$RECEIPT.tmp.$$"
RECOVERY_EVIDENCE_TMP="$RECOVERY_EVIDENCE.tmp.$$"
trap 'chmod -R u+w "$SOURCE_SNAPSHOT" 2>/dev/null || true; rm -rf "$WORK_ROOT"; rm -f "$RECEIPT_TMP" "$RECOVERY_EVIDENCE_TMP"' EXIT
source_git --attr-source="$CURRENT_REVISION" archive --format=tar \
  --output="$SOURCE_ARCHIVE" "$CURRENT_REVISION"
chmod 400 "$SOURCE_ARCHIVE"
CURRENT_TREE="$(shasum -a 256 "$SOURCE_ARCHIVE" | awk '{print tolower($1)}')"
env -u TAR_OPTIONS COPYFILE_DISABLE=1 /usr/bin/tar -xf "$SOURCE_ARCHIVE" -C "$SOURCE_SNAPSHOT"
[[ "$(shasum -a 256 "$SOURCE_ARCHIVE" | awk '{print tolower($1)}')" == "$CURRENT_TREE" ]] || {
  echo "production source archive changed while it was extracted" >&2
  exit 1
}
env -u TAR_OPTIONS COPYFILE_DISABLE=1 /usr/bin/tar -xf "$SOURCE_ARCHIVE" -C "$ICP_PROJECT_ROOT"
[[ "$(shasum -a 256 "$SOURCE_ARCHIVE" | awk '{print tolower($1)}')" == "$CURRENT_TREE" ]] || {
  echo "production source archive changed while the ICP project was isolated" >&2
  exit 1
}
chmod -R a-w "$SOURCE_SNAPSHOT"
TOOLCHAIN="$(python3 -I -S - "$SOURCE_SNAPSHOT/rust-toolchain.toml" <<'PY'
import re, sys
text = open(sys.argv[1], encoding="utf-8").read()
values = re.findall(r'^\s*channel\s*=\s*"([0-9]+\.[0-9]+\.[0-9]+)"\s*$', text, re.MULTILINE)
if len(values) != 1:
    raise SystemExit("production Rust toolchain must use one exact release channel")
print(values[0])
PY
)"
BUILD_TMP="$TARGET/tmp"
mkdir -m 700 "$BUILD_TMP"
python3 -I -S - "$PLAN" "$WASM" "$INPUTS/production-canister-plan.json" "$INPUTS/bridge-canister.wasm" <<'PY'
import os, stat, sys
for source, target, limit in ((sys.argv[1], sys.argv[3], 1024 * 1024), (sys.argv[2], sys.argv[4], 100 * 1024 * 1024)):
    fd = os.open(source, os.O_RDONLY | getattr(os, "O_NOFOLLOW", 0))
    try:
        info = os.fstat(fd)
        if not stat.S_ISREG(info.st_mode) or info.st_size <= 0 or info.st_size > limit:
            raise SystemExit("production Canister input is not a bounded regular file")
        chunks = []
        while True:
            chunk = os.read(fd, 1024 * 1024)
            if not chunk: break
            chunks.append(chunk)
    finally:
        os.close(fd)
    out = os.open(target, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o400)
    try:
        for chunk in chunks: os.write(out, chunk)
        os.fsync(out)
    finally:
        os.close(out)
PY
PLAN="$INPUTS/production-canister-plan.json"
WASM="$INPUTS/bridge-canister.wasm"
(
  CDPATH='' cd -- /
  [[ "$(/usr/bin/env -i HOME="$ACCOUNT_HOME" RUSTUP_HOME="$ACCOUNT_HOME/.rustup" \
    RUSTUP_TOOLCHAIN="$TOOLCHAIN" PATH=/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin \
    "$CARGO_BIN" --version)" == "cargo $TOOLCHAIN "* ]]
  [[ "$(/usr/bin/env -i HOME="$ACCOUNT_HOME" RUSTUP_HOME="$ACCOUNT_HOME/.rustup" \
    RUSTUP_TOOLCHAIN="$TOOLCHAIN" PATH=/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin \
    "$RUSTC_BIN" --version)" == "rustc $TOOLCHAIN "* ]]
  /usr/bin/env -i HOME="$ACCOUNT_HOME" CARGO_HOME="$CARGO_HOME_PATH" \
    RUSTUP_HOME="$ACCOUNT_HOME/.rustup" RUSTUP_TOOLCHAIN="$TOOLCHAIN" \
    CARGO_TARGET_DIR="$TARGET" CARGO_NET_OFFLINE=true CARGO_INCREMENTAL=0 \
    RUSTC="$RUSTC_BIN" TMPDIR="$BUILD_TMP" LC_ALL=C \
    PATH=/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin \
    "$CARGO_BIN" build --frozen --quiet --release \
    --manifest-path "$SOURCE_SNAPSHOT/Cargo.toml" -p bridge-profile
)
PROFILE_BIN="$TARGET/release/bridge-profile"
[[ -x "$PROFILE_BIN" ]] || { echo "bridge-profile build did not produce an executable" >&2; exit 1; }
"$PROFILE_BIN" validate-production-canister-plan "$PLAN" >/dev/null
"$PROFILE_BIN" render-production-canister-inputs "$PLAN" "$INPUTS" >/dev/null

WASM_SHA256="$(shasum -a 256 "$WASM" | awk '{print tolower($1)}')"
read -r PLAN_REVISION PLAN_TREE CANISTER_ID PLAN_WASM < <(python3 -I -S - "$PLAN" <<'PY'
import json, sys
value = json.load(open(sys.argv[1], encoding="utf-8"))
print(value["source_revision"], value["source_tree_sha256"].lower(), value["bridge_canister_id"], value["bridge_canister_wasm_sha256"].lower())
PY
)
[[ "$PLAN_WASM" == "$WASM_SHA256" ]] || {
  echo "production Canister plan does not match the frozen Wasm" >&2
  exit 1
}
if [[ "$RESUME_POST_INSTALL" == true ]]; then
  read -r PLAN_SHA256 INIT_CANDID_SHA256 < <(python3 -I -S - "$INPUTS/production-canister-install-inputs.json" <<'PY'
import json, sys
value = json.load(open(sys.argv[1], encoding="utf-8"))
print(value["plan_sha256"].lower(), value["init_candid_sha256"].lower())
PY
  )
  PLAN_FILE_SHA256="$(shasum -a 256 "$PLAN" | awk '{print tolower($1)}')"
  [[ "$PLAN_REVISION" == "$RECOVERY_INSTALLED_SOURCE_REVISION" \
    && "$PLAN_TREE" == "$RECOVERY_INSTALLED_SOURCE_TREE_SHA256" \
    && "$PLAN_SHA256" == "$RECOVERY_PLAN_SHA256" \
    && "$PLAN_FILE_SHA256" == "$RECOVERY_PLAN_FILE_SHA256" \
    && "$INIT_CANDID_SHA256" == "$RECOVERY_INIT_CANDID_SHA256" \
    && "$WASM_SHA256" == "$RECOVERY_WASM_SHA256" \
    && "$CANISTER_ID" == "$RECOVERY_CANISTER_ID" \
    && "$PLAN_REVISION" != "$CURRENT_REVISION" ]] || {
    echo "post-install recovery inputs do not exactly match the authorized interrupted install" >&2
    exit 1
  }
else
  [[ "$PLAN_REVISION" == "$CURRENT_REVISION" && "$PLAN_TREE" == "$CURRENT_TREE" ]] || {
    echo "production Canister plan/Wasm is not bound to the current clean source" >&2
    exit 1
  }
fi
MAPPED_ID="$(python3 -I -S - "$SOURCE_SNAPSHOT/.icp/data/mappings/production.ids.json" <<'PY'
import json, sys
value = json.load(open(sys.argv[1], encoding="utf-8"))
if not isinstance(value, dict) or not isinstance(value.get("bridge-canister"), str):
    raise SystemExit("production mapping does not contain bridge-canister")
print(value["bridge-canister"])
PY
)"
[[ "$MAPPED_ID" == "$CANISTER_ID" ]] || {
  echo "production mapping differs from the reviewed Canister plan" >&2
  exit 1
}
INSTALLER="$(project_icp identity principal --identity "$BRIDGE_ICP_IDENTITY" --project-root-override "$ICP_PROJECT_ROOT")"

if [[ "$RESUME_POST_INSTALL" == true ]]; then
  [[ "$INSTALLER" == "$RECOVERY_INSTALLER_PRINCIPAL" ]] || {
    echo "post-install recovery requires the authorized production installer principal" >&2
    exit 1
  }
  read -r RESERVATION_DEVICE RESERVATION_INODE < <(reservation_identity "$RESERVATION")
  reserve_output "$RECOVERY_RESERVATION" "post-install recovery reservation"
  read -r RECOVERY_RESERVATION_DEVICE RECOVERY_RESERVATION_INODE \
    < <(reservation_identity "$RECOVERY_RESERVATION")
fi

status_fields() {
  python3 -I -S - "$1" <<'PY'
import json, re, sys
value = json.loads(sys.argv[1])
def named(node, name):
    out=[]
    if isinstance(node, dict):
        for key, child in node.items():
            if key == name: out.append(child)
            out += named(child, name)
    elif isinstance(node, list):
        for child in node: out += named(child, name)
    return out
def bytes_value(node):
    if node is None: return None
    if isinstance(node, str):
        text=node.removeprefix("0x")
        if re.fullmatch(r"[0-9a-fA-F]{64}", text): return text.lower()
    if isinstance(node, list) and len(node)==32 and all(isinstance(v,int) and not isinstance(v,bool) and 0<=v<=255 for v in node):
        return bytes(node).hex()
    if isinstance(node, dict) and len(node)==1:
        return bytes_value(next(iter(node.values())))
    return None
controllers=named(value,"controllers")
modules=named(value,"module_hash")
if len(controllers)!=1 or not isinstance(controllers[0],list): raise SystemExit("ambiguous Canister controller status")
module=None
if len(modules)>1: raise SystemExit("ambiguous Canister module hash status")
if modules: module=bytes_value(modules[0])
print(",".join(str(v) for v in controllers[0]), module or "-")
PY
}

PRE_STATUS="$(project_icp canister status "$CANISTER_ID" -n ic --json --identity "$BRIDGE_ICP_IDENTITY" --project-root-override "$ICP_PROJECT_ROOT")"
read -r PRE_CONTROLLERS PRE_MODULE < <(status_fields "$PRE_STATUS")
if [[ "$RESUME_POST_INSTALL" == true ]]; then
  [[ "$PRE_CONTROLLERS" == "$INSTALLER" && "$PRE_MODULE" == "$WASM_SHA256" ]] || {
    echo "post-install recovery requires the approved installed module and sole installer controller" >&2
    exit 1
  }
else
  [[ "$PRE_CONTROLLERS" == "$INSTALLER" && "$PRE_MODULE" == "-" ]] || {
    echo "production Canister must be empty and controlled only by the installer" >&2
    exit 1
  }
fi

require_current_source_identity() {
  local context="$1"
  require_clean_source_without_history_overrides || return 1
  [[ "$(source_git rev-parse HEAD)" == "$CURRENT_REVISION" \
    && "$(source_git --attr-source="$CURRENT_REVISION" archive --format=tar "$CURRENT_REVISION" \
      | shasum -a 256 | awk '{print $1}')" == "$CURRENT_TREE" ]] || {
    echo "$context" >&2
    return 1
  }
}

require_current_source_identity "source changed after the production Canister inputs were frozen"
if [[ "$RESUME_POST_INSTALL" == false ]]; then
  reserve_output "$RESERVATION" "production Canister install reservation"
  read -r RESERVATION_DEVICE RESERVATION_INODE < <(reservation_identity "$RESERVATION")
  project_icp canister install "$CANISTER_ID" -n ic --mode install --wasm "$WASM" \
    --args-file "$INPUTS/canister-init.bin" --args-format bin --yes \
    --identity "$BRIDGE_ICP_IDENTITY" --project-root-override "$ICP_PROJECT_ROOT" || {
    echo "Canister install result is unknown; inspect live status and do not rerun" >&2
    exit 1
  }
fi

call_hex() {
  local method="$1" args="$2" query="${3:-false}"
  if [[ "$query" == true ]]; then
    project_icp canister call "$CANISTER_ID" "$method" "$args" -n ic --query \
      --identity "$BRIDGE_ICP_IDENTITY" --candid "$SOURCE_SNAPSHOT/canister/bridge-canister/bridge.did" \
      --output hex --project-root-override "$ICP_PROJECT_ROOT"
  else
    project_icp canister call "$CANISTER_ID" "$method" "$args" -n ic \
      --identity "$BRIDGE_ICP_IDENTITY" --candid "$SOURCE_SNAPSHOT/canister/bridge-canister/bridge.did" \
      --output hex --project-root-override "$ICP_PROJECT_ROOT"
  fi
}

INIT_RESPONSE="$(call_hex initialize_public_config '()')"
VALIDATION_RESPONSE="$(call_hex start_storage_validation '()')"
VALIDATION_COMPLETE="$("$PROFILE_BIN" storage-validation-complete "$VALIDATION_RESPONSE")"
[[ "$VALIDATION_COMPLETE" == true || "$VALIDATION_COMPLETE" == false ]] || {
  echo "storage validation completion result is malformed" >&2; exit 1;
}
for _ in {1..1024}; do
  [[ "$VALIDATION_COMPLETE" == true ]] && break
  VALIDATION_RESPONSE="$(call_hex continue_storage_validation '(100 : nat16)')"
  VALIDATION_COMPLETE="$("$PROFILE_BIN" storage-validation-complete "$VALIDATION_RESPONSE")"
  [[ "$VALIDATION_COMPLETE" == true || "$VALIDATION_COMPLETE" == false ]] || {
    echo "storage validation completion result is malformed" >&2; exit 1;
  }
done
[[ "$VALIDATION_COMPLETE" == true ]] || {
  echo "storage validation did not complete within the fixed bound" >&2; exit 1;
}
CHECKSUM_RESPONSE="$(call_hex refresh_storage_checksum '(4194304 : nat64)')"
CHECKSUM_COMPLETE="$("$PROFILE_BIN" storage-checksum-complete "$CHECKSUM_RESPONSE")"
[[ "$CHECKSUM_COMPLETE" == true || "$CHECKSUM_COMPLETE" == false ]] || {
  echo "storage checksum completion result is malformed" >&2; exit 1;
}
for _ in {1..1024}; do
  [[ "$CHECKSUM_COMPLETE" == true ]] && break
  CHECKSUM_RESPONSE="$(call_hex refresh_storage_checksum '(4194304 : nat64)')"
  CHECKSUM_COMPLETE="$("$PROFILE_BIN" storage-checksum-complete "$CHECKSUM_RESPONSE")"
  [[ "$CHECKSUM_COMPLETE" == true || "$CHECKSUM_COMPLETE" == false ]] || {
    echo "storage checksum completion result is malformed" >&2; exit 1;
  }
done
[[ "$CHECKSUM_COMPLETE" == true ]] || {
  echo "storage checksum did not complete within the fixed bound" >&2; exit 1;
}

RUNTIME_RESPONSE="$(call_hex get_runtime_binding '()' true)"
OPERATIONAL_RESPONSE="$(call_hex get_operational_config '()' true)"
CONTROL_PLANE_RESPONSE="$(call_hex get_control_plane_addresses '()' true)"
STATUS_RESPONSE="$(call_hex get_bridge_status '()' true)"
LIFECYCLE_RESPONSE="$(call_hex get_production_lifecycle '()' true)"
INTEGRITY_RESPONSE="$(call_hex storage_integrity_check '()' true)"
POST_STATUS="$(project_icp canister status "$CANISTER_ID" -n ic --json --identity "$BRIDGE_ICP_IDENTITY" --project-root-override "$ICP_PROJECT_ROOT")"
read -r POST_CONTROLLERS POST_MODULE < <(status_fields "$POST_STATUS")
[[ "$POST_CONTROLLERS" == "$INSTALLER" && "$POST_MODULE" == "$WASM_SHA256" ]] || {
  echo "installed module or controller set differs from the approved plan" >&2
  exit 1
}
require_current_source_identity "source changed while production Canister postconditions were collected"

"$PROFILE_BIN" write-production-canister-receipt "$PLAN" "$INSTALLER" "$POST_MODULE" \
  "$INIT_RESPONSE" "$VALIDATION_RESPONSE" "$CHECKSUM_RESPONSE" "$RUNTIME_RESPONSE" \
  "$OPERATIONAL_RESPONSE" "$CONTROL_PLANE_RESPONSE" "$STATUS_RESPONSE" \
  "$LIFECYCLE_RESPONSE" "$INTEGRITY_RESPONSE" \
  "$RECEIPT_TMP"
RECEIPT_SHA256="$(shasum -a 256 "$RECEIPT_TMP" | awk '{print tolower($1)}')"
PUBLICATION_STATUS="$(project_icp canister status "$CANISTER_ID" -n ic --json \
  --identity "$BRIDGE_ICP_IDENTITY" --project-root-override "$ICP_PROJECT_ROOT")"
read -r PUBLICATION_CONTROLLERS PUBLICATION_MODULE < <(status_fields "$PUBLICATION_STATUS")
[[ "$PUBLICATION_CONTROLLERS" == "$INSTALLER" && "$PUBLICATION_MODULE" == "$WASM_SHA256" ]] || {
  echo "module or controller changed before production Canister artifact publication" >&2
  exit 1
}
if [[ "$RESUME_POST_INSTALL" == true ]]; then
  DRIVER_SHA256="$(source_git cat-file blob "$CURRENT_REVISION:scripts/production-canister-install.sh" \
    | shasum -a 256 | awk '{print tolower($1)}')"
  python3 -I -S - "$RECOVERY_EVIDENCE_TMP" "$PLAN_REVISION" "$PLAN_TREE" \
    "$CURRENT_REVISION" "$CURRENT_TREE" "$PLAN_SHA256" "$PLAN_FILE_SHA256" \
    "$INIT_CANDID_SHA256" "$WASM_SHA256" "$DRIVER_SHA256" "$CANISTER_ID" \
    "$INSTALLER" "$PUBLICATION_CONTROLLERS" "$PUBLICATION_MODULE" "$RECEIPT_SHA256" \
    "$INIT_RESPONSE" "$VALIDATION_RESPONSE" \
    "$CHECKSUM_RESPONSE" "$RUNTIME_RESPONSE" "$OPERATIONAL_RESPONSE" \
    "$CONTROL_PLANE_RESPONSE" "$STATUS_RESPONSE" "$LIFECYCLE_RESPONSE" \
    "$INTEGRITY_RESPONSE" "$POST_STATUS" "$PUBLICATION_STATUS" <<'PY'
import hashlib, json, os, sys
(
    output, installed_revision, installed_tree, recovery_revision, recovery_tree,
    plan_sha256, plan_file_sha256, init_candid_sha256, wasm_sha256,
    driver_sha256, canister_id, installer, publication_controllers,
    publication_module_sha256, receipt_sha256, *responses,
) = sys.argv[1:]
names = (
    "initialize_public_config", "storage_validation", "storage_checksum",
    "runtime_binding", "operational_config", "control_plane_addresses",
    "bridge_status", "production_lifecycle", "storage_integrity",
    "management_status", "pre_publication_management_status",
)
if len(responses) != len(names):
    raise SystemExit("unexpected production recovery response count")
value = {
    "schema_version": 1,
    "kind": "production-canister-post-install-recovery",
    "installed_source_revision": installed_revision,
    "installed_source_tree_sha256": installed_tree,
    "recovery_source_revision": recovery_revision,
    "recovery_source_tree_sha256": recovery_tree,
    "recovery_driver_path": "scripts/production-canister-install.sh",
    "recovery_driver_sha256": driver_sha256,
    "plan_sha256": plan_sha256,
    "plan_file_sha256": plan_file_sha256,
    "init_candid_sha256": init_candid_sha256,
    "bridge_canister_wasm_sha256": wasm_sha256,
    "bridge_canister_id": canister_id,
    "installer_principal": installer,
    "pre_publication_controllers": publication_controllers.split(","),
    "pre_publication_module_sha256": publication_module_sha256,
    "storage_validation_max_rows": 100,
    "storage_checksum_max_bytes": 4194304,
    "response_sha256": {
        name: hashlib.sha256(response.encode()).hexdigest()
        for name, response in zip(names, responses)
    },
    "receipt_sha256": receipt_sha256,
}
encoded = (json.dumps(value, sort_keys=True, separators=(",", ":")) + "\n").encode()
fd = os.open(output, os.O_WRONLY | os.O_CREAT | os.O_EXCL | getattr(os, "O_NOFOLLOW", 0), 0o600)
try:
    view = memoryview(encoded)
    while view:
        written = os.write(fd, view)
        if written <= 0:
            raise SystemExit("short write while staging post-install recovery evidence")
        view = view[written:]
    os.fsync(fd)
finally:
    os.close(fd)
PY
  RECOVERY_EVIDENCE_SHA256="$(shasum -a 256 "$RECOVERY_EVIDENCE_TMP" | awk '{print tolower($1)}')"
  require_current_source_identity "source changed while production Canister recovery artifacts were staged"
  [[ "$(shasum -a 256 "$RECEIPT_TMP" | awk '{print tolower($1)}')" == "$RECEIPT_SHA256" ]] || {
    echo "staged production Canister receipt changed before recovery evidence publication" >&2
    exit 1
  }
  read -r RECOVERY_EVIDENCE_DEVICE RECOVERY_EVIDENCE_INODE \
    < <(publish_new_artifact "$RECOVERY_EVIDENCE_TMP" "$RECOVERY_EVIDENCE" \
      "$RECOVERY_EVIDENCE_SHA256")
fi
require_current_source_identity "source changed before production Canister receipt publication"
[[ "$(shasum -a 256 "$RECEIPT_TMP" | awk '{print tolower($1)}')" == "$RECEIPT_SHA256" ]] || {
  echo "staged production Canister receipt changed before publication" >&2
  exit 1
}
read -r RECEIPT_DEVICE RECEIPT_INODE \
  < <(publish_new_artifact "$RECEIPT_TMP" "$RECEIPT" "$RECEIPT_SHA256")
require_current_source_identity "source changed after production Canister artifact publication"
CLEANUP_STATUS="$(project_icp canister status "$CANISTER_ID" -n ic --json \
  --identity "$BRIDGE_ICP_IDENTITY" --project-root-override "$ICP_PROJECT_ROOT")"
read -r CLEANUP_CONTROLLERS CLEANUP_MODULE < <(status_fields "$CLEANUP_STATUS")
[[ "$CLEANUP_CONTROLLERS" == "$PUBLICATION_CONTROLLERS" \
  && "$CLEANUP_CONTROLLERS" == "$INSTALLER" \
  && "$CLEANUP_MODULE" == "$PUBLICATION_MODULE" \
  && "$CLEANUP_MODULE" == "$WASM_SHA256" ]] || {
  echo "module or controller changed before production Canister reservation cleanup" >&2
  exit 1
}
require_current_source_identity "source changed before production Canister reservation cleanup"
verify_published_artifact "$RECEIPT" "$RECEIPT_SHA256" \
  "$RECEIPT_DEVICE" "$RECEIPT_INODE" >/dev/null
if [[ "$RESUME_POST_INSTALL" == true ]]; then
  verify_published_artifact "$RECOVERY_EVIDENCE" "$RECOVERY_EVIDENCE_SHA256" \
    "$RECOVERY_EVIDENCE_DEVICE" "$RECOVERY_EVIDENCE_INODE" >/dev/null
  remove_matching_reservation "$RECOVERY_RESERVATION" \
    "$RECOVERY_RESERVATION_DEVICE" "$RECOVERY_RESERVATION_INODE"
fi
remove_matching_reservation "$RESERVATION" "$RESERVATION_DEVICE" "$RESERVATION_INODE"
if [[ "$RESUME_POST_INSTALL" == true ]]; then
  echo "Production Canister post-install recovery completed; receipt=$RECEIPT evidence=$RECOVERY_EVIDENCE"
else
  echo "Production Canister installed paused; receipt=$RECEIPT"
fi
