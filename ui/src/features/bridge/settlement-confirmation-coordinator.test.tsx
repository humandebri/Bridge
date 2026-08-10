import { cleanup, render, waitFor } from "@testing-library/react"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"

const mocks = vi.hoisted(() => ({
  notifyWithdrawal: vi.fn(),
  continueWithdrawal: vi.fn(),
  getReceipt: vi.fn(),
  getBlock: vi.fn(),
  readPending: vi.fn(),
  removePending: vi.fn(),
  setBlocked: vi.fn(),
  getWithdrawal: vi.fn(),
  update: vi.fn(),
  setAction: vi.fn(),
  toastInfo: vi.fn(),
  toastWarning: vi.fn(),
  toastError: vi.fn(),
  progress: undefined as undefined | Record<string, unknown>,
}))

vi.mock("@/features/bridge/bridge-progress-provider", () => ({
  useBridgeProgress: () => ({ progress: mocks.progress, update: mocks.update, setAction: mocks.setAction }),
}))
vi.mock("@/lib/evm/client", () => ({
  basePublicClient: { getTransactionReceipt: mocks.getReceipt, getBlock: mocks.getBlock },
}))
vi.mock("@/lib/pending-confirmations", () => ({
  readPendingConfirmations: mocks.readPending,
  removePendingConfirmation: mocks.removePending,
  setPendingConfirmationBlocked: mocks.setBlocked,
}))
vi.mock("@/lib/ic/bridge", () => ({
  createBridgeActor: vi.fn().mockResolvedValue({ get_withdrawal: mocks.getWithdrawal }),
}))
vi.mock("@/lib/ic/withdrawal-notification-client", () => ({
  NotifyWithdrawalCallError: class NotifyWithdrawalCallError extends Error {},
  continueWithdrawalWithBrowserIdentity: mocks.continueWithdrawal,
  notifyWithdrawalWithBrowserIdentity: mocks.notifyWithdrawal,
}))
vi.mock("@/lib/withdrawal-notification", () => ({
  withdrawalNotificationPresentation: () => ({ tone: "info", message: "recorded" }),
}))
vi.mock("sonner", () => ({ toast: { info: mocks.toastInfo, warning: mocks.toastWarning, error: mocks.toastError, success: vi.fn() } }))
vi.mock("@/config/profile", () => ({ deploymentProfile: { icHost: "https://ic.example", bridgeCanisterId: "aaaaa-aa" } }))

import { SettlementConfirmationCoordinator } from "./settlement-confirmation-coordinator"

const hash = `0x${"33".repeat(32)}` as const
const pending = { kind: "withdrawal", transactionHash: hash, owner: "aaaaa-aa", blocked: false }

beforeEach(() => {
  vi.clearAllMocks()
  mocks.progress = {
    id: "withdraw:1",
    direction: "withdraw",
    phase: "base-withdrawal-submitted",
    transactionHash: hash,
    receiveAmount: "1.5",
    receiveSymbol: "TICRC1",
    destination: "aaaaa-aa",
  }
  mocks.readPending.mockReturnValue([pending])
  mocks.getReceipt.mockResolvedValue({ status: "success", blockNumber: 10n })
  mocks.getBlock.mockResolvedValue({ number: 9n })
  mocks.removePending.mockResolvedValue(undefined)
  mocks.setBlocked.mockResolvedValue(undefined)
  mocks.notifyWithdrawal.mockResolvedValue({ Ingested: { finalized_head_block_number: 10n, withdrawal_id: new Uint8Array(32).fill(7) } })
  mocks.continueWithdrawal.mockResolvedValue({ Complete: { state: { Withdrawal: { Paid: null } } } })
})

afterEach(cleanup)

