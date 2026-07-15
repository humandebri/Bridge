import { IDL } from "@dfinity/candid"
import { describe, expect, it } from "vitest"
import { decodeDepositReply, decodeNotifyWithdrawalReply, notifyWithdrawalErrorMessage } from "./wallet"

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

describe("withdrawal notification errors", () => {
  it("renders actionable RPC and rate-limit failures", () => {
    expect(notifyWithdrawalErrorMessage({ RpcInconsistent: null })).toContain("providers disagreed")
    expect(notifyWithdrawalErrorMessage({ RateLimited: { retry_after_seconds: 42n } })).toContain("42 seconds")
  })

  it("decodes the confirmed-head receipt shape used by the public Candid", () => {
    const settlement = IDL.Variant({
      Complete: IDL.Record({ state: IDL.Text }),
      Stopped: IDL.Record({ state: IDL.Text, reason: IDL.Variant({ RpcUnavailable: IDL.Null }) }),
      ReconciliationProgress: IDL.Record({ state: IDL.Text }),
      WaitingForConfirmation: IDL.Record({ transaction_hash: IDL.Vec(IDL.Nat8), state: IDL.Text }),
      Submitted: IDL.Record({ transaction_hash: IDL.Vec(IDL.Nat8), state: IDL.Text }),
    })
    const resultType = IDL.Variant({
      Ok: IDL.Variant({
        Duplicate: IDL.Record({ withdrawal_id: IDL.Vec(IDL.Nat8), settlement: IDL.Opt(settlement) }),
        Ingested: IDL.Record({ confirmed_head_block_number: IDL.Nat64, withdrawal_id: IDL.Vec(IDL.Nat8), settlement: IDL.Opt(settlement) }),
      }),
      Err: IDL.Variant({ BaseStateMismatch: IDL.Null, BridgeSignerMismatch: IDL.Null }),
    })
    const reply = new Uint8Array(IDL.encode([resultType], [{ Ok: { Ingested: { confirmed_head_block_number: 42n, withdrawal_id: new Uint8Array(32).fill(7), settlement: [] } } }]))

    expect(decodeNotifyWithdrawalReply(reply)).toMatchObject({ Ingested: { confirmed_head_block_number: 42n } })
  })

  it.each(["BaseStateMismatch", "BridgeSignerMismatch"] as const)("decodes %s as a normal bridge rejection", (variant) => {
    const resultType = IDL.Variant({
      Ok: IDL.Variant({
        Duplicate: IDL.Record({ withdrawal_id: IDL.Vec(IDL.Nat8), settlement: IDL.Opt(IDL.Null) }),
        Ingested: IDL.Record({ confirmed_head_block_number: IDL.Nat64, withdrawal_id: IDL.Vec(IDL.Nat8), settlement: IDL.Opt(IDL.Null) }),
      }),
      Err: IDL.Variant({ BaseStateMismatch: IDL.Null, BridgeSignerMismatch: IDL.Null }),
    })
    const reply = new Uint8Array(IDL.encode([resultType], [{ Err: { [variant]: null } }]))

    expect(() => decodeNotifyWithdrawalReply(reply)).toThrow(variant === "BaseStateMismatch" ? "state does not match" : "signer does not match")
  })
})
