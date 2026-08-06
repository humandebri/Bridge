import { IDL } from "@dfinity/candid"
import { Principal } from "@dfinity/principal"
import { afterEach, describe, expect, it, vi } from "vitest"
import { idlFactory } from "@/generated/bridge.idl"
import { decodeDepositReply, decodeNotifyWithdrawalReply, NotifyWithdrawalCallError, notifyWithdrawalErrorMessage, OisyAdapter, PlugAdapter, requestDepositRefundErrorMessage } from "./wallet"

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
  it("keeps refund admission retry guidance actionable", () => {
    expect(requestDepositRefundErrorMessage({ RateLimited: { retry_after_seconds: 42n } })).toContain("Retry after")
    expect(requestDepositRefundErrorMessage({ AutomaticProgressPending: { next_run_at_ns: [2_000_000_000n] } })).toContain("Next confirmation check")
  })

  it("decodes RateLimited as a normal bridge rejection", () => {
    const reply = new Uint8Array(IDL.encode([resultType("request_deposit")], [{ Err: { RateLimited: { retry_after_seconds: 42n } } }]))

    expect(() => decodeDepositReply(reply)).toThrow("Bridge rejected deposit")
    expect(() => decodeDepositReply(reply)).toThrow("42")
  })

  it("explains a cycles reserve rejection without treating funding as accepted", () => {
    const reply = new Uint8Array(IDL.encode(
      [resultType("request_deposit")],
      [{ Err: { ReserveUnavailable: null } }],
    ))

    expect(() => decodeDepositReply(reply)).toThrow("cycles reserve is temporarily insufficient")
  })

  it("decodes typed funding rejection and retry guidance", () => {
    const rejected = new Uint8Array(IDL.encode(
      [resultType("request_deposit")],
      [{ Err: { FundingRejected: { InsufficientAllowance: { allowance: 7n } } } }],
    ))
    const unavailable = new Uint8Array(IDL.encode(
      [resultType("request_deposit")],
      [{ Err: { FundingUnavailable: { retry_after_seconds: 30n } } }],
    ))

    expect(() => decodeDepositReply(rejected)).toThrow("allowance is insufficient")
    expect(() => decodeDepositReply(rejected)).toThrow("current 7")
    expect(() => decodeDepositReply(unavailable)).toThrow("Retry the same deposit in 30 seconds")
  })

  it("explains reservation maintenance with a same-deposit retry delay", () => {
    const reply = new Uint8Array(IDL.encode(
      [resultType("request_deposit")],
      [{ Err: { ReservationMaintenance: { retry_after_seconds: 17n } } }],
    ))

    expect(() => decodeDepositReply(reply)).toThrow("Retry the same deposit in 17 seconds")
  })
})

describe("OISY popup lifecycle", () => {
  const owner = "aaaaa-aa"
  const createWallet = (currentOwner = owner) => ({
    requestPermissionsNotGranted: vi.fn().mockResolvedValue({ allPermissionsGranted: true }),
    accounts: vi.fn().mockResolvedValue([{ owner: currentOwner }]),
    approve: vi.fn().mockResolvedValue(7n),
    callCanister: vi.fn(),
    disconnect: vi.fn().mockResolvedValue(undefined),
  })

  it("closes the popup after connecting while retaining the selected account", async () => {
    const wallet = createWallet()
    const connectWallet = vi.fn().mockResolvedValue(wallet)
    const adapter = new OisyAdapter("https://icp-api.io", owner, owner, connectWallet)

    await expect(adapter.connect()).resolves.toEqual({ owner })
    await expect(adapter.getAccount()).resolves.toEqual({ owner })

    expect(connectWallet).toHaveBeenCalledOnce()
    expect(wallet.disconnect).toHaveBeenCalledOnce()
  })

  it("restores the expected account without opening OISY and validates it on prepare", async () => {
    const wallet = createWallet()
    const connectWallet = vi.fn().mockResolvedValue(wallet)
    const adapter = new OisyAdapter("https://icp-api.io", owner, owner, connectWallet, { owner })

    await expect(adapter.getAccount()).resolves.toEqual({ owner })
    expect(connectWallet).not.toHaveBeenCalled()

    const close = await adapter.prepare()
    expect(connectWallet).toHaveBeenCalledOnce()
    await close()
    expect(wallet.disconnect).toHaveBeenCalledOnce()
  })

  it("rejects a changed restored OISY account before an action", async () => {
    const wallet = createWallet("2vxsx-fae")
    const connectWallet = vi.fn().mockResolvedValue(wallet)
    const adapter = new OisyAdapter("https://icp-api.io", owner, owner, connectWallet, { owner })

    await expect(adapter.prepare()).rejects.toThrow("OISY account changed")
    expect(wallet.approve).not.toHaveBeenCalled()
    expect(wallet.callCanister).not.toHaveBeenCalled()
    expect(wallet.disconnect).toHaveBeenCalledOnce()
  })

  it("reuses a prepared popup for approval and closes it after the action", async () => {
    const initialWallet = createWallet()
    const approvalWallet = createWallet()
    const connectWallet = vi.fn()
      .mockResolvedValueOnce(initialWallet)
      .mockResolvedValueOnce(approvalWallet)
    const adapter = new OisyAdapter("https://icp-api.io", owner, owner, connectWallet)
    await adapter.connect()
    const close = await adapter.prepare()

    await expect(adapter.approve({ amount: 10n, currentAllowance: 0n, ledgerFee: 1n })).resolves.toBe(7n)
    expect(approvalWallet.disconnect).not.toHaveBeenCalled()
    await close()

    expect(connectWallet).toHaveBeenCalledTimes(2)
    expect(approvalWallet.approve).toHaveBeenCalledOnce()
    expect(approvalWallet.disconnect).toHaveBeenCalledOnce()
  })

  it("rejects a changed OISY account and still closes the popup", async () => {
    const initialWallet = createWallet()
    const changedWallet = createWallet("2vxsx-fae")
    const connectWallet = vi.fn()
      .mockResolvedValueOnce(initialWallet)
      .mockResolvedValueOnce(changedWallet)
    const adapter = new OisyAdapter("https://icp-api.io", owner, owner, connectWallet)
    await adapter.connect()

    await expect(adapter.approve({ amount: 10n, currentAllowance: 0n, ledgerFee: 1n })).rejects.toThrow("OISY account changed")

    expect(changedWallet.approve).not.toHaveBeenCalled()
    expect(changedWallet.disconnect).toHaveBeenCalledOnce()
  })
})

