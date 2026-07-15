import { describe, expect, it } from "vitest"
import { assertProductionUiProfile } from "./deploy-safety"

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
