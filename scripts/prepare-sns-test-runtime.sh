#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DEST="${BRIDGE_SNS_TEST_RUNTIME:-$ROOT/.tools/sns-test-runtime}"
[[ "$(didc --version)" == "didc 0.5.4" ]] || { echo "SNS test bindings require didc 0.5.4" >&2; exit 1; }
mkdir -p "$DEST"
python3 - "$DEST" <<'PY'
import hashlib,pathlib,sys,tarfile,gzip,subprocess,tempfile,urllib.request,os
p=pathlib.Path(sys.argv[1])
archive_path=p/'canisters.tar'
expected='7ccbc9db3cf8abc5da4bd657ddda4d1a6ec92b8cfe09372ecfe0780a372b3034'
if not archive_path.exists():
 temporary=None
 try:
  with tempfile.NamedTemporaryFile(dir=p, prefix='.canisters.', delete=False) as output:
   temporary=pathlib.Path(output.name)
   with urllib.request.urlopen('https://github.com/dfinity/ic/releases/download/release-2026-09-10_03-28--all-in-one-node/canisters.tar', timeout=120) as response:
    while chunk:=response.read(1024*1024):
     output.write(chunk)
  if hashlib.sha256(temporary.read_bytes()).hexdigest()!=expected:
   raise SystemExit('SNS test archive differs from the pinned official release')
  os.replace(temporary,archive_path)
 finally:
  if temporary is not None: temporary.unlink(missing_ok=True)
raw=archive_path.read_bytes()
if hashlib.sha256(raw).hexdigest()!='7ccbc9db3cf8abc5da4bd657ddda4d1a6ec92b8cfe09372ecfe0780a372b3034':
 raise SystemExit('SNS test archive differs from the pinned official release')
with tarfile.open(p/'canisters.tar') as archive:
 for name in ['sns-governance-canister','sns-root-canister']:
  (p/(name+'.wasm')).write_bytes(gzip.decompress(archive.extractfile(name+'.wasm.gz').read()))
  did=p/(name+'.did');did.write_bytes(archive.extractfile(name+'.wasm.gz.did').read())
  code=subprocess.check_output(['didc','bind',str(did),'-t','js'],text=True)
  (p/(name+'.cjs')).write_text(code.replace('export const idlFactory =','exports.idlFactory =').replace('export const init =','exports.init ='))
print(p)
PY