describe("Plug restored account validation", () => {
  afterEach(() => {
    Object.defineProperty(window, "ic", { configurable: true, value: undefined })
  })

  function installPlug(owner: string) {
    const plug = {
      requestConnect: vi.fn(),
      disconnect: vi.fn().mockResolvedValue(undefined),
      agent: { getPrincipal: vi.fn().mockResolvedValue(Principal.fromText(owner)) },
      createActor: vi.fn(),
    }
    Object.defineProperty(window, "ic", { configurable: true, value: { plug } })
    return plug
  }

  it("uses a restored Plug account without requesting a new connection", async () => {
    const plug = installPlug("aaaaa-aa")
    const adapter = new PlugAdapter("https://icp-api.io", "aaaaa-aa", "aaaaa-aa", { owner: "aaaaa-aa" })

    expect(adapter.requiresUserGesture).toBe(true)
    await expect(adapter.getAccount()).resolves.toEqual({ owner: "aaaaa-aa" })
    expect(plug.requestConnect).not.toHaveBeenCalled()

    await adapter.prepare()
    expect(adapter.requiresUserGesture).toBe(false)
  })

  it("rejects a changed Plug principal before creating an actor", async () => {
    const plug = installPlug("2vxsx-fae")
    const adapter = new PlugAdapter("https://icp-api.io", "aaaaa-aa", "aaaaa-aa", { owner: "aaaaa-aa" })

    await expect(adapter.getAccount()).rejects.toThrow("Plug account changed")
    expect(plug.createActor).not.toHaveBeenCalled()
  })
})

describe("withdrawal notification errors", () => {
  it("renders actionable RPC and rate-limit failures", () => {
    expect(notifyWithdrawalErrorMessage({ RpcInconsistent: null })).toContain("providers disagreed")
  })

  it("decodes the confirmed-head receipt shape used by the public Candid", () => {
    const reply = new Uint8Array(IDL.encode([resultType("notify_withdrawal")], [{ Ok: { Ingested: { finalized_head_block_number: 42n, withdrawal_id: new Uint8Array(32).fill(7) } } }]))

    expect(decodeNotifyWithdrawalReply(reply)).toMatchObject({ Ingested: { finalized_head_block_number: 42n } })
  })

  it.each(["BaseStateMismatch", "BridgeSignerMismatch"] as const)("decodes %s as a normal bridge rejection", (variant) => {
    const reply = new Uint8Array(IDL.encode([resultType("notify_withdrawal")], [{ Err: { [variant]: null } }]))

    expect(() => decodeNotifyWithdrawalReply(reply)).toThrow(variant === "BaseStateMismatch" ? "state does not match" : "signer does not match")
  })

  it.each(["RateLimited", "InsufficientCycles"] as const)("decodes %s as a normal bridge rejection", (variant) => {
    const reply = new Uint8Array(IDL.encode([resultType("notify_withdrawal")], [{ Err: { [variant]: null } }]))

    expect(() => decodeNotifyWithdrawalReply(reply)).toThrow(variant === "RateLimited" ? "rate limited" : "enough cycles")
  })

  it("decodes the fee guard as an actionable bridge rejection", () => {
    const reply = new Uint8Array(IDL.encode([resultType("notify_withdrawal")], [{
      Err: { LedgerFeeExceedsServiceFee: { charged_service_fee: 10n, ledger_fee: 11n } },
    }]))

    let thrown: unknown
    try {
      decodeNotifyWithdrawalReply(reply)
    } catch (error) {
      thrown = error
    }
    expect(thrown).toBeInstanceOf(NotifyWithdrawalCallError)
    expect((thrown as NotifyWithdrawalCallError).code).toBe("LedgerFeeExceedsServiceFee")
    expect((thrown as Error).message).toContain("ledger fee exceeded")
    expect((thrown as Error).message).toContain("Contact the bridge operator")
    expect((thrown as Error).message).not.toContain("will be retried")
    expect((thrown as Error).message).not.toContain("invalid withdrawal notification error")
  })

})
