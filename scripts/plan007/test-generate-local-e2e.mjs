import assert from "node:assert/strict"
import { readFile } from "node:fs/promises"
import path from "node:path"
import { fileURLToPath } from "node:url"
import {
  CURRENT_STABLE_SCHEMA_VERSION,
  CURRENT_RECORD_WIRE_VERSION,
  LOCAL_E2E_SCHEMA_VERSION,
  STAGING_ACTIVATION_DELAY_SECONDS,
  validateDeploymentInstanceBinding,
  validateUpgradeEvidence,
} from "./generate-local-e2e.mjs"
import { runtimeTemplateSha256 } from "./runtime-template-hash.mjs"
import { createExclusiveProgressPause } from "../../ui/e2e-real/progress-pause.mjs"

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..")
const schema = JSON.parse(await readFile(path.join(root, "deployments/sepolia-staging/local-e2e.schema.json"), "utf8"))
assert.equal(schema.properties.schema_version.const, LOCAL_E2E_SCHEMA_VERSION)
assert.equal(schema.properties.activation_timelock_delay_seconds.const, STAGING_ACTIVATION_DELAY_SECONDS)
assert.equal(schema.properties.stable_schema_version.const, CURRENT_STABLE_SCHEMA_VERSION)
assert.equal(schema.properties.record_wire_version.const, CURRENT_RECORD_WIRE_VERSION)
assert(schema.required.includes("stable_schema_version"))
assert(schema.required.includes("record_wire_version"))
assert(schema.required.includes("state_upgrade"))
assert.equal(schema.properties.state_upgrade.$ref, "#/$defs/stateUpgrade")
assert.deepEqual(schema.$defs.stateUpgrade.required, ["verified", "before", "after"])
assert(schema.required.includes("bridge_runtime_template_sha256"))
assert(schema.required.includes("bsns_runtime_template_sha256"))
assert(schema.required.includes("deployment_instance_id"))

const artifact = {
  deployedBytecode: {
    object: "0x600000006001",
    immutableReferences: { "1": [{ start: 1, length: 3 }] },
  },
}
const expectedTemplateHash = runtimeTemplateSha256(artifact)
assert.equal(runtimeTemplateSha256(artifact, "0x60abcdef6001"), expectedTemplateHash)
assert.throws(() => runtimeTemplateSha256(artifact, "0x61abcdef6001"), /outside immutable references/)
assert.throws(() => runtimeTemplateSha256(artifact, "0x60abcdef60"), /length/)
assert.throws(
  () => runtimeTemplateSha256({ deployedBytecode: { object: "0x6000", immutableReferences: {} } }, "0x6100"),
  /outside immutable references/,
)

const state = {
  owner_sequence: "2",
  status: {
    schema_version: String(CURRENT_STABLE_SCHEMA_VERSION),
    counts: { pending_ledger_operations: "1", reserved_deposit_mint_operations: "1" },
    settlement_scheduler: { scheduled: "1", leased: "0" },
  },
  runtime_binding: {
    deployment_instance_id: Array(32).fill(9),
    minimum_withdrawal_id: Array(32).fill(8),
    base_chain_id: "84532",
    bridge_contract: Array(20).fill(1),
    expected_bridge_runtime_sha256: Array(32).fill(2),
    timelock_contract: Array(20).fill(3),
    expected_bridge_signer: Array(20).fill(4),
    ledger_canister_id: "aaaaa-aa",
    index_canister_id: "aaaaa-aa",
    evm_rpc_canister_id: "aaaaa-aa",
    rpc_provider_urls_sha256: Array(32).fill(5),
    schema_version: String(CURRENT_STABLE_SCHEMA_VERSION),
    operational_config_sha256: Array(32).fill(6),
  },
  operational_config: {
    deposit_rate_limit_window_seconds: "60",
    deposit_rate_limit_global: 30,
    deposit_rate_limit_per_principal: 3,
    notification_rate_limit_window_seconds: "600",
    notification_rate_limit_global: 60,
    notification_ingestion_rate_limit_global: 30,
    settlement_rate_limit_window_seconds: "600",
    settlement_rate_limit_global: 60,
    settlement_rate_limit_per_principal: 6,
    settlement_rate_limit_per_record: 3,
    settlement_retry_interval_seconds: "60",
  },
  deposits: [{ deposit_id: [1], owner_sequence: "1", mint_authorization: [{ digest: [2], deadline: "1801" }] }],
  withdrawals: [],
  audit_events: { events: [{ sequence: "1" }] },
  activation_status: { pending_timelock_operation: [], deposits_paused: false },
  storage_integrity: "ok",
}
const complete = { verified: true, before: structuredClone(state), after: structuredClone(state) }

