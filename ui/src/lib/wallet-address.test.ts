import { describe, expect, it } from "vitest"
import { shortenWalletAddress } from "./wallet-address"

describe("shortenWalletAddress", () => {
  it("shortens IC and EVM addresses while leaving connection prompts intact", () => {
    expect(shortenWalletAddress("7upbt-d4p76-s34bm-nl3iw-ry7i7-oyqvf-iy36c-osku3-4mi7w-dgaks-iqe")).toBe("7upbt-…-iqe")
    expect(shortenWalletAddress("0x88F88c9667ECB746c11b8a0182f11F622FFbb844")).toBe("0x88F8…b844")
    expect(shortenWalletAddress("Connect IC wallet")).toBe("Connect IC wallet")
  })
})
