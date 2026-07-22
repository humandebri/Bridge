import { act, render } from "@testing-library/react"
import { beforeEach, describe, expect, it, vi } from "vitest"
import { useIcWallet } from "@/features/wallet/ic-wallet-provider"
import { basePublicClient } from "@/lib/evm/client"
import { createBridgeActor } from "@/lib/ic/bridge"
import { NotifyWithdrawalCallError, SettlementActionCallError, type IcWalletAdapter } from "@/lib/ic/wallet"
import { PENDING_CONFIRMATIONS_CHANGED, pendingConfirmationsStorageKey, readPendingConfirmations, savePendingConfirmation, type PendingConfirmation } from "@/lib/pending-confirmations"
import { CONFIRMATION_POLL_MS, SettlementConfirmationCoordinator, confirmWhenFinalized, runWithConfirmationLock } from "./settlement-confirmation-coordinator"

vi.mock("sonner", () => ({ toast: { info: vi.fn(), success: vi.fn(), warning: vi.fn() } }))
vi.mock("@/features/wallet/ic-wallet-provider", () => ({ useIcWallet: vi.fn() }))
vi.mock("@/lib/evm/client", () => ({ basePublicClient: { getTransactionReceipt: vi.fn(), getBlock: vi.fn() } }))
vi.mock("@/lib/ic/bridge", () => ({ createBridgeActor: vi.fn() }))

const owner = "aaaaa-aa"
const deposit: PendingConfirmation = {
  kind: "deposit",
  settlementId: `0x${"01".repeat(32)}`,
  transactionHash: `0x${"02".repeat(32)}`,
  owner,
  blocked: false,
  bridgeCanisterId: "",
  chainId: 84532,
  bridgeAddress: "",
}

function adapter(confirmDeposit = vi.fn()): IcWalletAdapter {
  return {
    provider: "plug",
    connect: vi.fn(),
    getAccount: vi.fn().mockResolvedValue({ owner }),
    disconnect: vi.fn(),
    approve: vi.fn(),
    requestDeposit: vi.fn(),
    notifyWithdrawal: vi.fn(),
    confirmDeposit,
    continueDeposit: vi.fn(),
    continueWithdrawal: vi.fn(),
  }
}

function finalizedReceipt() {
  vi.mocked(basePublicClient.getTransactionReceipt).mockResolvedValue({ blockNumber: 10n, status: "success" } as never)
  vi.mocked(basePublicClient.getBlock).mockResolvedValue({ number: 12n } as never)
}

beforeEach(() => {
  window.localStorage.clear()
  for (let index = window.localStorage.length - 1; index >= 0; index -= 1) {
    const key = window.localStorage.key(index)
    if (key?.startsWith("kinic.bridge.confirmation-lease.")) window.localStorage.removeItem(key)
  }
  vi.clearAllMocks()
  Object.defineProperty(document, "visibilityState", { configurable: true, value: "visible" })
  Object.defineProperty(navigator, "locks", {
    configurable: true,
    value: {
      request: vi.fn(async (_name: string, _options: LockOptions, callback: (lock: Lock | null) => Promise<unknown>) => callback({ name: "confirmation" } as Lock)),
    },
  })
  vi.mocked(createBridgeActor).mockResolvedValue({
    get_deposit: vi.fn().mockResolvedValue([{ base_confirmation: [{ Submitted: { transaction_hash: new Uint8Array(32).fill(2) } }] }]),
  } as never)
})

