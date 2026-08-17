import { describe, expect, it } from "vitest"
import { deploymentProfile, deploymentProfileSchema, profileCompleteness } from "./profile"

describe("reviewed deployment profile", () => {
  it("reports every missing preflight deployment value", () => {
    const blockers = profileCompleteness(deploymentProfile)
    expect(blockers).toContain("Bridge contract address is missing")
    expect(blockers).toContain("IC token index ID is missing")
    expect(blockers).toContain("Expected Bridge signer is missing")
    expect(blockers).toContain("Timelock contract address is missing")
    expect(blockers).toContain("Timelock delay is missing")
    expect(blockers).toContain("Deployment instance ID is missing")
    expect(blockers).toHaveLength(17)
    expect(deploymentProfile.snsRootCanisterId).toBeNull()
    expect(deploymentProfile.icToken).toEqual({ name: "TEST ICRC1", symbol: "TICRC1", decimals: 8 })
    expect(deploymentProfile.baseToken).toEqual({ symbol: "KINIC", decimals: 8 })
  })

  it("accepts deterministic JSON release fields and coerces deployment block", () => {
    const parsed = deploymentProfileSchema.parse({
      ...deploymentProfile,
      deploymentBlock: "123",
    })
    expect(parsed.deploymentBlock).toBe(123n)
  })

  it("requires reviewed history RPCs for Sepolia staging", () => {
    expect(() => deploymentProfileSchema.parse({
      ...deploymentProfile,
      environment: "sepolia-staging",
      environmentMode: "short-delay-test-only",
      activationTimelockDelaySeconds: 300,
      bridgeCanisterId: "aaaaa-aa",
      deploymentInstanceId: `0x${"99".repeat(32)}`,
      minimumWithdrawalId: `0x${"00".repeat(31)}01`,
      ledgerCanisterId: "aaaaa-aa",
      indexCanisterId: "aaaaa-aa",
      evmRpcCanisterId: "7hfb6-caaaa-aaaar-qadga-cai",
      baseHistoryRpcUrls: undefined,
    })).toThrow("reviewed Base history RPC URLs")
  })

  it("rejects a zero deployment instance ID", () => {
    expect(() => deploymentProfileSchema.parse({
      ...deploymentProfile,
      deploymentInstanceId: `0x${"00".repeat(32)}`,
      minimumWithdrawalId: `0x${"00".repeat(31)}01`,
    })).toThrow("hash must be nonzero")
  })

  it("fails closed for a production profile without Gate B deployment binding", () => {
    const blockers = profileCompleteness({
      ...deploymentProfile,
      testOnly: false,
      deploymentBlock: 0n,
      gateBManifestSha256: null,
    })
    expect(blockers).toContain("Production deployment block is not Gate B bound")
    expect(blockers).toContain("Verified Gate B manifest SHA-256 is missing")
    expect(blockers).toContain("KINIC SNS Root ID is missing")
  })
})
