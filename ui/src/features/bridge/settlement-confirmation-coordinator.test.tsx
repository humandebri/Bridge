import { act, cleanup, render, waitFor } from "@testing-library/react"
import type { Hex } from "viem"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"
import type { PendingConfirmation, PendingNotificationFailure } from "@/lib/pending-confirmations"

type PendingWithdrawal = Extract<PendingConfirmation, { kind: "withdrawal" }>
type NotificationAttemptKind = "automatic" | "short-retry" | "finality-readvance" | "manual"
type TestProgressAction = { label: string; run: () => void | Promise<void> }

const mocks = vi.hoisted(() => ({
  notifyWithdrawal: vi.fn(),
  continueWithdrawal: vi.fn(),
  getReceipt: vi.fn(),
  getBlock: vi.fn(),
  readPending: vi.fn(),
  markNotified: vi.fn<(entry: PendingWithdrawal, withdrawalId: Hex) => Promise<void>>(),
  markAttempt: vi.fn<(entry: PendingWithdrawal, kind: NotificationAttemptKind, finalizedBlock: bigint) => Promise<void>>(),
  setFailure: vi.fn<(entry: PendingWithdrawal, failure: PendingNotificationFailure) => Promise<void>>(),
  removePending: vi.fn(),
  getWithdrawal: vi.fn(),
  update: vi.fn(),
  setAction: vi.fn<(progressId: string, action?: TestProgressAction) => void>(),
  toastInfo: vi.fn(),
  toastWarning: vi.fn(),
  toastError: vi.fn(),
  progress: undefined as undefined | Record<string, unknown>,
  pendingEntries: [] as PendingWithdrawal[],
}))

vi.mock("@/features/bridge/bridge-progress-provider", () => ({
  useBridgeProgress: () => ({ progress: mocks.progress, update: mocks.update, setAction: mocks.setAction }),
}))
vi.mock("@/lib/evm/client", () => ({
  basePublicClient: { getTransactionReceipt: mocks.getReceipt, getBlock: mocks.getBlock },
}))
vi.mock("@/lib/pending-confirmations", () => ({
  readPendingConfirmations: mocks.readPending,
  markPendingConfirmationNotified: mocks.markNotified,
  markPendingConfirmationNotificationAttempt: mocks.markAttempt,
  removePendingConfirmation: mocks.removePending,
  setPendingConfirmationNotificationFailure: mocks.setFailure,
}))
vi.mock("@/lib/ic/bridge", () => ({
  createBridgeActor: vi.fn().mockResolvedValue({ get_withdrawal: mocks.getWithdrawal }),
}))
vi.mock("@/lib/ic/withdrawal-notification-client", () => ({
  NotifyWithdrawalCallError: class NotifyWithdrawalCallError extends Error {
    constructor(readonly code: string, message: string) { super(message) }
  },
  continueWithdrawalWithBrowserIdentity: mocks.continueWithdrawal,
  notifyWithdrawalWithBrowserIdentity: mocks.notifyWithdrawal,
}))
vi.mock("@/lib/withdrawal-notification", () => ({
  withdrawalNotificationPresentation: () => ({ tone: "info", message: "recorded" }),
}))
vi.mock("sonner", () => ({ toast: { info: mocks.toastInfo, warning: mocks.toastWarning, error: mocks.toastError, success: vi.fn() } }))
vi.mock("@/config/profile", () => ({ deploymentProfile: { icHost: "https://ic.example", bridgeCanisterId: "aaaaa-aa" } }))

import { NotifyWithdrawalCallError } from "@/lib/ic/withdrawal-notification-client"
import { SettlementConfirmationCoordinator } from "./settlement-confirmation-coordinator"

