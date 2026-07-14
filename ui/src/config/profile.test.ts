import { describe, expect, it } from "vitest"
import { deploymentProfile, profileCompleteness } from "./profile"

describe("reviewed deployment profile", () => {
  it("keeps the incomplete checked-in preflight fail-closed", () => {
    const blockers = profileCompleteness(deploymentProfile)
    expect(blockers).toContain("Deployment profile is not approved for writes")
    expect(blockers).toContain("Bridge contract address is missing")
    expect(blockers.length).toBeGreaterThan(5)
  })
})
