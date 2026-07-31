import assert from "node:assert/strict"
import { deploymentInstanceHex, verifyReinstallInstance } from "./check-reinstall-instance.mjs"

const previousBytes = Array(32).fill(17)
const previousHex = `0x${"11".repeat(32)}`
const nextHex = `0x${"22".repeat(32)}`

assert.equal(deploymentInstanceHex(previousBytes, "test"), previousHex)
assert.deepEqual(
  verifyReinstallInstance(
    { deploymentInstanceId: nextHex },
    { schema_version: 30, deployment_instance_id: previousBytes },
  ),
  { live_schema_version: 30, previous_deployment_instance_id: previousHex, next: nextHex },
)
assert.deepEqual(
  verifyReinstallInstance(
    { deploymentInstanceId: nextHex },
    { schema_version: 29 },
  ),
  { live_schema_version: 29, previous_deployment_instance_id: null, next: nextHex },
)
assert.throws(
  () => verifyReinstallInstance(
    { deploymentInstanceId: previousHex },
    { schema_version: 30, deployment_instance_id: previousBytes },
  ),
  /rejected reuse/,
)
assert.throws(
  () => verifyReinstallInstance(
    { deploymentInstanceId: nextHex },
    { schema_version: 30 },
  ),
  /must be a nonzero/,
)
assert.throws(
  () => verifyReinstallInstance(
    { deploymentInstanceId: nextHex },
    { schema_version: 29, deployment_instance_id: previousHex },
  ),
  /must not contain/,
)
assert.throws(
  () => verifyReinstallInstance(
    { deploymentInstanceId: nextHex },
    { schema_version: 28 },
  ),
  /only accepts/,
)
for (const value of [undefined, `0x${"00".repeat(32)}`, [], Array(32).fill(0)]) {
  assert.throws(() => deploymentInstanceHex(value, "test"))
}

process.stdout.write("staging reinstall deployment instance tests passed\n")
