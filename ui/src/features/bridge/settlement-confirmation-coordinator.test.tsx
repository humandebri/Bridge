import { act, render } from "@testing-library/react"
import { beforeEach, describe, expect, it, vi } from "vitest"
import { useIcWallet } from "@/features/wallet/ic-wallet-provider"
import { basePublicClient } from "@/lib/evm/client"
import { NotifyWithdrawalCallError, SettlementActionCallError, type IcWalletAdapter } from "@/lib/ic/wallet"
import { PENDING_CONFIRMATIONS_CHANGED, readPendingConfirmations, removePendingConfirmation, savePendingConfirmation, type PendingConfirmation } from "@/lib/pending-confirmations"
import { CONFIRMATION_POLL_MS, SettlementConfirmationCoordinator, confirmWhenFinalized } from "./settlement-confirmation-coordinator"

vi.mock("sonner", () => ({ toast: { info: vi.fn(), success: vi.fn(), warning: vi.fn() } }))
vi.mock("@/features/wallet/ic-wallet-provider", () => ({ useIcWallet: vi.fn() }))
vi.mock("@/lib/evm/client", () => ({ basePublicClient: { getTransactionReceipt: vi.fn(), getBlock: vi.fn() } }))

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
    getAccount: vi.fn(),
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
  vi.mocked(basePublicClient.getTransactionReceipt).mockResolvedValue({ blockNumber: 10n } as never)
  vi.mocked(basePublicClient.getBlock).mockResolvedValue({ number: 12n } as never)
}

beforeEach(() => {
  for (const entry of readPendingConfirmations()) removePendingConfirmation(entry)
  vi.clearAllMocks()
  Object.defineProperty(document, "visibilityState", { configurable: true, value: "visible" })
})

describe("confirmWhenFinalized", () => {
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
    savePendingConfirmation(deposit)
    const result = await confirmWhenFinalized(deposit, adapter(vi.fn().mockRejectedValue(error)))
    expect(result.status).toBe("retry")
    expect(readPendingConfirmations()[0]?.blocked).toBe(false)
  })

  it("blocks an explicit permanent settlement rejection", async () => {
    finalizedReceipt()
    savePendingConfirmation(deposit)
    expect(await confirmWhenFinalized(deposit, adapter(vi.fn().mockRejectedValue(new SettlementActionCallError("TransactionMismatch", "mismatch"))))).toEqual({ status: "blocked" })
    expect(readPendingConfirmations()[0]?.blocked).toBe(true)
  })

  it("notifies and removes a finalized withdrawal", async () => {
    finalizedReceipt()
    const notifyWithdrawal = vi.fn().mockResolvedValue({ Ingested: { withdrawal_id: new Uint8Array(32), settlement: [] } })
    const wallet = adapter()
    wallet.notifyWithdrawal = notifyWithdrawal
    savePendingConfirmation({ kind: "withdrawal", transactionHash: deposit.transactionHash, owner })
    const entry = readPendingConfirmations()[0]
    expect(entry?.kind).toBe("withdrawal")

    expect(await confirmWhenFinalized(entry!, wallet)).toEqual({ status: "complete" })
    expect(notifyWithdrawal).toHaveBeenCalledWith(new Uint8Array(32).fill(2))
    expect(readPendingConfirmations()).toEqual([])
  })

  it("blocks a withdrawal owner mismatch", async () => {
    finalizedReceipt()
    const wallet = adapter()
    wallet.notifyWithdrawal = vi.fn().mockRejectedValue(new Error("The connected IC wallet does not own this withdrawal."))
    savePendingConfirmation({ kind: "withdrawal", transactionHash: deposit.transactionHash, owner })
    const entry = readPendingConfirmations()[0]!

    expect(await confirmWhenFinalized(entry, wallet)).toEqual({ status: "blocked" })
    expect(readPendingConfirmations()[0]?.blocked).toBe(true)
  })

  it.each(["RpcUnavailable", "TransactionNotConfirmed", "Busy"] as const)("retries transient withdrawal error %s", async (code) => {
    finalizedReceipt()
    const wallet = adapter()
    wallet.notifyWithdrawal = vi.fn().mockRejectedValue(new NotifyWithdrawalCallError(code, code))
    savePendingConfirmation({ kind: "withdrawal", transactionHash: deposit.transactionHash, owner })
    const entry = readPendingConfirmations()[0]!

    expect(await confirmWhenFinalized(entry, wallet)).toEqual({ status: "retry", retryAt: undefined })
    expect(readPendingConfirmations()[0]?.blocked).toBe(false)
  })
})

describe("SettlementConfirmationCoordinator", () => {
  it("polls multiple settlements independently and resumes immediately when visible", async () => {
    vi.useFakeTimers()
    finalizedReceipt()
    const confirmDeposit = vi.fn().mockResolvedValue({ WaitingForConfirmation: { state: { Deposit: { MintPending: null } }, transaction_hash: new Uint8Array(32) } })
    vi.mocked(useIcWallet).mockReturnValue({ account: { owner }, adapter: adapter(confirmDeposit), provider: "plug", connect: vi.fn(), disconnect: vi.fn() })
    savePendingConfirmation(deposit)
    savePendingConfirmation({ ...deposit, settlementId: `0x${"03".repeat(32)}` })
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
    await act(() => vi.advanceTimersByTimeAsync(CONFIRMATION_POLL_MS))
    await flushPromises()
    expect(confirmDeposit).toHaveBeenCalledTimes(6)
    view.unmount()
    vi.useRealTimers()
  })
})

async function flushPromises(): Promise<void> {
  await act(async () => {
    for (let index = 0; index < 8; index += 1) await Promise.resolve()
  })
}
