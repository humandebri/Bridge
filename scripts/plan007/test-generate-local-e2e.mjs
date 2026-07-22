import assert from "node:assert/strict"
import { readFile } from "node:fs/promises"
import path from "node:path"
import { fileURLToPath } from "node:url"
import { LOCAL_E2E_SCHEMA_VERSION, validateUpgradeEvidence } from "./generate-local-e2e.mjs"

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..")
const schema = JSON.parse(await readFile(path.join(root, "deployments/sepolia-staging/local-e2e.schema.json"), "utf8"))
assert.equal(schema.properties.schema_version.const, LOCAL_E2E_SCHEMA_VERSION)
assert(schema.required.includes("state_upgrade"))
assert.equal(schema.properties.state_upgrade.$ref, "#/$defs/stateUpgrade")
assert.deepEqual(schema.$defs.stateUpgrade.required, ["verified", "before", "after"])

const state = {
  owner_sequence: "2",
  status: {
    schema_version: "17",
    counts: { pending_evm_operations: "1", pending_ledger_operations: "1" },
    settlement_scheduler: { scheduled: "1", leased: "0" },
  },
  public_config: {
    deposit_rate_limit_window_seconds: "60",
    deposit_rate_limit_global: 30,
    deposit_rate_limit_per_principal: 3,
    settlement_rate_limit_window_seconds: "600",
    settlement_rate_limit_global: 60,
    settlement_rate_limit_per_principal: 6,
    settlement_rate_limit_per_record: 3,
  },
  deposits: [{ deposit_id: [1], owner_sequence: "1", base_confirmation: [{ Submitted: { transaction_hash: [2] } }] }],
  withdrawals: [],
  audit_events: { events: [{ sequence: "1" }] },
}
const complete = { verified: true, before: structuredClone(state), after: structuredClone(state) }

assert.equal(validateUpgradeEvidence(structuredClone(complete)).verified, true)

for (const mutate of [
  (value) => { value.verified = false },
  (value) => { value.after.owner_sequence = "3" },
  (value) => { value.before.deposits = []; value.after.deposits = [] },
  (value) => { value.before.deposits[0].base_confirmation = []; value.after.deposits[0].base_confirmation = [] },
  (value) => { delete value.before.status.settlement_scheduler; delete value.after.status.settlement_scheduler },
  (value) => { delete value.before.public_config.settlement_rate_limit_per_record; delete value.after.public_config.settlement_rate_limit_per_record },
  (value) => { value.before.status.counts.pending_evm_operations = 1; value.after.status.counts.pending_evm_operations = 1 },
]) {
  const invalid = structuredClone(complete)
  mutate(invalid)
  assert.throws(() => validateUpgradeEvidence(invalid))
}

process.stdout.write("plan007 local E2E evidence validation tests passed\n")
