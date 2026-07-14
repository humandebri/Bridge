import { IDL } from "@dfinity/candid"
import { describe, expect, it } from "vitest"
import { decodeDepositReply } from "./wallet"

describe("OISY deposit reply decoding", () => {
  it("decodes RateLimited as a normal bridge rejection", () => {
    const resultType = IDL.Variant({
      Ok: IDL.Record({ deposit_id: IDL.Vec(IDL.Nat8), state: IDL.Text }),
      Err: IDL.Variant({ RateLimited: IDL.Record({ retry_after_seconds: IDL.Nat64 }) }),
    })
    const reply = new Uint8Array(IDL.encode([resultType], [{ Err: { RateLimited: { retry_after_seconds: 42n } } }]))

    expect(() => decodeDepositReply(reply)).toThrow("Bridge rejected deposit")
    expect(() => decodeDepositReply(reply)).toThrow("42")
  })
})
