import assert from "node:assert/strict"
import { deploymentInstanceHex, obsoleteReplacementPolicy, verifyReinstallInstance } from "./check-reinstall-instance.mjs"

const previousBytes = Array(32).fill(17)
const previousHex = `0x${"11".repeat(32)}`
const nextHex = `0x${"22".repeat(32)}`
const currentStatus = { module_hash: `0x${"33".repeat(32)}` }
const obsoleteProfile = {
  bridgeCanisterId: obsoleteReplacementPolicy.bridge_canister_id,
  deploymentInstanceId: obsoleteReplacementPolicy.previous_deployment_instance_id,
}
const obsoleteLive = {
  schema_version: obsoleteReplacementPolicy.live_schema_version,
  deployment_instance_id: obsoleteReplacementPolicy.previous_deployment_instance_id,
}
const obsoleteStatus = { module_hash: obsoleteReplacementPolicy.module_hash }

assert.equal(deploymentInstanceHex(previousBytes, "test"), previousHex)
assert.deepEqual(
  verifyReinstallInstance(
    { deploymentInstanceId: nextHex },
    { schema_version: 31, deployment_instance_id: previousBytes },
    currentStatus,
  ),
  {
    replacement_mode: "current-schema-reinstall",
    live_schema_version: 31,
    previous_deployment_instance_id: previousHex,
    live_module_hash: currentStatus.module_hash,
    next: nextHex,
  },
)
assert.deepEqual(
  verifyReinstallInstance(
    { deploymentInstanceId: previousHex },
    { schema_version: 31, deployment_instance_id: previousBytes },
    currentStatus,
  ),
  {
    replacement_mode: "current-schema-upgrade",
    live_schema_version: 31,
    previous_deployment_instance_id: previousHex,
    live_module_hash: currentStatus.module_hash,
    next: previousHex,
  },
)
assert.deepEqual(
  verifyReinstallInstance(
    obsoleteProfile,
    obsoleteLive,
    obsoleteStatus,
  ),
  {
    replacement_mode: "obsolete-schema-upgrade",
    live_schema_version: 30,
    previous_deployment_instance_id: obsoleteReplacementPolicy.previous_deployment_instance_id,
    live_module_hash: obsoleteReplacementPolicy.module_hash,
    next: obsoleteReplacementPolicy.previous_deployment_instance_id,
  },
)
assert.throws(
  () => verifyReinstallInstance(
    { deploymentInstanceId: nextHex },
    { schema_version: 31 },
    currentStatus,
  ),
  /must be a nonzero/,
)
assert.throws(
  () => verifyReinstallInstance(
    { deploymentInstanceId: nextHex },
    { schema_version: 29, deployment_instance_id: previousBytes },
    currentStatus,
  ),
  /audited obsolete schema/,
)
for (const [profile, live, status] of [
  [{ ...obsoleteProfile, bridgeCanisterId: "aaaaa-aa" }, obsoleteLive, obsoleteStatus],
  [obsoleteProfile, { ...obsoleteLive, deployment_instance_id: previousHex }, obsoleteStatus],
  [obsoleteProfile, obsoleteLive, { module_hash: currentStatus.module_hash }],
]) {
  assert.throws(
    () => verifyReinstallInstance(profile, live, status),
    /reviewed replacement policy|must preserve/,
  )
}
assert.throws(
  () => verifyReinstallInstance({ deploymentInstanceId: nextHex }, { schema_version: 31, deployment_instance_id: previousBytes }, {}),
  /module hash/,
)
assert.throws(
  () => verifyReinstallInstance(
    { deploymentInstanceId: nextHex },
    { schema_version: 31, deployment_instance_id: previousBytes },
    { module_hash: `0x${"00".repeat(32)}` },
  ),
  /nonzero/,
)
for (const value of [undefined, `0x${"00".repeat(32)}`, [], Array(32).fill(0)]) {
  assert.throws(() => deploymentInstanceHex(value, "test"))
}

process.stdout.write("staging install deployment instance tests passed\n")