describe("confirmWhenFinalized", () => {
  it("switches a pending deposit to the canonical replacement hash before receipt polling", async () => {
    vi.mocked(createBridgeActor).mockResolvedValue({
      get_deposit: vi.fn().mockResolvedValue([{ base_confirmation: [{ Submitted: { transaction_hash: new Uint8Array(32).fill(4) } }] }]),
    } as never)
    await savePendingConfirmation(deposit)

    expect(await confirmWhenFinalized(deposit, adapter())).toEqual({ status: "retry" })
    expect(basePublicClient.getTransactionReceipt).not.toHaveBeenCalled()
    expect(readPendingConfirmations()[0]?.transactionHash).toBe(`0x${"04".repeat(32)}`)
  })

  it("retries Base RPC failures and canister observations that are not ready", async () => {
    const wallet = adapter(vi.fn().mockResolvedValue({ WaitingForConfirmation: { state: { Deposit: { MintPending: null } }, transaction_hash: new Uint8Array(32) } }))
    vi.mocked(basePublicClient.getTransactionReceipt).mockRejectedValueOnce(new Error("RPC unavailable"))
    expect(await confirmWhenFinalized(deposit, wallet)).toEqual({ status: "retry" })

    finalizedReceipt()
    expect(await confirmWhenFinalized(deposit, wallet)).toEqual({ status: "retry" })
    expect(readPendingConfirmations()).toEqual([])
  })

  it.each([
    new SettlementActionCallError("Busy", "busy"),
    new SettlementActionCallError("AutomaticProgressPending", "pending"),
    new SettlementActionCallError("RateLimited", "limited", 42_000),
    new SettlementActionCallError("StorageFailure", "storage"),
  ])("retries transient settlement errors without blocking", async (error) => {
    finalizedReceipt()
    await savePendingConfirmation(deposit)
    const result = await confirmWhenFinalized(deposit, adapter(vi.fn().mockRejectedValue(error)))
    expect(result.status).toBe("retry")
    expect(readPendingConfirmations()[0]?.blocked).toBe(false)
  })

  it("blocks an explicit permanent settlement rejection", async () => {
    finalizedReceipt()
    await savePendingConfirmation(deposit)
    expect(await confirmWhenFinalized(deposit, adapter(vi.fn().mockRejectedValue(new SettlementActionCallError("TransactionMismatch", "mismatch"))))).toEqual({ status: "blocked" })
    expect(readPendingConfirmations()[0]?.blocked).toBe(true)
  })

  it("notifies and removes a finalized withdrawal", async () => {
    finalizedReceipt()
    const notifyWithdrawal = vi.fn().mockResolvedValue({ Ingested: { withdrawal_id: new Uint8Array(32), settlement: [] } })
    const wallet = adapter()
    wallet.notifyWithdrawal = notifyWithdrawal
    await savePendingConfirmation({ kind: "withdrawal", transactionHash: deposit.transactionHash, owner })
    const entry = readPendingConfirmations()[0]
    expect(entry?.kind).toBe("withdrawal")

    expect(await confirmWhenFinalized(entry!, wallet)).toEqual({ status: "complete" })
    expect(notifyWithdrawal).toHaveBeenCalledWith(new Uint8Array(32).fill(2))
    expect(readPendingConfirmations()).toEqual([])
  })

  it("discards a finalized reverted withdrawal without opening the IC wallet", async () => {
    vi.mocked(basePublicClient.getTransactionReceipt).mockResolvedValue({ blockNumber: 10n, status: "reverted" } as never)
    vi.mocked(basePublicClient.getBlock).mockResolvedValue({ number: 12n } as never)
    const notifyWithdrawal = vi.fn()
    const wallet = adapter()
    wallet.notifyWithdrawal = notifyWithdrawal
    await savePendingConfirmation({ kind: "withdrawal", transactionHash: deposit.transactionHash, owner })
    const entry = readPendingConfirmations()[0]!

    expect(await confirmWhenFinalized(entry, wallet)).toEqual({ status: "reverted" })
    expect(notifyWithdrawal).not.toHaveBeenCalled()
    expect(readPendingConfirmations()).toEqual([])
  })

  it("blocks a withdrawal owner mismatch", async () => {
    finalizedReceipt()
    const wallet = adapter()
    wallet.notifyWithdrawal = vi.fn().mockRejectedValue(new Error("The connected IC wallet does not own this withdrawal."))
    await savePendingConfirmation({ kind: "withdrawal", transactionHash: deposit.transactionHash, owner })
    const entry = readPendingConfirmations()[0]!

    expect(await confirmWhenFinalized(entry, wallet)).toEqual({ status: "blocked" })
    expect(readPendingConfirmations()[0]?.blocked).toBe(true)
  })

  it.each(["RpcUnavailable", "TransactionNotConfirmed", "Busy", "RateLimited", "InsufficientCycles"] as const)("retries transient withdrawal error %s", async (code) => {
    finalizedReceipt()
    const wallet = adapter()
    wallet.notifyWithdrawal = vi.fn().mockRejectedValue(new NotifyWithdrawalCallError(code, code))
    await savePendingConfirmation({ kind: "withdrawal", transactionHash: deposit.transactionHash, owner })
    const entry = readPendingConfirmations()[0]!

    expect(await confirmWhenFinalized(entry, wallet)).toEqual({ status: "retry", retryAt: undefined })
    expect(readPendingConfirmations()[0]?.blocked).toBe(false)
  })
})

