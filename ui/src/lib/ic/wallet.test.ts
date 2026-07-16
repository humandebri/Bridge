import { IDL } from "@dfinity/candid"
import { describe, expect, it } from "vitest"
import { idlFactory } from "@/generated/bridge.idl"
import { decodeDepositReply, decodeNotifyWithdrawalReply, notifyWithdrawalErrorMessage } from "./wallet"

// didc's runtime JS intentionally has no static return type; the checked-in TS binding is the typed contract.
// eslint-disable-next-line @typescript-eslint/no-unsafe-assignment
const bridgeService: IDL.ServiceClass = idlFactory({ IDL })
function resultType(method: "request_deposit" | "notify_withdrawal") {
  const codec = (bridgeService._fields as Array<[string, IDL.FuncClass]>).find(([name]) => name === method)?.[1]
  if (!codec) throw new Error(`Missing generated codec for ${method}`)
  const result = codec.retTypes[0]
  if (!result) throw new Error(`Missing generated result codec for ${method}`)
  return result
}

describe("OISY deposit reply decoding", () => {
  it("decodes RateLimited as a normal bridge rejection", () => {
    const reply = new Uint8Array(IDL.encode([resultType("request_deposit")], [{ Err: { RateLimited: { retry_after_seconds: 42n } } }]))

    expect(() => decodeDepositReply(reply)).toThrow("Bridge rejected deposit")
    expect(() => decodeDepositReply(reply)).toThrow("42")
  })
})

describe("withdrawal notification errors", () => {
  it("renders actionable RPC and rate-limit failures", () => {
    expect(notifyWithdrawalErrorMessage({ RpcInconsistent: null })).toContain("providers disagreed")
  })

  it("decodes the confirmed-head receipt shape used by the public Candid", () => {
    const reply = new Uint8Array(IDL.encode([resultType("notify_withdrawal")], [{ Ok: { Ingested: { finalized_head_block_number: 42n, withdrawal_id: new Uint8Array(32).fill(7), settlement: [] } } }]))

    expect(decodeNotifyWithdrawalReply(reply)).toMatchObject({ Ingested: { finalized_head_block_number: 42n } })
  })

  it.each(["BaseStateMismatch", "BridgeSignerMismatch"] as const)("decodes %s as a normal bridge rejection", (variant) => {
    const reply = new Uint8Array(IDL.encode([resultType("notify_withdrawal")], [{ Err: { [variant]: null } }]))

    expect(() => decodeNotifyWithdrawalReply(reply)).toThrow(variant === "BaseStateMismatch" ? "state does not match" : "signer does not match")
  })

})
