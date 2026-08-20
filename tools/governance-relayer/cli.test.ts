import assert from "node:assert/strict"
import { generateKeyPairSync } from "node:crypto"
import test from "node:test"
import { Ed25519KeyIdentity } from "@icp-sdk/core/identity"
import { Secp256k1KeyIdentity } from "@icp-sdk/core/identity/secp256k1"
import {
  canisterErrorMessage,
  commandRequiresIdentity,
  identityFromPem,
  unwrap,
  waitForFinalized,
} from "./cli.ts"

test("uses an anonymous IC actor only for relay lifecycle commands", () => {
  for (const command of ["status", "relay", "confirm", "run"]) {
    assert.equal(commandRequiresIdentity(command), false)
  }
  for (const command of ["prepare", "replace", "schedule-activation", "execute-activation", "drain-emergency"]) {
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

test("stops finality waiting immediately when the transaction reverted", async () => {
  let blockReads = 0
  const hash = `0x${"12".repeat(32)}` as `0x${string}`
  await assert.rejects(waitForFinalized({
    async getTransactionReceipt() {
      return { blockNumber: 42n, blockHash: `0x${"34".repeat(32)}` as `0x${string}`, status: "reverted" as const }
    },
    async getBlock() {
      blockReads += 1
      return { number: 42n, hash: `0x${"34".repeat(32)}` as `0x${string}` }
    },
  }, hash), new RegExp(`Transaction reverted: ${hash}`))
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
