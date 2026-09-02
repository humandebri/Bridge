import { IDL } from "@icp-sdk/core/candid"
import { Principal } from "@icp-sdk/core/principal"
import { afterEach, describe, expect, it, vi } from "vitest"
import { idlFactory } from "@/generated/bridge.idl"
import { decodeDepositReply, OisyAdapter, PlugAdapter, requestDepositRefundErrorMessage } from "./wallet"

const bridgeService: IDL.ServiceClass = idlFactory({ IDL })
function resultType(method: "request_deposit") {
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
  const createWallet = (currentOwner = owner, subaccount?: string) => ({
    requestPermissionsNotGranted: vi.fn().mockResolvedValue({ allPermissionsGranted: true }),
    accounts: vi.fn().mockResolvedValue([{ owner: currentOwner, subaccount }]),
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

  it("rejects_a_changed_restored_OISY_account_before_an_action", async () => {
    const wallet = createWallet("2vxsx-fae")
    const connectWallet = vi.fn().mockResolvedValue(wallet)
    const adapter = new OisyAdapter("https://icp-api.io", owner, owner, connectWallet, { owner })

    await expect(adapter.prepare()).rejects.toThrow("OISY account changed")
    expect(wallet.approve).not.toHaveBeenCalled()
    expect(wallet.callCanister).not.toHaveBeenCalled()
    expect(wallet.disconnect).toHaveBeenCalledOnce()
  })

  it("rejects a changed restored OISY subaccount before an action", async () => {
    const expectedSubaccount = new Uint8Array(32).fill(1)
    const wallet = createWallet(owner, `0x${"02".repeat(32)}`)
    const connectWallet = vi.fn().mockResolvedValue(wallet)
    const adapter = new OisyAdapter("https://icp-api.io", owner, owner, connectWallet, {
      owner,
      subaccount: expectedSubaccount,
    })

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

  it("calls_continue_deposit_with_the_exact_deposit_ID", async () => {
    const depositId = new Uint8Array(32).fill(7)
    const wallet = createWallet()
    wallet.callCanister.mockRejectedValue(new Error("stop after dispatch"))
    const connectWallet = vi.fn().mockResolvedValue(wallet)
    const adapter = new OisyAdapter("https://icp-api.io", owner, owner, connectWallet, { owner })
    const method = (bridgeService._fields as Array<[string, IDL.FuncClass]>)
      .find(([name]) => name === "continue_deposit")?.[1]
    if (!method) throw new Error("Missing generated continue_deposit codec")
    const encoded = new Uint8Array(IDL.encode(method.argTypes, [depositId]))
    const expectedArg = btoa(String.fromCharCode(...encoded))

    await expect(adapter.continueDeposit(depositId)).rejects.toThrow("stop after dispatch")
    expect(wallet.callCanister).toHaveBeenCalledWith({
      canisterId: owner,
      sender: owner,
      method: "continue_deposit",
      arg: expectedArg,
    })
    expect(wallet.disconnect).toHaveBeenCalledOnce()
  })
})

describe("Plug restored account validation", () => {
  afterEach(() => {
    Object.defineProperty(window, "ic", { configurable: true, value: undefined })
  })

  function installPlug(owner: string, options: { connected?: boolean; withAgent?: boolean } = {}) {
    const plug = {
      isConnected: vi.fn().mockResolvedValue(options.connected ?? true),
      requestConnect: vi.fn().mockResolvedValue(true),
      disconnect: vi.fn().mockResolvedValue(undefined),
      agent: options.withAgent === false
        ? undefined
        : { getPrincipal: vi.fn().mockResolvedValue(Principal.fromText(owner)) },
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
    expect(plug.isConnected).toHaveBeenCalledOnce()
    expect(plug.requestConnect).not.toHaveBeenCalled()
  })

  it("reconnects a restored Plug session before clearing the user gesture", async () => {
    const plug = installPlug("aaaaa-aa", { connected: false })
    const adapter = new PlugAdapter("https://icp-api.io", "aaaaa-aa", "aaaaa-aa", { owner: "aaaaa-aa" })

    await adapter.prepare()

    expect(plug.requestConnect).toHaveBeenCalledWith({
      whitelist: ["aaaaa-aa", "aaaaa-aa"],
      host: "https://icp-api.io",
    })
    expect(adapter.requiresUserGesture).toBe(false)
  })

  it("reconnects when Plug reports a session without an Agent", async () => {
    const plug = installPlug("aaaaa-aa", { withAgent: false })
    plug.requestConnect.mockImplementation(() => {
      plug.agent = { getPrincipal: vi.fn().mockResolvedValue(Principal.fromText("aaaaa-aa")) }
      return Promise.resolve(true)
    })
    const adapter = new PlugAdapter("https://icp-api.io", "aaaaa-aa", "aaaaa-aa", { owner: "aaaaa-aa" })

    await adapter.prepare()

    expect(plug.requestConnect).toHaveBeenCalledOnce()
    expect(adapter.requiresUserGesture).toBe(false)
  })

  it("keeps the user gesture requirement when reconnect is rejected", async () => {
    const plug = installPlug("aaaaa-aa", { connected: false })
    plug.requestConnect.mockResolvedValue(false)
    const adapter = new PlugAdapter("https://icp-api.io", "aaaaa-aa", "aaaaa-aa", { owner: "aaaaa-aa" })

    await expect(adapter.prepare()).rejects.toThrow("Plug connection was rejected")
    expect(adapter.requiresUserGesture).toBe(true)
  })

  it("rejects a changed Plug principal before creating an actor", async () => {
    const plug = installPlug("2vxsx-fae")
    const adapter = new PlugAdapter("https://icp-api.io", "aaaaa-aa", "aaaaa-aa", { owner: "aaaaa-aa" })

    await expect(adapter.getAccount()).rejects.toThrow("Plug account changed")
    expect(plug.createActor).not.toHaveBeenCalled()
    await expect(adapter.prepare()).rejects.toThrow("Plug account changed")
    expect(adapter.requiresUserGesture).toBe(true)
  })

  it("re-reads the Plug principal after an approval prompt", async () => {
    const plug = installPlug("aaaaa-aa")
    plug.agent!.getPrincipal
      .mockResolvedValueOnce(Principal.fromText("aaaaa-aa"))
      .mockResolvedValueOnce(Principal.fromText("2vxsx-fae"))
    plug.createActor.mockResolvedValue({ icrc2_approve: vi.fn().mockResolvedValue({ Ok: 1n }) })
    const adapter = new PlugAdapter("https://icp-api.io", "aaaaa-aa", "aaaaa-aa", { owner: "aaaaa-aa" })

    await expect(adapter.approve({ amount: 10n, currentAllowance: 0n, ledgerFee: 1n }))
      .rejects.toThrow("Plug account changed")
  })

  it("re-reads the Plug principal after a deposit prompt", async () => {
    const plug = installPlug("aaaaa-aa")
    plug.agent!.getPrincipal
      .mockResolvedValueOnce(Principal.fromText("aaaaa-aa"))
      .mockResolvedValueOnce(Principal.fromText("2vxsx-fae"))
    plug.createActor.mockResolvedValue({ request_deposit: vi.fn().mockResolvedValue({}) })
    const adapter = new PlugAdapter("https://icp-api.io", "aaaaa-aa", "aaaaa-aa", { owner: "aaaaa-aa" })

    await expect(adapter.requestDeposit({
      ownerSequence: 1n,
      baseRecipient: new Uint8Array(20),
      grossAmount: 10n,
      maxServiceFee: 1n,
    })).rejects.toThrow("Plug account changed")
  })

  it("accepts_AuthorizationWindowTooShort_as_a_valid_stopped_continuation", async () => {
    const plug = installPlug("aaaaa-aa")
    plug.createActor.mockResolvedValue({
      continue_deposit: vi.fn().mockResolvedValue({
        Ok: {
          Stopped: {
            state: { Deposit: { AuthorizationPending: null } },
            reason: { AuthorizationWindowTooShort: null },
          },
        },
      }),
    })
    const adapter = new PlugAdapter("https://icp-api.io", "aaaaa-aa", "aaaaa-aa", { owner: "aaaaa-aa" })

    await expect(adapter.continueDeposit(new Uint8Array(32).fill(7))).resolves.toEqual({
      Stopped: {
        state: { Deposit: { AuthorizationPending: null } },
        reason: { AuthorizationWindowTooShort: null },
      },
    })
  })
})