describe("SettlementConfirmationCoordinator", () => {
  it("keeps_a_successful_included_withdrawal_pending_until_the_Base_finalized_head_reaches_its_block", async () => {
    render(<SettlementConfirmationCoordinator />)

    await waitFor(() => expect(mocks.update).toHaveBeenCalledWith("withdraw:1", expect.objectContaining({ phase: "base-withdrawal-included", receiptBlockNumber: "10" })))
    expect(mocks.update).toHaveBeenCalledWith("withdraw:1", {
      phase: "base-withdrawal-finalizing",
      finalizedBlockNumber: "9",
    })
    expect(mocks.notifyWithdrawal).not.toHaveBeenCalled()
    expect(mocks.removePending).not.toHaveBeenCalled()
  })

  it("notifies_with_the_browser_identity_after_Base_finality_without_an_IC_wallet", async () => {
    mocks.getBlock.mockResolvedValue({ number: 10n })
    render(<SettlementConfirmationCoordinator />)

    await waitFor(() => expect(mocks.notifyWithdrawal).toHaveBeenCalledOnce())
    await waitFor(() => expect(mocks.continueWithdrawal).toHaveBeenCalledOnce())
    expect(mocks.continueWithdrawal).toHaveBeenCalledWith(new Uint8Array(32).fill(7))
    expect(mocks.update).toHaveBeenCalledWith("withdraw:1", {
      phase: "awaiting-ic-notification",
      finalizedBlockNumber: "10",
    })
    expect(mocks.update).toHaveBeenCalledWith("withdraw:1", {
      phase: "ic-notification-recorded",
      withdrawal: { owner: "aaaaa-aa", withdrawalId: `0x${"07".repeat(32)}` },
    })
    expect(mocks.setAction).not.toHaveBeenCalled()
    expect(mocks.update).not.toHaveBeenCalledWith("withdraw:1", expect.objectContaining({ phase: "complete" }))
  })

  it("accepts a duplicate notification receipt as recorded", async () => {
    mocks.getBlock.mockResolvedValue({ number: 10n })
    mocks.notifyWithdrawal.mockResolvedValue({ Duplicate: { withdrawal_id: new Uint8Array(32).fill(8) } })

    render(<SettlementConfirmationCoordinator />)

    await waitFor(() => expect(mocks.update).toHaveBeenCalledWith("withdraw:1", {
      phase: "ic-notification-recorded",
      withdrawal: { owner: "aaaaa-aa", withdrawalId: `0x${"08".repeat(32)}` },
    }))
    expect(mocks.continueWithdrawal).toHaveBeenCalledOnce()
  })

  it("marks_the_transfer_complete_only_when_the_canister_withdrawal_reaches_Paid", async () => {
    mocks.readPending.mockReturnValue([])
    mocks.progress = {
      ...mocks.progress,
      phase: "ledger-payout",
      withdrawal: { owner: "aaaaa-aa", withdrawalId: `0x${"07".repeat(32)}` },
    }
    mocks.getWithdrawal.mockResolvedValue([{ state: { Paid: null } }])

    render(<SettlementConfirmationCoordinator />)

    await waitFor(() => expect(mocks.update).toHaveBeenCalledWith("withdraw:1", expect.objectContaining({ phase: "complete" })))
  })

  it("does not start a second observer when progress rerenders during receipt lookup", async () => {
    let resolveReceipt!: (receipt: { status: "success"; blockNumber: bigint }) => void
    mocks.getReceipt.mockReturnValue(new Promise((resolve) => { resolveReceipt = resolve }))
    mocks.getBlock.mockResolvedValue({ number: 10n })
    const view = render(<SettlementConfirmationCoordinator />)
    await waitFor(() => expect(mocks.getReceipt).toHaveBeenCalledOnce())

    mocks.progress = { ...mocks.progress, phase: "base-withdrawal-included" }
    view.rerender(<SettlementConfirmationCoordinator />)
    resolveReceipt({ status: "success", blockNumber: 10n })

    await waitFor(() => expect(mocks.notifyWithdrawal).toHaveBeenCalledOnce())
    expect(mocks.getReceipt).toHaveBeenCalledOnce()
  })

  it("browser_notification_remains_current_across_unrelated_rerenders", async () => {
    mocks.getBlock.mockResolvedValue({ number: 10n })
    let resolveNotification!: (value: { Ingested: { finalized_head_block_number: bigint; withdrawal_id: Uint8Array } }) => void
    mocks.notifyWithdrawal.mockReturnValue(new Promise((resolve) => { resolveNotification = resolve }))
    const view = render(<SettlementConfirmationCoordinator />)
    await waitFor(() => expect(mocks.notifyWithdrawal).toHaveBeenCalledOnce())

    mocks.progress = { ...mocks.progress, phase: "awaiting-ic-notification" }
    view.rerender(<SettlementConfirmationCoordinator />)
    resolveNotification({ Ingested: { finalized_head_block_number: 10n, withdrawal_id: new Uint8Array(32).fill(7) } })

    await waitFor(() => expect(mocks.removePending).toHaveBeenCalledOnce())
    expect(mocks.update).toHaveBeenCalledWith("withdraw:1", expect.objectContaining({ phase: "ic-notification-recorded" }))
    expect(mocks.toastInfo).toHaveBeenCalledOnce()
  })

  it("terminal_notification_failure_blocks_the_pending_observer_without_reopening_attention", async () => {
    mocks.getBlock.mockResolvedValue({ number: 10n })
    mocks.notifyWithdrawal.mockRejectedValue(new Error("Withdrawal identity conflict"))
    const view = render(<SettlementConfirmationCoordinator />)

    await waitFor(() => expect(mocks.setBlocked).toHaveBeenCalledWith(pending, true))
    expect(mocks.update).toHaveBeenCalledWith("withdraw:1", expect.objectContaining({ phase: "attention" }))

    mocks.progress = { ...mocks.progress, phase: "attention" }
    view.rerender(<SettlementConfirmationCoordinator />)
    document.dispatchEvent(new Event("visibilitychange"))
    await Promise.resolve()

    expect(mocks.getReceipt).toHaveBeenCalledOnce()
  })

  it("keeps retryable notification failures pending", async () => {
    mocks.getBlock.mockResolvedValue({ number: 10n })
    mocks.notifyWithdrawal.mockRejectedValue(new Error("Base RPC is unavailable"))

    render(<SettlementConfirmationCoordinator />)

    await waitFor(() => expect(mocks.notifyWithdrawal).toHaveBeenCalledOnce())
    expect(mocks.setBlocked).not.toHaveBeenCalled()
    expect(mocks.removePending).not.toHaveBeenCalled()
    expect(mocks.update).not.toHaveBeenCalledWith("withdraw:1", expect.objectContaining({ phase: "attention" }))
  })

  it("does not repeat a successful notification when pending storage cleanup fails", async () => {
    mocks.getBlock.mockResolvedValue({ number: 10n })
    mocks.removePending.mockRejectedValue(new Error("storage unavailable"))

    render(<SettlementConfirmationCoordinator />)
    await waitFor(() => expect(mocks.update).toHaveBeenCalledWith("withdraw:1", expect.objectContaining({ phase: "ic-notification-recorded" })))
    document.dispatchEvent(new Event("visibilitychange"))
    await Promise.resolve()

    expect(mocks.notifyWithdrawal).toHaveBeenCalledOnce()
    expect(mocks.continueWithdrawal).toHaveBeenCalledOnce()
    expect(mocks.removePending).toHaveBeenCalledOnce()
  })

  it("does_not_automatically_repeat_an_incomplete_payout_step", async () => {
    mocks.getBlock.mockResolvedValue({ number: 10n })
    mocks.continueWithdrawal.mockResolvedValue({ ReconciliationProgress: { state: { Withdrawal: { ReconciliationHold: { phase: { SearchByMemo: null } } } } } })

    render(<SettlementConfirmationCoordinator />)

    await waitFor(() => expect(mocks.update).toHaveBeenCalledWith("withdraw:1", expect.objectContaining({ phase: "attention" })))
    document.dispatchEvent(new Event("visibilitychange"))
    await Promise.resolve()
    expect(mocks.continueWithdrawal).toHaveBeenCalledOnce()
  })

  it.each(["complete", "attention"])("does not observe a %s active transfer", async (phase) => {
    mocks.progress = { ...mocks.progress, phase }

    render(<SettlementConfirmationCoordinator />)
    await Promise.resolve()

    expect(mocks.getReceipt).not.toHaveBeenCalled()
  })
})
