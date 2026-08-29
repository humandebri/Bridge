import assert from "node:assert/strict"
import { deploymentInstanceHex, verifyUpgradeInstance } from "./check-upgrade-instance.mjs"

const previousBytes = Array(32).fill(17)
const previousHex = `0x${"11".repeat(32)}`
const changedHex = `0x${"22".repeat(32)}`
const currentStatus = { module_hash: `0x${"35".repeat(32)}` }

assert.equal(deploymentInstanceHex(previousBytes, "test"), previousHex)
assert.deepEqual(
  verifyUpgradeInstance(
    { deploymentInstanceId: previousHex },
    { schema_version: 35, deployment_instance_id: previousBytes },
    currentStatus,
  ),
  {
    replacement_mode: "current-schema-upgrade",
    live_schema_version: 35,
    previous_deployment_instance_id: previousHex,
    live_module_hash: currentStatus.module_hash,
    next: previousHex,
  },
)
assert.throws(
  () => verifyUpgradeInstance(
    { deploymentInstanceId: changedHex },
    { schema_version: 35, deployment_instance_id: previousBytes },
    currentStatus,
  ),
  /reinstall is prohibited/,
)
for (const schemaVersion of [38, 37, 36, 34, 33, 32, 31, 30]) {
  assert.throws(
    () => verifyUpgradeInstance(
      { deploymentInstanceId: previousHex },
      { schema_version: schemaVersion, deployment_instance_id: previousBytes },
      currentStatus,
    ),
    /requires current schema v35/,
  )
}

assert.throws(() => verifyUpgradeInstance(
  { deploymentInstanceId: previousHex },
  { schema_version: 35, deployment_instance_id: previousBytes },
  {},
), /module hash/)

process.stdout.write("staging upgrade deployment instance tests passed\n")
