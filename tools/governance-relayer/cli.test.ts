import assert from "node:assert/strict"
import { generateKeyPairSync } from "node:crypto"
import test from "node:test"
import { Ed25519KeyIdentity } from "@icp-sdk/core/identity"
import { Secp256k1KeyIdentity } from "@icp-sdk/core/identity/secp256k1"
import { keccak256 } from "viem"
import { privateKeyToAccount } from "viem/accounts"
import {
  afterValidatingStoredArtifacts,
  activationConfirmationHash,
  activationBindingMatches,
  activationReplacementMatches,
  canisterErrorMessage,
  commandRequiresIdentity,
  confirmationHash,
  identityFromPem,
  isActivationArtifact,
  isNonceTooLow,
  parseExpectedGovernanceOperationId,
  parseOptions,
  selectPendingArtifact,
  selectPendingActivationArtifact,
  storedArtifactMatches,
  storedActivationConfirmationIdentity,
  unwrap,
  validateCommandOptions,
  validateStoredArtifact,
  waitForFinalized,
} from "./cli.ts"

test("parses only an allocatable exact governance operation ID", () => {
  assert.equal(parseExpectedGovernanceOperationId(0), 0n)
  assert.equal(parseExpectedGovernanceOperationId("9007199254740992"), 9_007_199_254_740_992n)
  for (const invalid of [
    9_007_199_254_740_992,
    "18446744073709551615",
    -1,
    "01",
    1.5,
    null,
  ]) {
    assert.throws(() => parseExpectedGovernanceOperationId(invalid))
  }
})

test("binds stored artifact fields to the independently decoded signed transaction", async () => {
  const account = privateKeyToAccount(`0x${"11".repeat(32)}`)
  const raw = await account.signTransaction({
    chainId: 8453,
    type: "eip1559",
    nonce: 7,
    to: `0x${"22".repeat(20)}`,
    data: "0x1234",
    gas: 100_000n,
    maxFeePerGas: 20n,
    maxPriorityFeePerGas: 2n,
    value: 0n,
  })
  const artifact = {
    raw_transaction: raw,
    transaction_hash: keccak256(raw),
    sender: account.address,
    chain_id: "8453",
    nonce: "7",
    target: `0x${"22".repeat(20)}`,
    calldata: "0x1234",
    gas_limit: "100000",
    max_fee_per_gas: "20",
    max_priority_fee_per_gas: "2",
  }
  await validateStoredArtifact(artifact)
  for (const drift of [
    { ...artifact, sender: `0x${"33".repeat(20)}` },
    { ...artifact, target: `0x${"33".repeat(20)}` },
    { ...artifact, calldata: "0x1235" },
    { ...artifact, chain_id: "1" },
    { ...artifact, nonce: "8" },
    { ...artifact, gas_limit: "100001" },
    { ...artifact, max_fee_per_gas: "21" },
    { ...artifact, max_priority_fee_per_gas: "3" },
    { ...artifact, transaction_hash: `0x${"00".repeat(32)}` },
  ]) {
    await assert.rejects(() => validateStoredArtifact(drift))
  }

  const nonzeroValueRaw = await account.signTransaction({
    chainId: 8453,
    type: "eip1559",
    nonce: 7,
    to: `0x${"22".repeat(20)}`,
    data: "0x1234",
    gas: 100_000n,
    maxFeePerGas: 20n,
    maxPriorityFeePerGas: 2n,
    value: 1n,
  })
  await assert.rejects(() => validateStoredArtifact({
    ...artifact,
    raw_transaction: nonzeroValueRaw,
    transaction_hash: keccak256(nonzeroValueRaw),
  }))
  const accessListRaw = await account.signTransaction({
    chainId: 8453,
    type: "eip1559",
    nonce: 7,
    to: `0x${"22".repeat(20)}`,
    data: "0x1234",
    gas: 100_000n,
    maxFeePerGas: 20n,
    maxPriorityFeePerGas: 2n,
    value: 0n,
    accessList: [{
      address: `0x${"44".repeat(20)}`,
      storageKeys: [`0x${"00".repeat(32)}`],
    }],
  })
  await assert.rejects(() => validateStoredArtifact({
    ...artifact,
    raw_transaction: accessListRaw,
    transaction_hash: keccak256(accessListRaw),
  }))

  let sideEffects = 0
  await assert.rejects(() => afterValidatingStoredArtifacts(
    [{ ...artifact, nonce: "8" }],
    async () => { sideEffects += 1 },
  ))
  assert.equal(sideEffects, 0)
  await afterValidatingStoredArtifacts([artifact], async () => { sideEffects += 1 })
  assert.equal(sideEffects, 1)
})