const hash = `0x${"33".repeat(32)}` as const
const blockHash = `0x${"44".repeat(32)}` as const
const pending: PendingWithdrawal = {
  kind: "withdrawal",
  transactionHash: hash,
  owner: "aaaaa-aa",
  blocked: false,
  bridgeCanisterId: "aaaaa-aa",
  chainId: 8453,
  bridgeAddress: "0x1111111111111111111111111111111111111111",
  notification: {
    status: "awaiting-notification" as const,
    automaticAttemptUsed: false,
    shortRetryUsed: false,
    finalityReadvanceUsed: false,
  },
}
const notifiedPending = {
  ...pending,
  notification: { status: "notified" as const, withdrawalId: `0x${"07".repeat(32)}` as const },
}

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
  mocks.pendingEntries = [pending]
  mocks.readPending.mockImplementation(() => mocks.pendingEntries)
  mocks.getReceipt.mockResolvedValue({ status: "success", blockNumber: 10n, blockHash })
  mocks.getBlock.mockResolvedValue({ number: 9n, hash: blockHash })
  mocks.removePending.mockResolvedValue(undefined)
  mocks.markNotified.mockImplementation((entry: PendingWithdrawal, withdrawalId: Hex): Promise<void> => {
    mocks.pendingEntries = mocks.pendingEntries.map((candidate) => candidate.transactionHash === entry.transactionHash
      ? { ...candidate, notification: { status: "notified", withdrawalId } }
      : candidate)
    return Promise.resolve()
  })
  mocks.markAttempt.mockImplementation((entry: PendingWithdrawal, kind: NotificationAttemptKind, finalizedBlock: bigint): Promise<void> => {
    mocks.pendingEntries = mocks.pendingEntries.map((candidate) => {
      if (candidate.transactionHash !== entry.transactionHash || candidate.notification.status !== "awaiting-notification") return candidate
      return {
        ...candidate,
        notification: {
          ...candidate.notification,
          automaticAttemptUsed: candidate.notification.automaticAttemptUsed || kind === "automatic",
          shortRetryUsed: kind === "manual" ? false : candidate.notification.shortRetryUsed || kind === "short-retry",
          finalityReadvanceUsed: candidate.notification.finalityReadvanceUsed || kind === "finality-readvance",
          lastAttemptedFinalizedBlock: finalizedBlock.toString(),
          failure: undefined,
        },
      }
    })
    return Promise.resolve()
  })
  mocks.setFailure.mockImplementation((entry: PendingWithdrawal, failure: PendingNotificationFailure): Promise<void> => {
    mocks.pendingEntries = mocks.pendingEntries.map((candidate) => candidate.transactionHash === entry.transactionHash
      ? { ...candidate, notification: { ...candidate.notification, failure } }
      : candidate)
    return Promise.resolve()
  })
  mocks.notifyWithdrawal.mockResolvedValue({ Ingested: { finalized_checkpoint_block_number: 10n, withdrawal_id: new Uint8Array(32).fill(7) } })
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

  it("does_not_notify_a_success_receipt_from_a_noncanonical_fork", async () => {
    mocks.getBlock.mockImplementation(({ blockTag }: { blockTag?: string }) => Promise.resolve(
      blockTag === "finalized"
        ? { number: 11n, hash: `0x${"55".repeat(32)}` }
        : { number: 10n, hash: `0x${"66".repeat(32)}` },
    ))

    render(<SettlementConfirmationCoordinator />)

    await waitFor(() => expect(mocks.getBlock).toHaveBeenCalledTimes(2))
    expect(mocks.notifyWithdrawal).not.toHaveBeenCalled()
    expect(mocks.removePending).not.toHaveBeenCalled()
  })

  it("surfaces_a_reverted_receipt_before_finality_is_available", async () => {
    for (const finalized of [
      { number: 9n, hash: `0x${"55".repeat(32)}` },
      { number: null, hash: null },
      { number: 10n, hash: null },
    ]) {
      mocks.getReceipt.mockResolvedValue({ status: "reverted", blockNumber: 10n, blockHash })
      mocks.getBlock.mockResolvedValue(finalized)

      render(<SettlementConfirmationCoordinator />)

      await waitFor(() => expect(mocks.removePending).toHaveBeenCalledWith(pending))
      expect(mocks.update).toHaveBeenCalledWith(
        "withdraw:1",
        expect.objectContaining({ phase: "attention", receiptBlockNumber: "10" }),
      )
      expect(mocks.getBlock).not.toHaveBeenCalled()
      expect(mocks.notifyWithdrawal).not.toHaveBeenCalled()
      cleanup()
      vi.clearAllMocks()
      mocks.readPending.mockReturnValue([pending])
    }
  })

  it("notifies_with_the_browser_identity_after_Base_finality_without_an_IC_wallet", async () => {
    mocks.getBlock.mockResolvedValue({ number: 10n, hash: blockHash })
    render(<SettlementConfirmationCoordinator />)

    await waitFor(() => expect(mocks.notifyWithdrawal).toHaveBeenCalledOnce())
    await waitFor(() => expect(mocks.continueWithdrawal).toHaveBeenCalledOnce())
    expect(mocks.continueWithdrawal).toHaveBeenCalledWith(new Uint8Array(32).fill(7))
    expect(mocks.markNotified).toHaveBeenCalledWith(pending, `0x${"07".repeat(32)}`)
    expect(mocks.markNotified.mock.invocationCallOrder[0]).toBeLessThan(mocks.continueWithdrawal.mock.invocationCallOrder[0]!)
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
    mocks.getBlock.mockResolvedValue({ number: 10n, hash: blockHash })
    mocks.notifyWithdrawal.mockResolvedValue({ Duplicate: { withdrawal_id: new Uint8Array(32).fill(8) } })

    render(<SettlementConfirmationCoordinator />)

    await waitFor(() => expect(mocks.update).toHaveBeenCalledWith("withdraw:1", {
      phase: "ic-notification-recorded",
      withdrawal: { owner: "aaaaa-aa", withdrawalId: `0x${"08".repeat(32)}` },
    }))
    expect(mocks.continueWithdrawal).toHaveBeenCalledOnce()
  })

  it("marks_the_transfer_complete_only_when_the_canister_withdrawal_reaches_Paid", async () => {
    mocks.readPending.mockReturnValue([notifiedPending])
    mocks.progress = {
      ...mocks.progress,
      phase: "ledger-payout",
      withdrawal: { owner: "aaaaa-aa", withdrawalId: `0x${"07".repeat(32)}` },
    }
    mocks.getWithdrawal.mockResolvedValue([{ state: { Paid: null } }])

    render(<SettlementConfirmationCoordinator />)

    await waitFor(() => expect(mocks.update).toHaveBeenCalledWith("withdraw:1", expect.objectContaining({ phase: "complete" })))
    expect(mocks.removePending).toHaveBeenCalledWith(notifiedPending)
    expect(mocks.notifyWithdrawal).not.toHaveBeenCalled()
  })

  it("does not start a second observer when progress rerenders during receipt lookup", async () => {
    let resolveReceipt!: (receipt: { status: "success"; blockNumber: bigint; blockHash: Hex }) => void
    mocks.getReceipt.mockReturnValue(new Promise((resolve) => { resolveReceipt = resolve }))
    mocks.getBlock.mockResolvedValue({ number: 10n, hash: blockHash })
    const view = render(<SettlementConfirmationCoordinator />)
    await waitFor(() => expect(mocks.getReceipt).toHaveBeenCalledOnce())

    mocks.progress = { ...mocks.progress, phase: "base-withdrawal-included" }
    view.rerender(<SettlementConfirmationCoordinator />)
    resolveReceipt({ status: "success", blockNumber: 10n, blockHash })

    await waitFor(() => expect(mocks.notifyWithdrawal).toHaveBeenCalledOnce())
    expect(mocks.getReceipt).toHaveBeenCalledOnce()
  })

  it("browser_notification_remains_current_across_unrelated_rerenders", async () => {
    mocks.getBlock.mockResolvedValue({ number: 10n, hash: blockHash })
    let resolveNotification!: (value: { Ingested: { finalized_checkpoint_block_number: bigint; withdrawal_id: Uint8Array } }) => void
    mocks.notifyWithdrawal.mockReturnValue(new Promise((resolve) => { resolveNotification = resolve }))
    const view = render(<SettlementConfirmationCoordinator />)
    await waitFor(() => expect(mocks.notifyWithdrawal).toHaveBeenCalledOnce())

    mocks.progress = { ...mocks.progress, phase: "awaiting-ic-notification" }
    view.rerender(<SettlementConfirmationCoordinator />)
    resolveNotification({ Ingested: { finalized_checkpoint_block_number: 10n, withdrawal_id: new Uint8Array(32).fill(7) } })

    await waitFor(() => expect(mocks.markNotified).toHaveBeenCalledOnce())
    expect(mocks.removePending).not.toHaveBeenCalled()
    expect(mocks.update).toHaveBeenCalledWith("withdraw:1", expect.objectContaining({ phase: "ic-notification-recorded" }))
    expect(mocks.toastInfo).toHaveBeenCalledOnce()
  })

  it("terminal_notification_failure_blocks_the_pending_observer_without_reopening_attention", async () => {
    mocks.getBlock.mockResolvedValue({ number: 10n, hash: blockHash })
    mocks.notifyWithdrawal.mockRejectedValue(new NotifyWithdrawalCallError(
      "WithdrawalBeforeAdmissionBoundary",
      "Withdrawal predates the admission boundary",
    ))
    const view = render(<SettlementConfirmationCoordinator />)

    await waitFor(() => expect(mocks.setFailure).toHaveBeenCalled())
    const [failedEntry, failure] = mocks.setFailure.mock.calls[0]!
    expect(failedEntry.notification).toMatchObject({ automaticAttemptUsed: true })
    expect(failure.disposition).toBe("terminal")
    expect(mocks.update).toHaveBeenCalledWith("withdraw:1", expect.objectContaining({ phase: "attention" }))

    mocks.progress = { ...mocks.progress, phase: "attention" }
    view.rerender(<SettlementConfirmationCoordinator />)
    document.dispatchEvent(new Event("visibilitychange"))
    await Promise.resolve()

    expect(mocks.getReceipt).toHaveBeenCalledOnce()
  })

  it("stops automatic retries after an RPC availability failure", async () => {
    mocks.getBlock.mockResolvedValue({ number: 10n, hash: blockHash })
    mocks.notifyWithdrawal.mockRejectedValue(new NotifyWithdrawalCallError("RpcUnavailable", "Base RPC is unavailable"))

    render(<SettlementConfirmationCoordinator />)

    await waitFor(() => expect(mocks.notifyWithdrawal).toHaveBeenCalledOnce())
    const [failedEntry, failure] = mocks.setFailure.mock.calls[0]!
    expect(failedEntry.notification).toMatchObject({ automaticAttemptUsed: true })
    expect(failure.disposition).toBe("manual-retry")
    expect(mocks.removePending).not.toHaveBeenCalled()
    expect(mocks.update).toHaveBeenCalledWith("withdraw:1", expect.objectContaining({ phase: "attention" }))
    expect(mocks.setAction).toHaveBeenCalledWith("withdraw:1", expect.objectContaining({ label: "Retry IC notification" }))
    document.dispatchEvent(new Event("visibilitychange"))
    await Promise.resolve()
    expect(mocks.notifyWithdrawal).toHaveBeenCalledOnce()
  })

  it("restores_a_manual_notification_retry_without_automatic_RPC", async () => {
    mocks.progress = { ...mocks.progress, phase: "attention" }
    mocks.pendingEntries = [{
      ...pending,
      notification: {
        status: "awaiting-notification",
        automaticAttemptUsed: true,
        shortRetryUsed: false,
        finalityReadvanceUsed: false,
        failure: {
          code: "RpcUnavailable",
          message: "Base RPC is unavailable",
          disposition: "manual-retry",
        },
      },
    }]

    render(<SettlementConfirmationCoordinator />)

    await waitFor(() => expect(mocks.setAction).toHaveBeenCalledWith(
      "withdraw:1",
      expect.objectContaining({ label: "Retry IC notification" }),
    ))
    expect(mocks.getReceipt).not.toHaveBeenCalled()
    expect(mocks.notifyWithdrawal).not.toHaveBeenCalled()
  })

  it("runs_a_restored_manual_notification_retry_and_clears_the_action_on_success", async () => {
    mocks.progress = { ...mocks.progress, phase: "attention" }
    mocks.getBlock.mockResolvedValue({ number: 10n, hash: blockHash })
    mocks.pendingEntries = [{
      ...pending,
      notification: {
        status: "awaiting-notification",
        automaticAttemptUsed: true,
        shortRetryUsed: false,
        finalityReadvanceUsed: false,
        failure: {
          code: "RpcUnavailable",
          message: "Base RPC is unavailable",
          disposition: "manual-retry",
        },
      },
    }]
    render(<SettlementConfirmationCoordinator />)
    await waitFor(() => expect(mocks.setAction).toHaveBeenCalledWith(
      "withdraw:1",
      expect.objectContaining({ label: "Retry IC notification" }),
    ))
    const retry = mocks.setAction.mock.calls.find(([, action]) => action?.label === "Retry IC notification")?.[1]

    await act(async () => { await retry?.run() })

    expect(mocks.notifyWithdrawal).toHaveBeenCalledOnce()
    expect(mocks.setAction).toHaveBeenLastCalledWith("withdraw:1", undefined)
  })

  it("reinstates_the_manual_notification_retry_after_an_explicit_retry_fails", async () => {
    mocks.progress = { ...mocks.progress, phase: "attention" }
    mocks.getBlock.mockResolvedValue({ number: 10n, hash: blockHash })
    mocks.notifyWithdrawal.mockRejectedValue(new NotifyWithdrawalCallError("RpcUnavailable", "Base RPC is unavailable"))
    mocks.pendingEntries = [{
      ...pending,
      notification: {
        status: "awaiting-notification",
        automaticAttemptUsed: true,
        shortRetryUsed: false,
        finalityReadvanceUsed: false,
        failure: {
          code: "RpcUnavailable",
          message: "Base RPC is unavailable",
          disposition: "manual-retry",
        },
      },
    }]
    render(<SettlementConfirmationCoordinator />)
    await waitFor(() => expect(mocks.setAction).toHaveBeenCalledWith(
      "withdraw:1",
      expect.objectContaining({ label: "Retry IC notification" }),
    ))
    const retry = mocks.setAction.mock.calls.find(([, action]) => action?.label === "Retry IC notification")?.[1]
    mocks.setAction.mockClear()

    await act(async () => { await retry?.run() })

    expect(mocks.setAction).toHaveBeenNthCalledWith(1, "withdraw:1", undefined)
    expect(mocks.setAction).toHaveBeenLastCalledWith(
      "withdraw:1",
      expect.objectContaining({ label: "Retry IC notification" }),
    )
  })

  it("restores_a_terminal_notification_failure_without_a_retry_action", async () => {
    mocks.progress = { ...mocks.progress, phase: "attention" }
    mocks.pendingEntries = [{
      ...pending,
      notification: {
        status: "awaiting-notification",
        automaticAttemptUsed: true,
        shortRetryUsed: false,
        finalityReadvanceUsed: false,
        failure: {
          code: "WithdrawalConflict",
          message: "Withdrawal identity conflict",
          disposition: "terminal",
        },
      },
    }]

    render(<SettlementConfirmationCoordinator />)

    await waitFor(() => expect(mocks.update).toHaveBeenCalledWith(
      "withdraw:1",
      expect.objectContaining({ phase: "attention" }),
    ))
    expect(mocks.setAction).not.toHaveBeenCalledWith(
      "withdraw:1",
      expect.objectContaining({ label: "Retry IC notification" }),
    )
    expect(mocks.getReceipt).not.toHaveBeenCalled()
  })

  it("retries TransactionNotConfirmed only once after the finalized head advances", async () => {
    mocks.getBlock.mockResolvedValue({ number: 10n, hash: blockHash })
    mocks.notifyWithdrawal.mockRejectedValue(new NotifyWithdrawalCallError(
      "TransactionNotConfirmed",
      "not finalized",
    ))

    render(<SettlementConfirmationCoordinator />)
    await waitFor(() => expect(mocks.setFailure).toHaveBeenCalledWith(
      expect.anything(),
      expect.objectContaining({ disposition: "finality-wait" }),
    ))
    await new Promise((resolve) => window.setTimeout(resolve, 0))
    expect(mocks.notifyWithdrawal).toHaveBeenCalledOnce()

    document.dispatchEvent(new Event("visibilitychange"))
    await waitFor(() => expect(mocks.getBlock).toHaveBeenCalledTimes(2))
    await new Promise((resolve) => window.setTimeout(resolve, 0))
    expect(mocks.notifyWithdrawal).toHaveBeenCalledOnce()

    mocks.getBlock.mockResolvedValue({ number: 11n, hash: blockHash })
    document.dispatchEvent(new Event("visibilitychange"))
    await waitFor(() => expect(mocks.notifyWithdrawal).toHaveBeenCalledTimes(2))
    await waitFor(() => expect(mocks.setFailure).toHaveBeenLastCalledWith(
      expect.anything(),
      expect.objectContaining({ disposition: "manual-retry" }),
    ))

    mocks.getBlock.mockResolvedValue({ number: 12n, hash: blockHash })
    document.dispatchEvent(new Event("visibilitychange"))
    await Promise.resolve()
    expect(mocks.notifyWithdrawal).toHaveBeenCalledTimes(2)
  })

  it("uses one five-second retry for an interrupted UI-to-IC call", async () => {
    vi.useFakeTimers()
    try {
      mocks.getBlock.mockResolvedValue({ number: 10n, hash: blockHash })
      mocks.notifyWithdrawal
        .mockRejectedValueOnce(new Error("network disconnected"))
        .mockResolvedValueOnce({ Ingested: { finalized_checkpoint_block_number: 10n, withdrawal_id: new Uint8Array(32).fill(7) } })

      render(<SettlementConfirmationCoordinator />)
      await act(async () => { await vi.advanceTimersByTimeAsync(0) })
      expect(mocks.notifyWithdrawal).toHaveBeenCalledOnce()
      await act(async () => { await vi.advanceTimersByTimeAsync(4_999) })
      expect(mocks.notifyWithdrawal).toHaveBeenCalledOnce()
      await act(async () => { await vi.advanceTimersByTimeAsync(1) })
      expect(mocks.notifyWithdrawal).toHaveBeenCalledTimes(2)
      expect(mocks.markAttempt).toHaveBeenCalledWith(expect.anything(), "short-retry", 10n)
    } finally {
      vi.useRealTimers()
    }
  })

  it("retains a successful notification until Paid", async () => {
    mocks.getBlock.mockResolvedValue({ number: 10n, hash: blockHash })

    render(<SettlementConfirmationCoordinator />)
    await waitFor(() => expect(mocks.update).toHaveBeenCalledWith("withdraw:1", expect.objectContaining({ phase: "ic-notification-recorded" })))

    expect(mocks.notifyWithdrawal).toHaveBeenCalledOnce()
    expect(mocks.continueWithdrawal).toHaveBeenCalledOnce()
    expect(mocks.removePending).not.toHaveBeenCalled()
  })

  it("restores_a_notified_withdrawal_without_notifying_again", async () => {
    mocks.readPending.mockReturnValue([notifiedPending])
    mocks.getWithdrawal.mockResolvedValue([{ state: { ReleasePending: null } }])

    render(<SettlementConfirmationCoordinator />)

    await waitFor(() => expect(mocks.update).toHaveBeenCalledWith("withdraw:1", {
      phase: "ledger-payout",
      withdrawal: { owner: "aaaaa-aa", withdrawalId: notifiedPending.notification.withdrawalId },
    }))
    expect(mocks.notifyWithdrawal).not.toHaveBeenCalled()
    expect(mocks.removePending).not.toHaveBeenCalled()
  })

  it("retains_a_notified_reconciliation_hold_for_explicit_recovery", async () => {
    mocks.readPending.mockReturnValue([notifiedPending])
    mocks.getWithdrawal.mockResolvedValue([{ state: { ReconciliationHold: null } }])

    render(<SettlementConfirmationCoordinator />)

    await waitFor(() => expect(mocks.update).toHaveBeenCalledWith("withdraw:1", expect.objectContaining({
      phase: "attention",
      withdrawal: { owner: "aaaaa-aa", withdrawalId: notifiedPending.notification.withdrawalId },
    })))
    expect(mocks.removePending).not.toHaveBeenCalled()
  })

  it("does_not_automatically_repeat_an_incomplete_payout_step", async () => {
    mocks.getBlock.mockResolvedValue({ number: 10n, hash: blockHash })
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
