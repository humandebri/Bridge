import { describe, expect, it } from "vitest"
import { assertProductionUiProfile } from "./deploy-safety"

describe("UI deployment safety", () => {
  it("requires_a_production_profile_bound_to_the_verified_Gate_B_manifest", () => {
    const manifest = "a".repeat(64)
    const hashes = { profileFileSha256: "b".repeat(64), profileCanonicalSha256: "c".repeat(64) }
    const production = {
      testOnly: false,
      mintRecoveryUrl: "https://recovery.bridge.kinic.xyz/v1/mint-recovery",
      environmentMode: null,
      activationTimelockDelaySeconds: 86_400,
      timelockAddress: `0x${"11".repeat(20)}`,
      gateBManifestSha256: manifest,
      deploymentBlock: 1n,
      ...hashes,
      canisterSchemaVersion: 36,
      canisterModuleSha256: "d".repeat(64),
      postActivationUpgradeSha256: "e".repeat(64),
      uiRpcConfigSha256: "f".repeat(64),
    }
    expect(() => assertProductionUiProfile(production, manifest)).not.toThrow()
    expect(() =>
      assertProductionUiProfile({ ...production, mintRecoveryUrl: undefined }, manifest),
    ).toThrow("recovery URL")
    for (const drift of [
      { canisterSchemaVersion: 35 },
      { canisterModuleSha256: undefined },
      { postActivationUpgradeSha256: undefined },
      { uiRpcConfigSha256: undefined },
    ]) {
      expect(() => assertProductionUiProfile({ ...production, ...drift }, manifest)).toThrow(
        "v36 module",
      )
    }
    expect(() =>
      assertProductionUiProfile({ testOnly: true, gateBManifestSha256: manifest }, manifest),
    ).toThrow("Production UI deploy rejects test-only")
    expect(() => assertProductionUiProfile({})).toThrow("Production UI deploy rejects test-only")
    expect(() => assertProductionUiProfile(production)).toThrow("requires a verified Gate B")
    expect(() =>
      assertProductionUiProfile({ ...production, gateBManifestSha256: null }, manifest),
    ).toThrow("does not match")
    expect(() => assertProductionUiProfile(production, "b".repeat(64))).toThrow("does not match")
    expect(() =>
      assertProductionUiProfile({ ...production, profileFileSha256: null }, manifest),
    ).toThrow("source profile hashes")
    expect(() =>
      assertProductionUiProfile(
        { ...production, environmentMode: "short-delay-test-only" },
        manifest,
      ),
    ).toThrow("environment modes")
    expect(() =>
      assertProductionUiProfile({ ...production, activationTimelockDelaySeconds: null }, manifest),
    ).toThrow("at least 24 hours")
    expect(() =>
      assertProductionUiProfile({ ...production, activationTimelockDelaySeconds: 300 }, manifest),
    ).toThrow("at least 24 hours")
    expect(() =>
      assertProductionUiProfile({ ...production, timelockAddress: null }, manifest),
    ).toThrow("Timelock contract address")
    expect(() =>
      assertProductionUiProfile({ ...production, deploymentBlock: 0n }, manifest),
    ).toThrow("positive deployment block")
    expect(() =>
      assertProductionUiProfile({ ...production, deploymentBlock: null }, manifest),
    ).toThrow("positive deployment block")
  })
})
