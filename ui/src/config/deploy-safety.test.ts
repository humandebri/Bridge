import { describe, expect, it } from "vitest"
import { assertProductionUiProfile, assertTestUiProfile, OFFICIAL_EVM_RPC_CANISTER_ID } from "./deploy-safety"

describe("UI deployment safety", () => {
  it("requires a production profile bound to the verified Gate B manifest", () => {
    const manifest = "a".repeat(64)
    const hashes = { profileFileSha256: "b".repeat(64), profileCanonicalSha256: "c".repeat(64) }
    expect(() => assertProductionUiProfile({ testOnly: false, gateBManifestSha256: manifest, ...hashes }, manifest)).not.toThrow()
    expect(() => assertProductionUiProfile({ testOnly: true, gateBManifestSha256: manifest }, manifest)).toThrow("Production UI deploy rejects test-only")
    expect(() => assertProductionUiProfile({})).toThrow("Production UI deploy rejects test-only")
    expect(() => assertProductionUiProfile({ testOnly: false, gateBManifestSha256: manifest })).toThrow("requires a verified Gate B")
    expect(() => assertProductionUiProfile({ testOnly: false, gateBManifestSha256: manifest, ...hashes }, "b".repeat(64))).toThrow("does not match")
    expect(() => assertProductionUiProfile({ testOnly: false, gateBManifestSha256: manifest }, manifest)).toThrow("source profile hashes")
  })
})

describe("test UI deployment safety", () => {
  const staging = {
    environment: "sepolia-staging",
    testOnly: true,
    environmentMode: "short-delay-test-only",
    activationTimelockDelaySeconds: 300,
    chainId: 84532,
    bridgeCanisterId: "aaaaa-aa",
    ledgerCanisterId: "2vxsx-fae",
    indexCanisterId: "ryjl3-tyaaa-aaaaa-aaaba-cai",
    evmRpcCanisterId: OFFICIAL_EVM_RPC_CANISTER_ID,
  }

  it("accepts an isolated Base Sepolia profile", () => {
    expect(() => assertTestUiProfile(staging)).not.toThrow()
    expect(() => assertTestUiProfile({ ...staging, bridgeCanisterId: "rlhjx-iyaaa-aaaaf-qcnyq-cai" })).not.toThrow()
  })

  it("rejects mainnet chain, production IDs, and a non-official EVM RPC canister", () => {
    expect(() => assertTestUiProfile({ ...staging, chainId: 8453 })).toThrow("Base Mainnet")
    expect(() => assertTestUiProfile({ ...staging, bridgeCanisterId: "73mez-iiaaa-aaaaq-aaasq-cai" })).toThrow("production canister")
    expect(() => assertTestUiProfile({ ...staging, evmRpcCanisterId: "aaaaa-aa" })).toThrow("official EVM RPC")
    expect(() => assertTestUiProfile({ ...staging, testOnly: false })).toThrow("testOnly")
  })
})
