import assert from "node:assert/strict"
import { deploymentInstanceHex, verifyReplacementInstance } from "./check-replacement-instance.mjs"

const previousHex = "0x2c344ca868267bc19791a2e1a966c210eef545945b3879223d3cb6c1255d789b"
const previousBytes = Array.from(Buffer.from(previousHex.slice(2), "hex"))
const changedHex = `0x${"22".repeat(32)}`
const currentStatus = { module_hash: "0xedecad666c6777f7d0d6d5f47851d71c4df77633826b7b3906578a6950632697" }

assert.equal(deploymentInstanceHex(previousBytes, "test"), previousHex)
assert.deepEqual(
  verifyReplacementInstance(
    { deploymentInstanceId: changedHex },
    { schema_version: 34, deployment_instance_id: previousBytes },
    currentStatus,
  ),
  {
    replacement_mode: "destructive-reinstall",
    live_schema_version: 34,
    previous_deployment_instance_id: previousHex,
    live_module_hash: currentStatus.module_hash,
    next: changedHex,
  },
)
assert.throws(
  () => verifyReplacementInstance(
    { deploymentInstanceId: previousHex },
    { schema_version: 34, deployment_instance_id: previousBytes },
    currentStatus,
  ),
  /fresh deployment instance/,
)
for (const schemaVersion of [38, 37, 36, 35, 33, 32, 31, 30]) {
  assert.throws(
    () => verifyReplacementInstance(
      { deploymentInstanceId: changedHex },
      { schema_version: schemaVersion, deployment_instance_id: previousBytes },
      currentStatus,
    ),
    /requires the reviewed legacy schema v34/,
  )
}

assert.throws(
  () => verifyReplacementInstance(
    { deploymentInstanceId: changedHex },
    { schema_version: 34, deployment_instance_id: Array(32).fill(17) },
    currentStatus,
  ),
  /differs from reviewed abandonment evidence/,
)
assert.throws(() => verifyReplacementInstance(
  { deploymentInstanceId: changedHex },
  { schema_version: 34, deployment_instance_id: previousBytes },
  {},
), /module hash/)

process.stdout.write("staging replacement deployment instance tests passed\n")