test("uses an anonymous IC actor only for read-only status, recovery, and raw relay commands", () => {
  for (const command of ["status", "relay", "recover-activation"]) {
    assert.equal(commandRequiresIdentity(command), false)
  }
  for (const command of ["confirm", "run", "prepare", "replace", "seal-operational-config", "prepare-schedule-activation", "prepare-execute-activation", "refresh-attestation", "drain-emergency"]) {
    assert.equal(commandRequiresIdentity(command), true)
  }
})

test("recovers exactly one pending activation transaction for the requested phase", () => {
  const schedule = { kind: { ScheduleActivation: {} } }
  const execute = { kind: { ExecuteActivation: {} } }
  assert.equal(
    selectPendingActivationArtifact([schedule, execute] as never, "schedule"),
    schedule,
  )
  assert.throws(
    () => selectPendingActivationArtifact([schedule, schedule] as never, "schedule"),
    /exactly one pending schedule/,
  )
  assert.throws(
    () => selectPendingActivationArtifact([execute] as never, "schedule"),
    /exactly one pending schedule/,
  )
})

test("loads an Ed25519 PKCS#8 identity exported by icp-cli", () => {
  const { privateKey } = generateKeyPairSync("ed25519")
  const pem = privateKey.export({ format: "pem", type: "pkcs8" }).toString()

  assert(identityFromPem(pem) instanceof Ed25519KeyIdentity)
})

test("loads a secp256k1 PKCS#8 identity", () => {
  const { privateKey } = generateKeyPairSync("ec", { namedCurve: "secp256k1" })
  const pem = privateKey.export({ format: "pem", type: "pkcs8" }).toString()

  assert(identityFromPem(pem) instanceof Secp256k1KeyIdentity)
})

test("rejects unsupported private-key curves", () => {
  const { privateKey } = generateKeyPairSync("ec", { namedCurve: "prime256v1" })
  const pem = privateKey.export({ format: "pem", type: "pkcs8" }).toString()

  assert.throws(() => identityFromPem(pem), /Unsupported identity PEM key type: P-256/)
})

test("returns a revert immediately so the caller can terminalize it in the Canister", async () => {
  let blockReads = 0
  const hash = `0x${"12".repeat(32)}` as `0x${string}`
  const outcome = await waitForFinalized({
    async getTransactionReceipt() {
      return { blockNumber: 42n, blockHash: `0x${"34".repeat(32)}` as `0x${string}`, status: "reverted" as const }
    },
    async getBlock() {
      blockReads += 1
      return { number: 42n, hash: `0x${"34".repeat(32)}` as `0x${string}` }
    },
  }, hash)
  assert.deepEqual(outcome, {
    blockNumber: 42n,
    blockHash: `0x${"34".repeat(32)}`,
    status: "reverted",
  })
  assert.equal(blockReads, 0)
})

test("reports a safe signing class without automatically retrying", () => {
  assert.throws(
    () => unwrap({ Err: { SigningUnavailable: { class: { InsufficientCycles: null } } } }),
    /Threshold signing unavailable \(InsufficientCycles\).*No automatic retry.*Top up/,
  )
  assert.match(
    canisterErrorMessage({ SigningUnavailable: { class: { RecoveryMismatch: null } } }),
    /Do not retry; inspect the canister state and controller-only logs/,
  )
})

test("classifies only explicit consumed-nonce errors for receipt recovery", () => {
  assert.equal(isNonceTooLow(new Error("nonce too low")), true)
  assert.equal(isNonceTooLow(new Error("nonce has already been used")), true)
  assert.equal(isNonceTooLow(new Error("replacement transaction underpriced")), false)
})

test("rejects duplicate, unknown, and conflicting command options", () => {
  assert.throws(
    () => parseOptions(["--operation-id", "1", "--operation-id", "2"]),
    /Duplicate option: --operation-id/,
  )
  assert.throws(
    () => validateCommandOptions("confirm", { typo: "value" }),
    /Unknown option for confirm: --typo/,
  )
  assert.throws(
    () => validateCommandOptions("confirm", {
      "transaction-hash": `0x${"12".repeat(32)}`,
      hash: `0x${"34".repeat(32)}`,
    }),
    /cannot be used together/,
  )
  assert.throws(
    () => validateCommandOptions("schedule-activation", {}),
    /Unknown command: schedule-activation/,
  )
})

test("uses the documented transaction-hash option without discarding it", () => {
  const hash = `0x${"12".repeat(32)}`
  const options = parseOptions(["--transaction-hash", hash])
  validateCommandOptions("confirm", options)
  assert.equal(confirmationHash(options), hash)
})

