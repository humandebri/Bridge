import assert from "node:assert/strict"
import { readFile } from "node:fs/promises"
import path from "node:path"
import { fileURLToPath } from "node:url"
import { LOCAL_E2E_SCHEMA_VERSION, STAGING_ACTIVATION_DELAY_SECONDS, validateUpgradeEvidence } from "./generate-local-e2e.mjs"

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..")
const schema = JSON.parse(await readFile(path.join(root, "deployments/sepolia-staging/local-e2e.schema.json"), "utf8"))
assert.equal(schema.properties.schema_version.const, LOCAL_E2E_SCHEMA_VERSION)
assert.equal(schema.properties.activation_timelock_delay_seconds.const, STAGING_ACTIVATION_DELAY_SECONDS)
assert(schema.required.includes("state_upgrade"))
assert.equal(schema.properties.state_upgrade.$ref, "#/$defs/stateUpgrade")
assert.deepEqual(schema.$defs.stateUpgrade.required, ["verified", "before", "after"])

const state = {
  owner_sequence: "2",
  status: {
    schema_version: "28",
    counts: { pending_ledger_operations: "1", reserved_deposit_mint_operations: "1" },
    settlement_scheduler: { scheduled: "1", leased: "0" },
  },
  public_config: {
    deposit_rate_limit_window_seconds: "60",
    deposit_rate_limit_global: 30,
    deposit_rate_limit_per_principal: 3,
    notification_rate_limit_window_seconds: "600",
    notification_rate_limit_global: 60,
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

for (const mutate of [
  (value) => { value.verified = false },
  (value) => { value.after.owner_sequence = "3" },
  (value) => { value.before.deposits = []; value.after.deposits = [] },
  (value) => { value.before.deposits[0].mint_authorization = []; value.after.deposits[0].mint_authorization = [] },
  (value) => { delete value.before.status.settlement_scheduler; delete value.after.status.settlement_scheduler },
  (value) => { delete value.before.public_config.notification_rate_limit_global; delete value.after.public_config.notification_rate_limit_global },
  (value) => { delete value.before.public_config.settlement_rate_limit_per_record; delete value.after.public_config.settlement_rate_limit_per_record },
  (value) => { delete value.before.public_config.settlement_retry_interval_seconds; delete value.after.public_config.settlement_retry_interval_seconds },
  (value) => { value.before.status.counts.reserved_deposit_mint_operations = 1; value.after.status.counts.reserved_deposit_mint_operations = 1 },
  (value) => { delete value.before.activation_status; delete value.after.activation_status },
  (value) => { value.before.storage_integrity = "failed"; value.after.storage_integrity = "failed" },
]) {
  const invalid = structuredClone(complete)
  mutate(invalid)
  assert.throws(() => validateUpgradeEvidence(invalid))
}

process.stdout.write("plan007 local E2E evidence validation tests passed\n")
