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
    expect(blockers).toHaveLength(15)
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

  it("fails closed for a production profile without Gate B deployment binding", () => {
    const blockers = profileCompleteness({
      ...deploymentProfile,
      testOnly: false,
      deploymentBlock: 0n,
      gateBManifestSha256: null,
    })
    expect(blockers).toContain("Production deployment block is not Gate B bound")
    expect(blockers).toContain("Verified Gate B manifest SHA-256 is missing")
  })
})
