import assert from "node:assert/strict"
import { generateKeyPairSync } from "node:crypto"
import test from "node:test"
import { Ed25519KeyIdentity } from "@icp-sdk/core/identity"
import { Secp256k1KeyIdentity } from "@icp-sdk/core/identity/secp256k1"
import {
  canisterErrorMessage,
  commandRequiresIdentity,
  confirmationHash,
  identityFromPem,
  isNonceTooLow,
  parseOptions,
  selectPendingArtifact,
  unwrap,
  validateCommandOptions,
  waitForFinalized,
} from "./cli.ts"

test("uses an anonymous IC actor only for status and raw relay commands", () => {
  for (const command of ["status", "relay"]) {
    assert.equal(commandRequiresIdentity(command), false)
  }
  for (const command of ["confirm", "run", "prepare", "replace", "schedule-activation", "execute-activation", "refresh-attestation", "drain-emergency"]) {
    assert.equal(commandRequiresIdentity(command), true)
  }
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
})

test("uses the documented transaction-hash option without discarding it", () => {
  const hash = `0x${"12".repeat(32)}`
  const options = parseOptions(["--transaction-hash", hash])
  validateCommandOptions("confirm", options)
  assert.equal(confirmationHash(options), hash)
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
