import { createHash } from "node:crypto"
import { mkdir, readFile, writeFile } from "node:fs/promises"
import path from "node:path"

const release = "ledger-suite-icrc-2026-03-09"
const artifacts = [
  { name: "ic-icrc1-ledger.wasm.gz", sha256: "354dd6ecfdc72b5409805b31dea22c9db11df6e14095a5a68924eb63535e6d8a" },
  { name: "ic-icrc1-index-ng.wasm.gz", sha256: "dab6808d0dfc06e5e88336d0c3d3e45e5448c6e36c2a781f3e9e09bd450f528c" },
]
const cache = path.resolve(import.meta.dirname, "../.e2e-cache")
await mkdir(cache, { recursive: true })

for (const artifact of artifacts) {
  const target = path.join(cache, artifact.name)
  let bytes
  try { bytes = await readFile(target) } catch { bytes = undefined }
  if (!bytes || digest(bytes) !== artifact.sha256) {
    const response = await fetch(`https://github.com/dfinity/ic/releases/download/${release}/${artifact.name}`)
    if (!response.ok) throw new Error(`Failed to download ${artifact.name}: HTTP ${response.status}`)
    bytes = Buffer.from(await response.arrayBuffer())
    if (digest(bytes) !== artifact.sha256) throw new Error(`${artifact.name} SHA-256 mismatch`)
    await writeFile(target, bytes)
  }
  if (digest(bytes) !== artifact.sha256) throw new Error(`${artifact.name} SHA-256 mismatch`)
  process.stdout.write(`${artifact.name} ${artifact.sha256}\n`)
}

function digest(bytes) { return createHash("sha256").update(bytes).digest("hex") }