assert.equal(validateUpgradeEvidence(structuredClone(complete)).verified, true)
const deploymentInstanceId = `0x${"09".repeat(32)}`
assert.equal(validateDeploymentInstanceBinding(complete, deploymentInstanceId), deploymentInstanceId)
assert.throws(
  () => validateDeploymentInstanceBinding(complete, `0x${"08".repeat(32)}`),
  /does not match/,
)
assert.throws(() => validateDeploymentInstanceBinding(complete, "0x09"), /invalid/)

const progressEvents = []
let releaseFirstOperation
let firstOperationEntered
const firstOperationReady = new Promise((resolve) => { firstOperationEntered = resolve })
const firstOperationHold = new Promise((resolve) => { releaseFirstOperation = resolve })
const withPausedProgress = createExclusiveProgressPause(
  async () => { progressEvents.push("stop") },
  async () => { progressEvents.push("start") },
)
let activeOperations = 0
let maximumActiveOperations = 0
const firstOperation = withPausedProgress(async () => {
  activeOperations += 1
  maximumActiveOperations = Math.max(maximumActiveOperations, activeOperations)
  progressEvents.push("first")
  firstOperationEntered()
  await firstOperationHold
  activeOperations -= 1
})
await firstOperationReady
const secondOperation = withPausedProgress(async () => {
  activeOperations += 1
  maximumActiveOperations = Math.max(maximumActiveOperations, activeOperations)
  progressEvents.push("second")
  activeOperations -= 1
})
await Promise.resolve()
assert.equal(maximumActiveOperations, 1)
releaseFirstOperation()
await Promise.all([firstOperation, secondOperation])
assert.equal(maximumActiveOperations, 1)
assert.deepEqual(progressEvents, ["stop", "first", "start", "stop", "second", "start"])

await assert.rejects(withPausedProgress(async () => { throw new Error("expected operation failure") }))
assert.equal(await withPausedProgress(async () => "released"), "released")

for (const mutate of [
  (value) => { value.verified = false },
  (value) => { value.before.status.schema_version = "28"; value.after.status.schema_version = "28" },
  (value) => { value.after.owner_sequence = "3" },
  (value) => { value.before.deposits = []; value.after.deposits = [] },
  (value) => { value.before.deposits[0].mint_authorization = []; value.after.deposits[0].mint_authorization = [] },
  (value) => { delete value.before.status.settlement_scheduler; delete value.after.status.settlement_scheduler },
  (value) => { delete value.before.operational_config.notification_rate_limit_global; delete value.after.operational_config.notification_rate_limit_global },
  (value) => { delete value.before.operational_config.notification_ingestion_rate_limit_global; delete value.after.operational_config.notification_ingestion_rate_limit_global },
  (value) => { value.before.runtime_binding.deployment_instance_id = Array(32).fill(0); value.after.runtime_binding.deployment_instance_id = Array(32).fill(0) },
  (value) => { delete value.before.operational_config.settlement_rate_limit_per_record; delete value.after.operational_config.settlement_rate_limit_per_record },
  (value) => { delete value.before.operational_config.settlement_retry_interval_seconds; delete value.after.operational_config.settlement_retry_interval_seconds },
  (value) => { delete value.before.runtime_binding.operational_config_sha256; delete value.after.runtime_binding.operational_config_sha256 },
  (value) => { value.before.status.counts.reserved_deposit_mint_operations = 1; value.after.status.counts.reserved_deposit_mint_operations = 1 },
  (value) => { delete value.before.activation_status; delete value.after.activation_status },
  (value) => { value.before.storage_integrity = "failed"; value.after.storage_integrity = "failed" },
]) {
  const invalid = structuredClone(complete)
  mutate(invalid)
  assert.throws(() => validateUpgradeEvidence(invalid))
}

process.stdout.write("plan007 local E2E evidence validation tests passed\n")
