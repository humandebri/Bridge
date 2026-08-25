import assert from "node:assert/strict"
import { deploymentInstanceHex, verifyUpgradeInstance } from "./check-upgrade-instance.mjs"

const previousBytes = Array(32).fill(17)
const previousHex = `0x${"11".repeat(32)}`
const changedHex = `0x${"22".repeat(32)}`
const currentStatus = { module_hash: `0x${"34".repeat(32)}` }

assert.equal(deploymentInstanceHex(previousBytes, "test"), previousHex)
assert.deepEqual(
  verifyUpgradeInstance(
    { deploymentInstanceId: previousHex },
    { schema_version: 34, deployment_instance_id: previousBytes },
    currentStatus,
  ),
  {
    replacement_mode: "current-schema-upgrade",
    live_schema_version: 34,
    previous_deployment_instance_id: previousHex,
    live_module_hash: currentStatus.module_hash,
    next: previousHex,
  },
)
assert.throws(
  () => verifyUpgradeInstance(
    { deploymentInstanceId: changedHex },
    { schema_version: 34, deployment_instance_id: previousBytes },
    currentStatus,
  ),
  /reinstall is prohibited/,
)
for (const schemaVersion of [38, 37, 36, 35, 32, 31, 30]) {
  assert.throws(
    () => verifyUpgradeInstance(
      { deploymentInstanceId: previousHex },
      { schema_version: schemaVersion, deployment_instance_id: previousBytes },
      currentStatus,
    ),
    /requires reviewed source schema v33 or current schema v34/,
  )
}

assert.equal(
  verifyUpgradeInstance(
    { deploymentInstanceId: previousHex },
    { schema_version: 33, deployment_instance_id: previousBytes },
    currentStatus,
  ).replacement_mode,
  "staging-v33-to-v34-upgrade",
)
assert.throws(() => verifyUpgradeInstance(
  { deploymentInstanceId: previousHex },
  { schema_version: 34, deployment_instance_id: previousBytes },
  {},
), /module hash/)

process.stdout.write("staging upgrade deployment instance tests passed\n")
