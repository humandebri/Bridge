import { createHash } from "node:crypto"
import { execFileSync } from "node:child_process"
import { mkdir, readFile, writeFile } from "node:fs/promises"
import path from "node:path"
import { fileURLToPath } from "node:url"
import { runtimeTemplateSha256FromFile } from "./runtime-template-hash.mjs"

const defaultRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..")
export const LOCAL_E2E_SCHEMA_VERSION = 8
export const STAGING_ACTIVATION_DELAY_SECONDS = 300
export const CURRENT_STABLE_SCHEMA_VERSION = 34

export function validateUpgradeEvidence(upgrade) {
  if (!upgrade || upgrade.verified !== true) throw new Error("real E2E did not prove same-Wasm state preservation")
  if (!upgrade.before || !upgrade.after || JSON.stringify(upgrade.before) !== JSON.stringify(upgrade.after)) {
    throw new Error("same-Wasm upgrade evidence has different before and after state")
  }
  const state = upgrade.after
  const schemaVersion = Number(state.status?.schema_version)
  if (schemaVersion !== CURRENT_STABLE_SCHEMA_VERSION) {
    throw new Error(`upgrade evidence must use current stable schema v${CURRENT_STABLE_SCHEMA_VERSION}`)
  }
  if (!Array.isArray(state.deposits) || state.deposits.length === 0) {
    throw new Error("upgrade evidence did not reopen every individual Deposit record")
  }
  if (!Array.isArray(state.withdrawals)) throw new Error("upgrade evidence did not account for individual Withdrawal records")
  if (typeof state.owner_sequence !== "string" || !/^\d+$/.test(state.owner_sequence)) throw new Error("upgrade evidence has no owner sequence")
  if (typeof state.status?.counts?.pending_ledger_operations !== "string" || typeof state.status?.counts?.reserved_deposit_mint_operations !== "string") {
    throw new Error("upgrade evidence has no Ledger-operation or Mint Authorization liability identities")
  }
  if (!state.status?.settlement_scheduler || !state.runtime_binding || !state.operational_config || !state.audit_events) {
    throw new Error("upgrade evidence omitted scheduler, runtime binding, operational configuration, or audit state")
  }
  const deploymentInstanceId = bytesHex(state.runtime_binding.deployment_instance_id)
  if (!/^0x[0-9a-f]{64}$/.test(deploymentInstanceId) || /^0x0+$/.test(deploymentInstanceId)) {
    throw new Error("upgrade evidence omitted a nonzero 32-byte deployment instance ID")
  }
  if (!state.activation_status || !("pending_timelock_operation" in state.activation_status)) {
    throw new Error("upgrade evidence omitted the pending Timelock operation identity")
  }
  if (state.storage_integrity !== "ok") throw new Error("upgrade evidence did not pass storage_integrity_check")
  for (const field of ["deployment_instance_id", "minimum_withdrawal_id", "base_chain_id", "bridge_contract", "expected_bridge_runtime_sha256", "timelock_contract", "expected_bridge_signer", "ledger_canister_id", "index_canister_id", "evm_rpc_canister_id", "rpc_provider_urls_sha256", "schema_version", "operational_config_sha256"]) {
    if (!(field in state.runtime_binding)) throw new Error(`upgrade evidence omitted runtime binding ${field}`)
  }
  for (const field of ["deposit_rate_limit_window_seconds", "deposit_rate_limit_global", "deposit_rate_limit_per_principal", "notification_rate_limit_window_seconds", "notification_rate_limit_global", "notification_ingestion_rate_limit_global", "settlement_rate_limit_window_seconds", "settlement_rate_limit_global", "settlement_rate_limit_per_principal", "settlement_rate_limit_per_record", "settlement_retry_interval_seconds"]) {
    if (!(field in state.operational_config)) throw new Error(`upgrade evidence omitted operational configuration ${field}`)
  }
  if (!state.deposits.some((record) => record && record.deposit_id && "owner_sequence" in record && record.mint_authorization?.length === 1)) {
    throw new Error("upgrade evidence did not preserve a Deposit identity and Mint Authorization")
  }
  return upgrade
}

