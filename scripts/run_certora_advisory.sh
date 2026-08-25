#!/usr/bin/env bash
# Run pinned advisory Certora compilation or private cloud verification.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
MODE="${1:-}"
TARGET="${2:-all}"
CERTORA_PROJECT="$ROOT/verification/certora"
OUTPUT_ROOT="$ROOT/verification/output/certora"
export PATH="$ROOT/.tools/bin:$PATH"

redact_certora_output() {
  local source="$1"
  local destination="$2"
  CERTORA_REDACT_VALUE="${CERTORAKEY:-}" python3 - "$source" "$destination" <<'PY'
import os
import re
import sys
from pathlib import Path

source, destination = map(Path, sys.argv[1:])
text = source.read_text(encoding="utf-8", errors="replace")
secret = os.environ.get("CERTORA_REDACT_VALUE", "")
if secret:
    text = text.replace(secret, "[REDACTED_CERTORAKEY]")
text = re.sub(r"([?&][A-Za-z0-9_.~-]+=)[^&\s]+", r"\1[REDACTED]", text)
destination.write_text(text, encoding="utf-8")
PY
}

if [[ "${1:-}" == "--test-redaction" ]]; then
  [[ $# == 3 ]] || { echo "usage: $0 --test-redaction SOURCE DESTINATION" >&2; exit 2; }
  redact_certora_output "$2" "$3"
  cat "$3"
  exit 0
fi

case "$MODE" in compile|cloud) ;; *) echo "usage: $0 {compile|cloud} {bridge|bsns|timelock|all}" >&2; exit 2 ;; esac
case "$TARGET" in bridge|bsns|timelock|all) ;; *) echo "unknown Certora target: $TARGET" >&2; exit 2 ;; esac
if [[ "$MODE" == cloud && -z "${CERTORAKEY:-}" ]]; then
  echo "CERTORAKEY is required for a cloud verification" >&2
  exit 2
fi

python3 "$ROOT/scripts/check_certora_manifest.py"
mkdir -p "$OUTPUT_ROOT"

certora=("$CERTORA_PROJECT/.venv/bin/certoraRun")
solc=(certora-solc)
cli_version="$("${certora[@]}" --version 2>&1)"
if [[ "$cli_version" != *"8.17.1"* ]]; then
  echo "Certora CLI version mismatch: $cli_version" >&2
  exit 1
fi
solc_version="$("${solc[@]}" --version 2>&1)"
if [[ "$solc_version" != *"Version: 0.8.36"* ]]; then
  echo "solc version mismatch: $solc_version" >&2
  exit 1
fi
git_commit="$(git -C "$ROOT" rev-parse HEAD)"

targets=()
if [[ "$TARGET" == all ]]; then targets=(bridge bsns timelock); else targets=("$TARGET"); fi

for item in "${targets[@]}"; do
  case "$item" in
    bridge) config="$CERTORA_PROJECT/confs/Bridge.conf" ;;
    bsns) config="$CERTORA_PROJECT/confs/BSNS.conf" ;;
    timelock) config="$CERTORA_PROJECT/confs/BridgeTimelockController.conf" ;;
  esac
  baseline="$OUTPUT_ROOT/$item-fingerprint.json"
  log="$OUTPUT_ROOT/$item.log"
  summary="$OUTPUT_ROOT/$item-summary.json"
  raw_log="$(mktemp "${TMPDIR:-/tmp}/certora-$item.XXXXXX")"
  chmod 600 "$raw_log"
  trap 'rm -f "${raw_log:-}"' EXIT
  python3 "$ROOT/scripts/proof_fingerprint.py" --write "$baseline" >/dev/null
  started="$(date +%s)"
  job_status=0
  args=("$config")
  if [[ "$MODE" == compile ]]; then args+=(--compilation_steps_only); fi
  set +e
  (cd "$ROOT" && "${certora[@]}" "${args[@]}") >"$raw_log" 2>&1
  job_status=$?
  set -e
  finished="$(date +%s)"
  python3 "$ROOT/scripts/proof_fingerprint.py" --check "$baseline" >/dev/null

  public_report=0
  if rg -q 'anonymousKey=' "$raw_log"; then
    echo "Certora emitted a public anonymous report URL" >&2
    public_report=1
    job_status=1
  fi
  if rg -qi '(^|[^[:alpha:]])(TIMEOUT|UNKNOWN)([^[:alpha:]]|$)|sanity[^\n]*(fail|warning)|unresolved[^\n]*(assert|havoc)|optimizer steps missing' "$raw_log"; then
    echo "Certora reported timeout, unknown, sanity, or assertion-resolution problems" >&2
    job_status=1
  fi
  redact_certora_output "$raw_log" "$log"
  cat "$log"
  python3 - "$summary" "$item" "$MODE" "$job_status" "$started" "$finished" "$baseline" "$cli_version" "$solc_version" "$git_commit" "$ROOT" "$config" <<'PY'
import json
import re
import sys
from pathlib import Path

(
    path,
    target,
    mode,
    status,
    started,
    finished,
    baseline,
    cli,
    solc,
    commit,
    root,
    config_path,
) = sys.argv[1:]
fingerprint = json.loads(Path(baseline).read_text(encoding="utf-8"))
config = json.loads(Path(config_path).read_text(encoding="utf-8"))
spec_path = config["verify"].split(":", 1)[1]
spec = (Path(root) / spec_path).read_text(encoding="utf-8")
rules = re.findall(r"(?m)^\s*(?:rule|invariant)\s+([A-Za-z_][A-Za-z0-9_]*)\b", spec)
result = "PASS" if status == "0" and mode == "cloud" else (
    "COMPILED" if status == "0" else "RUN_FAILED"
)
Path(path).write_text(
    json.dumps(
        {
            "schema": 1,
            "target": target,
            "mode": mode,
            "status": "pass" if status == "0" else "fail",
            "git_commit": commit,
            "started_at_epoch": int(started),
            "duration_seconds": int(finished) - int(started),
            "source_fingerprint": fingerprint,
            "certora_cli": cli.strip(),
            "prover_version": "release/15June2026",
            "solc": solc.strip(),
            "rule_results": {rule: result for rule in rules},
        },
        indent=2,
    ) + "\n",
    encoding="utf-8",
)
PY
  rm -f "$raw_log"
  raw_log=""
  if (( job_status != 0 )); then
    exit "$job_status"
  fi
done
