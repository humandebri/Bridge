import assert from "node:assert/strict"
import { generateKeyPairSync } from "node:crypto"
import test from "node:test"
import { Ed25519KeyIdentity } from "@icp-sdk/core/identity"
import { Secp256k1KeyIdentity } from "@icp-sdk/core/identity/secp256k1"
import { identityFromPem } from "./cli.ts"

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
