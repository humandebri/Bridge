import { describe, expect, it } from "vitest"
import { runtimeBytecodeSha256 } from "./runtime-bytecode-hash"

describe("runtime bytecode hashing", () => {
  it("uses SHA-256 over bytecode bytes, not Keccak-256", () => {
    expect(runtimeBytecodeSha256("0x01")).toBe(
      "0x4bf5122f344554c53bde2ebb8cd2b7e3d1600ad631c385a5d7cce23c7785459a",
    )
  })
})
