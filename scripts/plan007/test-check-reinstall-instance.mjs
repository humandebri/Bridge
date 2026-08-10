import assert from "node:assert/strict"
import { deploymentInstanceHex, verifyReinstallInstance } from "./check-reinstall-instance.mjs"

const previousBytes = Array(32).fill(17)
const previousHex = `0x${"11".repeat(32)}`
const nextHex = `0x${"22".repeat(32)}`
const currentStatus = { module_hash: `0x${"33".repeat(32)}` }

assert.equal(deploymentInstanceHex(previousBytes, "test"), previousHex)
assert.deepEqual(
  verifyReinstallInstance(
    { deploymentInstanceId: nextHex },
    { schema_version: 32, deployment_instance_id: previousBytes },
    currentStatus,
  ),
  {
    replacement_mode: "current-schema-reinstall",
    live_schema_version: 32,
    previous_deployment_instance_id: previousHex,
    live_module_hash: currentStatus.module_hash,
    next: nextHex,
  },
)
assert.deepEqual(
  verifyReinstallInstance(
    { deploymentInstanceId: previousHex },
    { schema_version: 32, deployment_instance_id: previousBytes },
    currentStatus,
  ),
  {
    replacement_mode: "current-schema-upgrade",
    live_schema_version: 32,
    previous_deployment_instance_id: previousHex,
    live_module_hash: currentStatus.module_hash,
    next: previousHex,
  },
)
for (const schemaVersion of [31, 30, 29, 33]) {
  assert.throws(
    () => verifyReinstallInstance(
      { deploymentInstanceId: nextHex },
      { schema_version: schemaVersion, deployment_instance_id: previousBytes },
      currentStatus,
    ),
    /only accepts current schema v32/,
  )
}
assert.throws(
  () => verifyReinstallInstance(
    { deploymentInstanceId: nextHex },
    { schema_version: 32 },
    currentStatus,
  ),
  /must be a nonzero/,
)
assert.throws(
  () => verifyReinstallInstance(
    { deploymentInstanceId: nextHex },
    { schema_version: 32, deployment_instance_id: previousBytes },
    {},
  ),
  /module hash/,
)
assert.throws(
  () => verifyReinstallInstance(
    { deploymentInstanceId: nextHex },
    { schema_version: 32, deployment_instance_id: previousBytes },
    { module_hash: `0x${"00".repeat(32)}` },
  ),
  /nonzero/,
)
for (const value of [undefined, `0x${"00".repeat(32)}`, [], Array(32).fill(0)]) {
  assert.throws(() => deploymentInstanceHex(value, "test"))
}

process.stdout.write("staging install deployment instance tests passed\n")