test("permits only the fixed activation transaction hash", () => {
  const expected = `0x${"12".repeat(32)}` as `0x${string}`
  assert.equal(
    activationConfirmationHash(parseOptions(["--transaction-hash", expected.toUpperCase().replace("0X", "0x")]), expected),
    expected,
  )
  assert.equal(activationConfirmationHash({}, expected), expected)
  assert.throws(
    () => activationConfirmationHash(
      parseOptions(["--hash", `0x${"34".repeat(32)}`]),
      expected,
    ),
    /differs from the fixed activation artifact/,
  )
})

test("selects the requested governance nonce lane instead of the first pending transaction", () => {
  const pending = [{ operation_id: 7n }, { operation_id: 9n }]
  assert.equal(selectPendingArtifact(pending, "9"), pending[1])
  assert.equal(selectPendingArtifact(pending, "8"), undefined)
  assert.throws(
    () => selectPendingArtifact(pending, undefined),
    /--operation-id is required when multiple governance transactions are pending/,
  )
})

test("preserves implicit selection for one pending governance transaction", () => {
  const pending = [{ operation_id: 7n }]
  assert.equal(selectPendingArtifact(pending, undefined), pending[0])
  assert.throws(
    () => selectPendingArtifact(pending, true),
    /--operation-id must be a non-negative integer/,
  )
})

test("binds relay and confirmation to the exact persisted pending artifact", () => {
  const live = {
    operation_id: 1n,
    transaction_hash: new Uint8Array(32).fill(0x12),
    generation: 0,
  }
  const stored = {
    operation_id: "1",
    transaction_hash: `0x${"12".repeat(32)}`,
    generation: 0,
  }
  assert.equal(storedArtifactMatches(stored, live), true)
  assert.equal(storedArtifactMatches({ ...stored, generation: 1 }, live), false)
})

test("binds an activation artifact to the exact Gate B authorization receipt", () => {
  const artifact = "12".repeat(32)
  const gate = "34".repeat(32)
  const binding = {
    schema_version: 1,
    phase: "schedule",
    gate_b_manifest_sha256: gate,
    artifact_sha256: artifact,
    authorization_receipt_sha256: "56".repeat(32),
    bound_at_unix: 1,
  }
  assert.equal(activationBindingMatches(binding, artifact, gate), true)
  for (const drift of [
    { ...binding, artifact_sha256: "78".repeat(32) },
    { ...binding, gate_b_manifest_sha256: "9a".repeat(32) },
    { ...binding, authorization_receipt_sha256: "bad" },
    { ...binding, extra: true },
  ]) {
    assert.equal(activationBindingMatches(drift, artifact, gate), false)
  }
})

test("recognizes activation transactions that must not use generic run or unbound relay", () => {
  assert.equal(isActivationArtifact({ kind: { ScheduleActivation: {} } }), true)
  assert.equal(isActivationArtifact({ kind: { ExecuteActivation: {} } }), true)
  assert.equal(isActivationArtifact({ kind: { PauseDepositMints: null } }), false)
  assert.equal(isActivationArtifact({ kind: null }), false)
})

test("recovers only the exact next activation replacement generation", () => {
  const stored = {
    operation_id: "0",
    kind: { ScheduleActivation: { salt: "0x12" } },
    chain_id: "8453",
    nonce: "7",
    sender: "0x11",
    target: "0x22",
    calldata: "0x33",
    gas_limit: "100",
    generation: 0,
  }
  const live = {
    operation_id: 0n,
    kind: { ScheduleActivation: { salt: new Uint8Array([0x12]) } },
    chain_id: 8453n,
    nonce: 7n,
    sender: new Uint8Array([0x11]),
    target: new Uint8Array([0x22]),
    calldata: new Uint8Array([0x33]),
    gas_limit: 100n,
    generation: 1,
    max_fee_per_gas: 200n,
    max_priority_fee_per_gas: 3n,
  } as never
  assert.equal(activationReplacementMatches(stored, live, 200n, 3n), true)
  assert.equal(activationReplacementMatches(stored, { ...live, generation: 2 }, 200n, 3n), false)
  assert.equal(activationReplacementMatches(stored, live, 201n, 3n), false)
})

test("recovers an idempotent activation confirmation from the fixed artifact", () => {
  const identity = storedActivationConfirmationIdentity({
    operation_id: "1",
    transaction_hash: `0x${"12".repeat(32)}`,
  })
  assert.equal(identity.operationId, 1n)
  assert.equal(identity.transactionHash, `0x${"12".repeat(32)}`)
  assert.throws(
    () => storedActivationConfirmationIdentity({ operation_id: "01", transaction_hash: "0x12" }),
    /invalid operation ID or transaction hash/,
  )
})
