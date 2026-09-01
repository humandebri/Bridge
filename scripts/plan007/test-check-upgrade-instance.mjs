import assert from "node:assert/strict"
import { deploymentInstanceHex, verifyUpgradeInstance } from "./check-upgrade-instance.mjs"

const previousBytes = Array(32).fill(17)
const previousHex = `0x${"11".repeat(32)}`
const changedHex = `0x${"22".repeat(32)}`
const bridgeCanisterId = "rlhjx-iyaaa-aaaaf-qcnyq-cai"
const currentStatus = {
  canister_id: bridgeCanisterId,
  module_hash: `0x${"34".repeat(32)}`,
  controller_principals: ["aaaaa-aa"],
  cycles_balance: 100,
}
const rpcDigestBytes = Array(32).fill(51)
const rpcDigestHex = `0x${"33".repeat(32)}`
const currentProfile = {
  bridgeCanisterId,
  deploymentInstanceId: previousHex,
  rpcProviderUrlsSha256: rpcDigestHex,
}

assert.equal(deploymentInstanceHex(previousBytes, "test"), previousHex)
assert.deepEqual(
  verifyUpgradeInstance(
    currentProfile,
    { schema_version: 35, deployment_instance_id: previousBytes, rpc_provider_urls_sha256: rpcDigestBytes },
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
    { ...currentProfile, deploymentInstanceId: changedHex },
    { schema_version: 35, deployment_instance_id: previousBytes, rpc_provider_urls_sha256: rpcDigestBytes },
    currentStatus,
  ),
  /reinstall is prohibited/,
)
for (const schemaVersion of [38, 37, 36, 34, 33, 32, 31, 30]) {
  assert.throws(
    () => verifyUpgradeInstance(
      currentProfile,
      { schema_version: schemaVersion, deployment_instance_id: previousBytes, rpc_provider_urls_sha256: rpcDigestBytes },
      currentStatus,
    ),
    /requires current schema v35; old and unknown schemas are unsupported/,
  )
}

assert.throws(
  () => verifyUpgradeInstance(
    currentProfile,
    { schema_version: 33, deployment_instance_id: previousBytes, rpc_provider_urls_sha256: rpcDigestBytes },
    currentStatus,
  ),
  /old and unknown schemas are unsupported/,
)
assert.throws(() => verifyUpgradeInstance(
  currentProfile,
  { schema_version: 35, deployment_instance_id: previousBytes, rpc_provider_urls_sha256: rpcDigestBytes },
  { ...currentStatus, module_hash: "0x0" },
), /module hash/)
assert.throws(() => verifyUpgradeInstance(
  { ...currentProfile, rpcProviderUrlsSha256: `0x${"44".repeat(32)}` },
  { schema_version: 35, deployment_instance_id: previousBytes, rpc_provider_urls_sha256: rpcDigestBytes },
  currentStatus,
), /RPC provider digest differs/)
assert.throws(() => verifyUpgradeInstance(
  currentProfile,
  { schema_version: 35, deployment_instance_id: previousBytes, rpc_provider_urls_sha256: rpcDigestBytes },
  { ...currentStatus, canister_id: "aaaaa-aa" },
), /reviewed Bridge canister/)
assert.throws(() => verifyUpgradeInstance(
  currentProfile,
  { schema_version: 35, deployment_instance_id: previousBytes, rpc_provider_urls_sha256: rpcDigestBytes },
  { module_hash: currentStatus.module_hash, controller_principals: ["aaaaa-aa"], cycles_balance: 100 },
), /fields differ/)
assert.throws(() => verifyUpgradeInstance(
  currentProfile,
  { schema_version: 35, deployment_instance_id: previousBytes, rpc_provider_urls_sha256: rpcDigestBytes },
  { ...currentStatus, extra: true },
), /fields differ/)

process.stdout.write("staging upgrade deployment instance tests passed\n")
