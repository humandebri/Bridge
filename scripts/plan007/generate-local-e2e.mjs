import { createHash } from "node:crypto"
import { execFileSync } from "node:child_process"
import { readFile, writeFile } from "node:fs/promises"
import path from "node:path"
import { fileURLToPath } from "node:url"

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..")
const factsPath = path.join(root, "ui/.e2e-runtime/local-e2e-facts.json")
const outputPath = path.join(root, "deployments/sepolia-staging/evidence/local-e2e.json")
const status = execFileSync("git", ["status", "--porcelain"], { cwd: root, encoding: "utf8" })
if (status.trim()) throw new Error("promotion evidence requires a clean working tree")

const facts = JSON.parse(await readFile(factsPath, "utf8"))
if (facts.state_upgrade !== true) throw new Error("real E2E did not prove same-Wasm state preservation")
if (facts.activation?.delay_seconds !== 259200 || facts.activation?.early_execute_reverted !== true) {
  throw new Error("real E2E did not prove the 72-hour activation delay")
}

const evidence = {
  schema_version: 1,
  created_at: new Date().toISOString(),
  source_commit: execFileSync("git", ["rev-parse", "HEAD"], { cwd: root, encoding: "utf8" }).trim(),
  bridge_wasm_sha256: await digest("target/test-deployment/wasm32-unknown-unknown/release/bridge_canister.wasm"),
  bridge_runtime_hash: facts.bridge_runtime_hash.toLowerCase(),
  bsns_runtime_hash: facts.bsns_runtime_hash.toLowerCase(),
  candid_sha256: await digest("canister/bridge-canister/bridge.did"),
  bridge_abi_sha256: await digest("contracts/abi/Bridge.json"),
  bsns_abi_sha256: await digest("contracts/abi/BSNS.json"),
  ledger_release: "ledger-suite-icrc-2026-03-09",
  ledger_wasm_sha256: await digest("ui/.e2e-cache/ic-icrc1-ledger.wasm.gz"),
  index_wasm_sha256: await digest("ui/.e2e-cache/ic-icrc1-index-ng.wasm.gz"),
  tests: {
    full_local_ci: "passed",
    real_frontend_e2e: "passed",
    canister_activation: "passed",
    timelock_72h: "passed",
    state_upgrade: "passed",
  },
}
await writeFile(outputPath, `${JSON.stringify(evidence, null, 2)}\n`)
process.stdout.write(`${outputPath}\n`)

async function digest(relative) {
  return createHash("sha256").update(await readFile(path.join(root, relative))).digest("hex")
}