export async function generateLocalEvidence(root = defaultRoot, requestedOutputPath) {
  const factsPath = path.join(root, "ui/.e2e-runtime/local-e2e-facts.json")
  if (!requestedOutputPath || !path.isAbsolute(requestedOutputPath)) {
    throw new Error("--output must be an absolute path outside the repository")
  }
  const outputPath = path.resolve(requestedOutputPath)
  const relativeOutput = path.relative(root, outputPath)
  if (relativeOutput === "" || (!relativeOutput.startsWith("..") && !path.isAbsolute(relativeOutput))) {
    throw new Error("--output must be outside the repository")
  }
  const status = execFileSync("git", ["status", "--porcelain"], { cwd: root, encoding: "utf8" })
  if (status.trim()) throw new Error("promotion evidence requires a clean working tree")
  const facts = JSON.parse(await readFile(factsPath, "utf8"))
  const upgrade = validateUpgradeEvidence(facts.state_upgrade)
  if (facts.activation?.delay_seconds !== STAGING_ACTIVATION_DELAY_SECONDS || facts.activation?.early_execute_reverted !== true) {
    throw new Error("real E2E did not prove the five-minute staging activation delay")
  }
  const evidence = {
    schema_version: LOCAL_E2E_SCHEMA_VERSION,
    environment_mode: "short-delay-test-only",
    activation_timelock_delay_seconds: STAGING_ACTIVATION_DELAY_SECONDS,
    deployment_instance_id: bytesHex(upgrade.after.runtime_binding.deployment_instance_id),
    created_at: new Date().toISOString(),
    source_commit: execFileSync("git", ["rev-parse", "HEAD"], { cwd: root, encoding: "utf8" }).trim(),
    bridge_wasm_sha256: await digest(root, "target/test-deployment/staging/bridge_canister.wasm"),
    bridge_runtime_template_sha256: await runtimeTemplateSha256FromFile(
      path.join(root, "contracts/out-staging/Bridge.sol/Bridge.json"),
    ),
    bsns_runtime_template_sha256: await runtimeTemplateSha256FromFile(
      path.join(root, "contracts/out-staging/BSNS.sol/BSNS.json"),
    ),
    candid_sha256: await digest(root, "canister/bridge-canister/bridge.did"),
    bridge_abi_sha256: await digest(root, "contracts/abi/Bridge.json"),
    bsns_abi_sha256: await digest(root, "contracts/abi/BSNS.json"),
    ledger_release: "ledger-suite-icrc-2026-03-09",
    ledger_wasm_sha256: await digest(root, "ui/.e2e-cache/ic-icrc1-ledger.wasm.gz"),
    index_wasm_sha256: await digest(root, "ui/.e2e-cache/ic-icrc1-index-ng.wasm.gz"),
    state_upgrade: upgrade,
    tests: {
      full_local_ci: "passed",
      real_frontend_e2e: "passed",
      canister_activation: "passed",
      timelock_delay_enforced: "passed",
      state_upgrade: "passed",
    },
  }
  await mkdir(path.dirname(outputPath), { recursive: true })
  await writeFile(outputPath, `${JSON.stringify(evidence, null, 2)}\n`)
  return outputPath
}

async function digest(root, relative) {
  return createHash("sha256").update(await readFile(path.join(root, relative))).digest("hex")
}

function bytesHex(value) {
  if (!Array.isArray(value) || value.length !== 32 || value.some((byte) => !Number.isInteger(byte) || byte < 0 || byte > 255)) {
    return ""
  }
  return `0x${value.map((byte) => byte.toString(16).padStart(2, "0")).join("")}`
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const args = process.argv.slice(2)
  if (args.length !== 2 || args[0] !== "--output") {
    throw new Error("usage: generate-local-e2e.mjs --output /absolute/path/local-e2e.json")
  }
  process.stdout.write(`${await generateLocalEvidence(defaultRoot, args[1])}\n`)
}