describe("SettlementConfirmationCoordinator", () => {
  it("allows only one tab to enter the same navigator lock", async () => {
    const originalLocks = navigator.locks
    let held = false
    Object.defineProperty(navigator, "locks", {
      configurable: true,
      value: {
        request: vi.fn(async (_name: string, _options: LockOptions, callback: (lock: Lock | null) => Promise<string>) => {
          if (held) return callback(null)
          held = true
          try { return await callback({ name: "confirmation" } as Lock) }
          finally { held = false }
        }),
      },
    })
    let release!: () => void
    const firstAction = vi.fn(() => new Promise<string>((resolve) => { release = () => resolve("complete") }))
    const secondAction = vi.fn(() => Promise.resolve("duplicate"))

    const first = runWithConfirmationLock("deposit", deposit, firstAction)
    await Promise.resolve()
    expect(await runWithConfirmationLock("deposit", deposit, secondAction)).toBeUndefined()
    expect(secondAction).not.toHaveBeenCalled()
    release()
    await expect(first).resolves.toBe("complete")
    Object.defineProperty(navigator, "locks", { configurable: true, value: originalLocks })
  })

  it("allows only one fallback lease claimant to reach the external action", async () => {
    Object.defineProperty(navigator, "locks", { configurable: true, value: undefined })
    let release!: () => void
    const firstAction = vi.fn(() => new Promise<string>((resolve) => { release = () => resolve("complete") }))
    const secondAction = vi.fn(() => Promise.resolve("duplicate"))

    const first = runWithConfirmationLock("deposit", deposit, firstAction)
    expect(await runWithConfirmationLock("deposit", deposit, secondAction)).toBeUndefined()
    await vi.waitFor(() => expect(firstAction).toHaveBeenCalledOnce())
    expect(secondAction).not.toHaveBeenCalled()
    release()
    await expect(first).resolves.toBe("complete")
  })

  it("aborts a fallback action after its fencing token is replaced", async () => {
    Object.defineProperty(navigator, "locks", { configurable: true, value: undefined })

    const result = await runWithConfirmationLock("deposit", deposit, (lease) => {
      const storageKey = Object.keys(window.localStorage).find((key) => key.startsWith("kinic.bridge.confirmation-lease.v2:"))
      expect(storageKey).toBeDefined()
      window.localStorage.setItem(storageKey!, JSON.stringify({ ownerId: "new-owner", expiresAt: Date.now() + 30_000, fencingToken: Number.MAX_SAFE_INTEGER }))
      lease.assertCurrent()
      return Promise.resolve("unexpected")
    })

    expect(result).toBeUndefined()
  })

  it("fails closed for a malformed fallback lease record", async () => {
    Object.defineProperty(navigator, "locks", { configurable: true, value: undefined })
    const first = runWithConfirmationLock("deposit", deposit, () => Promise.resolve("first"))
    const storageKey = Object.keys(window.localStorage).find((key) => key.startsWith("kinic.bridge.confirmation-lease.v2:"))
    expect(storageKey).toBeDefined()
    window.localStorage.setItem(storageKey!, "malformed")

    await expect(runWithConfirmationLock("deposit", deposit, () => Promise.resolve("unsafe"))).resolves.toBeUndefined()
    await expect(first).resolves.toBeUndefined()
  })

  it("restores a submitted withdrawal after an RPC failure and completes it after remount", async () => {
    const notifyWithdrawal = vi.fn().mockResolvedValue({ Ingested: { withdrawal_id: new Uint8Array(32), settlement: [] } })
    const wallet = adapter()
    wallet.notifyWithdrawal = notifyWithdrawal
    vi.mocked(useIcWallet).mockReturnValue({ account: { owner }, adapter: wallet, provider: "plug", connect: vi.fn(), disconnect: vi.fn() })
    vi.mocked(basePublicClient.getTransactionReceipt).mockRejectedValueOnce(new Error("RPC unavailable"))
    await savePendingConfirmation({ kind: "withdrawal", transactionHash: deposit.transactionHash, owner })

    const firstView = render(<SettlementConfirmationCoordinator />)
    await flushPromises()
    expect(readPendingConfirmations()).toHaveLength(1)
    expect(readPendingConfirmations()[0]?.blocked).toBe(false)
    expect(notifyWithdrawal).not.toHaveBeenCalled()
    firstView.unmount()

    finalizedReceipt()
    const restoredView = render(<SettlementConfirmationCoordinator />)
    await flushPromises()
    expect(notifyWithdrawal).toHaveBeenCalledOnce()
    expect(readPendingConfirmations()).toEqual([])
    restoredView.unmount()
  })

  it("rechecks immediately when an external event reports new settlement progress", async () => {
    vi.useFakeTimers()
    const confirmDeposit = vi.fn().mockResolvedValue({ WaitingForConfirmation: { state: { Deposit: { MintPending: null } }, transaction_hash: new Uint8Array(32) } })
    vi.mocked(useIcWallet).mockReturnValue({ account: { owner }, adapter: adapter(confirmDeposit), provider: "plug", connect: vi.fn(), disconnect: vi.fn() })
    vi.mocked(basePublicClient.getTransactionReceipt).mockRejectedValueOnce(new Error("RPC unavailable"))
    await savePendingConfirmation(deposit)
    const view = render(<SettlementConfirmationCoordinator />)

    await flushPromises()
    expect(confirmDeposit).not.toHaveBeenCalled()

    finalizedReceipt()
    await act(() => window.dispatchEvent(new StorageEvent("storage", { key: pendingConfirmationsStorageKey() })))
    await flushPromises()
    expect(confirmDeposit).toHaveBeenCalledOnce()

    view.unmount()
    vi.useRealTimers()
  })

  it("polls multiple settlements independently and resumes immediately when visible", async () => {
    vi.useFakeTimers()
    finalizedReceipt()
    const confirmDeposit = vi.fn().mockResolvedValue({ WaitingForConfirmation: { state: { Deposit: { MintPending: null } }, transaction_hash: new Uint8Array(32) } })
    vi.mocked(useIcWallet).mockReturnValue({ account: { owner }, adapter: adapter(confirmDeposit), provider: "plug", connect: vi.fn(), disconnect: vi.fn() })
    await savePendingConfirmation(deposit)
    await savePendingConfirmation({ ...deposit, settlementId: `0x${"03".repeat(32)}` })
    const view = render(<SettlementConfirmationCoordinator />)

    await flushPromises()
    expect(confirmDeposit).toHaveBeenCalledTimes(2)
    Object.defineProperty(document, "visibilityState", { configurable: true, value: "hidden" })
    await act(() => vi.advanceTimersByTimeAsync(CONFIRMATION_POLL_MS))
    expect(confirmDeposit).toHaveBeenCalledTimes(2)

    Object.defineProperty(document, "visibilityState", { configurable: true, value: "visible" })
    await act(() => document.dispatchEvent(new Event("visibilitychange")))
    await flushPromises()
    expect(confirmDeposit).toHaveBeenCalledTimes(4)

    await act(() => window.dispatchEvent(new Event(PENDING_CONFIRMATIONS_CHANGED)))
    await flushPromises()
    expect(confirmDeposit).toHaveBeenCalledTimes(6)
    await act(() => vi.advanceTimersByTimeAsync(CONFIRMATION_POLL_MS))
    await flushPromises()
    expect(confirmDeposit).toHaveBeenCalledTimes(8)
    view.unmount()
    vi.useRealTimers()
  })
})

async function flushPromises(): Promise<void> {
  await act(async () => {
    for (let index = 0; index < 8; index += 1) await Promise.resolve()
  })
}
