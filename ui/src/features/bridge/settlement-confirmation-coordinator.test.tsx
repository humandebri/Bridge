vi.mock("wagmi", () => ({ useChainId: () => 8453 }))
vi.mock("@/features/status/use-status", () => ({
  useRuntimeValidation: () => ({ data: undefined, refetch: vi.fn() }),
  useRuntimeHeartbeat: () => ({ refetch: vi.fn() }),
}))
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
  receiptDetails: vi.fn(),
  getBlock: vi.fn(),
  revertQuorum: vi.fn(),
  readPending: vi.fn(),
  markNotified: vi.fn<(entry: PendingWithdrawal, withdrawalId: Hex) => Promise<void>>(),
  markAttempt:
    vi.fn<
      (
        entry: PendingWithdrawal,
        kind: NotificationAttemptKind,
        finalizedBlock: bigint,
      ) => Promise<void>
    >(),
  setFailure:
    vi.fn<(entry: PendingWithdrawal, failure: PendingNotificationFailure) => Promise<void>>(),
  removePending: vi.fn(),
  getWithdrawal: vi.fn(),
  update: vi.fn(),
  completeWithdrawalProgress: vi.fn(),
  setAction: vi.fn<(progressId: string, action?: TestProgressAction) => void>(),
  toastInfo: vi.fn(),
  toastWarning: vi.fn(),
  toastError: vi.fn(),
  progress: undefined as undefined | Record<string, unknown>,
  pendingEntries: [] as PendingWithdrawal[],
}))

vi.mock("@/lib/base-transaction-observation", async () => {
  const { basePublicClient } = await import("@/lib/evm/client")
  return {
    readBaseReceipt: (hash: `0x${string}`) => basePublicClient.getTransactionReceipt({ hash }),
    readBaseBlock: (block: bigint | "finalized" | "latest") =>
      basePublicClient.getBlock(
        typeof block === "bigint" ? { blockNumber: block } : { blockTag: block },
      ),
  }
})
vi.mock("@/lib/transaction-recovery", () => ({
  withdrawalReceiptDetails: mocks.receiptDetails,
  TransactionEvidenceMismatch: class TransactionEvidenceMismatch extends Error {},
}))

vi.mock("@/features/bridge/bridge-progress-provider", () => ({
  useBridgeProgress: () => ({
    progress: mocks.progress,
    update: updateWithoutSource,
    setAction: mocks.setAction,
    completeWithdrawal: mocks.completeWithdrawalProgress,
  }),
}))
vi.mock("@/lib/evm/client", () => ({
  basePublicClient: { getTransactionReceipt: mocks.getReceipt, getBlock: mocks.getBlock },
  hasIndependentFinalizedRevertQuorum: mocks.revertQuorum,
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
    constructor(
      readonly code: string,
      message: string,
    ) {
      super(message)
    }
  },
  continueWithdrawalWithBrowserIdentity: mocks.continueWithdrawal,
  notifyWithdrawalWithBrowserIdentity: mocks.notifyWithdrawal,
}))
vi.mock("@/lib/withdrawal-notification", () => ({
  withdrawalNotificationPresentation: () => ({ tone: "info", message: "recorded" }),
}))
vi.mock("sonner", () => ({
  toast: {
    info: mocks.toastInfo,
    warning: mocks.toastWarning,
    error: mocks.toastError,
    success: vi.fn(),
  },
}))
vi.mock("@/config/profile", () => ({
  deploymentProfile: { icHost: "https://ic.example", bridgeCanisterId: "aaaaa-aa" },
}))

import { TransactionEvidenceMismatch } from "@/lib/transaction-recovery"
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
  mocks.receiptDetails.mockResolvedValue({ id: new Uint8Array(32).fill(7) })
  mocks.revertQuorum.mockResolvedValue(true)
  mocks.removePending.mockResolvedValue(undefined)
  mocks.markNotified.mockImplementation(
    (entry: PendingWithdrawal, withdrawalId: Hex): Promise<void> => {
      mocks.pendingEntries = mocks.pendingEntries.map((candidate) =>
        candidate.transactionHash === entry.transactionHash
          ? { ...candidate, notification: { status: "notified", withdrawalId } }
          : candidate,
      )
      return Promise.resolve()
    },
  )
  mocks.markAttempt.mockImplementation(
    (
      entry: PendingWithdrawal,
      kind: NotificationAttemptKind,
      finalizedBlock: bigint,
    ): Promise<void> => {
      mocks.pendingEntries = mocks.pendingEntries.map((candidate) => {
        if (
          candidate.transactionHash !== entry.transactionHash ||
          candidate.notification.status !== "awaiting-notification"
        )
          return candidate
        return {
          ...candidate,
          notification: {
            ...candidate.notification,
            automaticAttemptUsed:
              candidate.notification.automaticAttemptUsed || kind === "automatic",
            shortRetryUsed:
              kind === "manual"
                ? false
                : candidate.notification.shortRetryUsed || kind === "short-retry",
            finalityReadvanceUsed:
              candidate.notification.finalityReadvanceUsed || kind === "finality-readvance",
            lastAttemptedFinalizedBlock: finalizedBlock.toString(),
            failure: undefined,
          },
        }
      })
      return Promise.resolve()
    },
  )
  mocks.setFailure.mockImplementation(
    (entry: PendingWithdrawal, failure: PendingNotificationFailure): Promise<void> => {
      mocks.pendingEntries = mocks.pendingEntries.map((candidate) =>
        candidate.transactionHash === entry.transactionHash
          ? { ...candidate, notification: { ...candidate.notification, failure } }
          : candidate,
      )
      return Promise.resolve()
    },
  )
  mocks.notifyWithdrawal.mockResolvedValue({
    Ingested: { finalized_checkpoint_block_number: 10n, withdrawal_id: new Uint8Array(32).fill(7) },
  })
  mocks.continueWithdrawal.mockResolvedValue({
    Complete: { state: { Withdrawal: { Paid: null } } },
  })
})

afterEach(() => {
  cleanup()
  vi.restoreAllMocks()
  vi.useRealTimers()
})

describe("SettlementConfirmationCoordinator", () => {
  it("observes an included withdrawal while the browser tab is hidden", async () => {
    vi.spyOn(document, "visibilityState", "get").mockReturnValue("hidden")
    render(<SettlementConfirmationCoordinator />)
    await waitFor(() =>
      expect(mocks.update).toHaveBeenCalledWith(
        "withdraw:1",
        expect.objectContaining({ phase: "base-withdrawal-included" }),
      ),
    )
    expect(mocks.notifyWithdrawal).not.toHaveBeenCalled()
  })

  it("keeps_a_successful_included_withdrawal_pending_until_the_Base_finalized_head_reaches_its_block", async () => {
    render(<SettlementConfirmationCoordinator />)

    await waitFor(() =>
      expect(mocks.update).toHaveBeenCalledWith(
        "withdraw:1",
        expect.objectContaining({ phase: "base-withdrawal-included", receiptBlockNumber: "10" }),
      ),
    )
    expect(mocks.update).toHaveBeenCalledWith("withdraw:1", {
      phase: "base-withdrawal-finalizing",
      finalizedBlockNumber: "9",
    })
    expect(mocks.notifyWithdrawal).not.toHaveBeenCalled()
    expect(mocks.removePending).not.toHaveBeenCalled()

    mocks.update.mockClear()
    mocks.getReceipt.mockRejectedValue(new Error("RPC unavailable"))
    act(() => {
      document.dispatchEvent(new Event("visibilitychange"))
    })
    await waitFor(() => expect(mocks.getReceipt).toHaveBeenCalledTimes(2))
    expect(mocks.update).toHaveBeenCalledWith("withdraw:1", {
      observationError: "Transfer status could not be refreshed. Retrying.",
    })

    const missing = new Error("Receipt missing")
    missing.name = "TransactionReceiptNotFoundError"
    mocks.getReceipt.mockRejectedValue(missing)
    act(() => {
      document.dispatchEvent(new Event("visibilitychange"))
    })
    await waitFor(() =>
      expect(mocks.update).toHaveBeenCalledWith("withdraw:1", {
        phase: "base-withdrawal-submitted",
        baseTransactionOutcome: undefined,
        receiptBlockNumber: undefined,
      }),
    )
    expect(mocks.notifyWithdrawal).not.toHaveBeenCalled()
    expect(mocks.removePending).not.toHaveBeenCalled()
  })

  it("does_not_notify_a_success_receipt_from_a_noncanonical_fork", async () => {
    mocks.getBlock.mockImplementation(({ blockTag }: { blockTag?: string }) =>
      Promise.resolve(
        blockTag === "finalized"
          ? { number: 11n, hash: `0x${"55".repeat(32)}` }
          : { number: 10n, hash: `0x${"66".repeat(32)}` },
      ),
    )

    render(<SettlementConfirmationCoordinator />)

    await waitFor(() => expect(mocks.getBlock).toHaveBeenCalledTimes(2))
    expect(mocks.notifyWithdrawal).not.toHaveBeenCalled()
    expect(mocks.removePending).not.toHaveBeenCalled()
  })

  it("keeps_a_reverted_receipt_pending_until_its_block_is_finalized", async () => {
    mocks.getReceipt.mockResolvedValue({ status: "reverted", blockNumber: 10n, blockHash })
    mocks.getBlock.mockResolvedValue({ number: 9n, hash: `0x${"55".repeat(32)}` })
    render(<SettlementConfirmationCoordinator />)
    await waitFor(() =>
      expect(mocks.update).toHaveBeenCalledWith(
        "withdraw:1",
        expect.objectContaining({ phase: "base-withdrawal-included", receiptBlockNumber: "10" }),
      ),
    )
    expect(mocks.removePending).not.toHaveBeenCalled()
    expect(mocks.notifyWithdrawal).not.toHaveBeenCalled()
  })

  it("discards_a_reverted_receipt_only_after_finality_and_canonicality", async () => {
    mocks.getReceipt.mockResolvedValue({ status: "reverted", blockNumber: 10n, blockHash })
    mocks.getBlock.mockResolvedValue({ number: 10n, hash: blockHash })

    render(<SettlementConfirmationCoordinator />)

    await waitFor(() => expect(mocks.removePending).toHaveBeenCalledWith(pending))
    expect(mocks.update).toHaveBeenCalledWith(
      "withdraw:1",
      expect.objectContaining({ phase: "attention", receiptBlockNumber: "10" }),
    )
    expect(mocks.notifyWithdrawal).not.toHaveBeenCalled()
  })

  it("keeps_a_single_provider_revert_when_independent_quorum_is_missing", async () => {
    mocks.getReceipt.mockResolvedValue({ status: "reverted", blockNumber: 10n, blockHash })
    mocks.getBlock.mockResolvedValue({ number: 10n, hash: blockHash })
    mocks.revertQuorum.mockResolvedValue(false)

    render(<SettlementConfirmationCoordinator />)

    await waitFor(() => expect(mocks.revertQuorum).toHaveBeenCalledWith(hash))
    expect(mocks.removePending).not.toHaveBeenCalled()
    expect(mocks.notifyWithdrawal).not.toHaveBeenCalled()
  })

  it("notifies_with_the_browser_identity_after_Base_finality_without_an_IC_wallet", async () => {
    mocks.getBlock.mockResolvedValue({ number: 10n, hash: blockHash })
    render(<SettlementConfirmationCoordinator />)

    await waitFor(() => expect(mocks.notifyWithdrawal).toHaveBeenCalledOnce())
    await waitFor(() => expect(mocks.continueWithdrawal).toHaveBeenCalledOnce())
    expect(mocks.continueWithdrawal).toHaveBeenCalledWith(new Uint8Array(32).fill(7))
    expect(mocks.markNotified).toHaveBeenCalledWith(pending, `0x${"07".repeat(32)}`)
    expect(mocks.markNotified.mock.invocationCallOrder[0]).toBeLessThan(
      mocks.continueWithdrawal.mock.invocationCallOrder[0]!,
    )
    expect(mocks.update).toHaveBeenCalledWith("withdraw:1", {
      phase: "awaiting-ic-notification",
      finalizedBlockNumber: "10",
    })
    expect(mocks.update).toHaveBeenCalledWith("withdraw:1", {
      phase: "ic-notification-recorded",
      withdrawal: { owner: "aaaaa-aa", withdrawalId: `0x${"07".repeat(32)}` },
    })
    expect(mocks.setAction).not.toHaveBeenCalled()
    expect(mocks.completeWithdrawalProgress).toHaveBeenCalledWith({
      transactionHash: hash,
      owner: "aaaaa-aa",
      withdrawalId: `0x${"07".repeat(32)}`,
    })
    expect(mocks.removePending).toHaveBeenCalledWith(pending)
  })

  it("accepts a duplicate notification receipt as recorded", async () => {
    mocks.getBlock.mockResolvedValue({ number: 10n, hash: blockHash })
    mocks.notifyWithdrawal.mockResolvedValue({
      Duplicate: { withdrawal_id: new Uint8Array(32).fill(8) },
    })

    render(<SettlementConfirmationCoordinator />)

    await waitFor(() =>
      expect(mocks.update).toHaveBeenCalledWith("withdraw:1", {
        phase: "ic-notification-recorded",
        withdrawal: { owner: "aaaaa-aa", withdrawalId: `0x${"08".repeat(32)}` },
      }),
    )
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

    await waitFor(() =>
      expect(mocks.completeWithdrawalProgress).toHaveBeenCalledWith({
        transactionHash: hash,
        owner: "aaaaa-aa",
        withdrawalId: notifiedPending.notification.withdrawalId,
      }),
    )
    expect(mocks.removePending).toHaveBeenCalledWith(notifiedPending)
    expect(mocks.notifyWithdrawal).not.toHaveBeenCalled()
  })

  it("does not start a second observer when progress rerenders during receipt lookup", async () => {
    let resolveReceipt!: (receipt: {
      status: "success"
      blockNumber: bigint
      blockHash: Hex
    }) => void
    mocks.getReceipt.mockReturnValue(
      new Promise((resolve) => {
        resolveReceipt = resolve
      }),
    )
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
    let resolveNotification!: (value: {
      Ingested: { finalized_checkpoint_block_number: bigint; withdrawal_id: Uint8Array }
    }) => void
    mocks.notifyWithdrawal.mockReturnValue(
      new Promise((resolve) => {
        resolveNotification = resolve
      }),
    )
    const view = render(<SettlementConfirmationCoordinator />)
    await waitFor(() => expect(mocks.notifyWithdrawal).toHaveBeenCalledOnce())

    mocks.progress = { ...mocks.progress, phase: "awaiting-ic-notification" }
    view.rerender(<SettlementConfirmationCoordinator />)
    resolveNotification({
      Ingested: {
        finalized_checkpoint_block_number: 10n,
        withdrawal_id: new Uint8Array(32).fill(7),
      },
    })

    await waitFor(() => expect(mocks.markNotified).toHaveBeenCalledOnce())
    expect(mocks.removePending).toHaveBeenCalledWith(pending)
    expect(mocks.update).toHaveBeenCalledWith(
      "withdraw:1",
      expect.objectContaining({ phase: "ic-notification-recorded" }),
    )
    expect(mocks.completeWithdrawalProgress).toHaveBeenCalledOnce()
    expect(mocks.toastInfo).toHaveBeenCalledOnce()
  })

  it("terminal_notification_failure_blocks_the_pending_observer_without_reopening_attention", async () => {
    mocks.getBlock.mockResolvedValue({ number: 10n, hash: blockHash })
    mocks.notifyWithdrawal.mockRejectedValue(
      new NotifyWithdrawalCallError(
        "WithdrawalBeforeAdmissionBoundary",
        "Withdrawal predates the admission boundary",
      ),
    )
    const view = render(<SettlementConfirmationCoordinator />)

    await waitFor(() => expect(mocks.setFailure).toHaveBeenCalled())
    const [failedEntry, failure] = mocks.setFailure.mock.calls[0]!
    expect(failedEntry.notification).toMatchObject({ automaticAttemptUsed: true })
    expect(failure.disposition).toBe("terminal")
    expect(mocks.update).toHaveBeenCalledWith(
      "withdraw:1",
      expect.objectContaining({ phase: "attention" }),
    )

    mocks.progress = { ...mocks.progress, phase: "attention" }
    view.rerender(<SettlementConfirmationCoordinator />)
    document.dispatchEvent(new Event("visibilitychange"))
    await Promise.resolve()

    expect(mocks.getReceipt).toHaveBeenCalledOnce()
  })

  it("stops automatic retries after an RPC availability failure", async () => {
    mocks.getBlock.mockResolvedValue({ number: 10n, hash: blockHash })
    mocks.notifyWithdrawal.mockRejectedValue(
      new NotifyWithdrawalCallError("RpcUnavailable", "Base RPC is unavailable"),
    )

    render(<SettlementConfirmationCoordinator />)

    await waitFor(() => expect(mocks.notifyWithdrawal).toHaveBeenCalledOnce())
    const [failedEntry, failure] = mocks.setFailure.mock.calls[0]!
    expect(failedEntry.notification).toMatchObject({ automaticAttemptUsed: true })
    expect(failure.disposition).toBe("manual-retry")
    expect(mocks.removePending).not.toHaveBeenCalled()
    expect(mocks.update).toHaveBeenCalledWith(
      "withdraw:1",
      expect.objectContaining({ phase: "attention" }),
    )
    expect(mocks.setAction).toHaveBeenCalledWith(
      "withdraw:1",
      expect.objectContaining({ label: "Retry IC notification" }),
    )
    document.dispatchEvent(new Event("visibilitychange"))
    await Promise.resolve()
    expect(mocks.notifyWithdrawal).toHaveBeenCalledOnce()
  })

  it("resumes_restored_notification_after_backoff", async () => {
    mocks.progress = { ...mocks.progress, phase: "attention" }
    mocks.pendingEntries = [
      {
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
      },
    ]

    render(<SettlementConfirmationCoordinator />)

    await waitFor(() =>
      expect(mocks.setAction).toHaveBeenCalledWith(
        "withdraw:1",
        expect.objectContaining({ label: "Retry IC notification" }),
      ),
    )
    expect(mocks.getReceipt).not.toHaveBeenCalled()
    expect(mocks.notifyWithdrawal).not.toHaveBeenCalled()
    mocks.getBlock.mockResolvedValue({ number: 10n, hash: blockHash })
    const clock = vi.spyOn(Date, "now").mockReturnValue(Date.now() + 31_000)
    try {
      document.dispatchEvent(new Event("visibilitychange"))
      await waitFor(() => expect(mocks.notifyWithdrawal).toHaveBeenCalledOnce())
    } finally {
      clock.mockRestore()
    }
  })

  it("runs_a_restored_manual_notification_retry_and_clears_the_action_on_success", async () => {
    mocks.progress = { ...mocks.progress, phase: "attention" }
    mocks.getBlock.mockResolvedValue({ number: 10n, hash: blockHash })
    mocks.pendingEntries = [
      {
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
      },
    ]
    render(<SettlementConfirmationCoordinator />)
    await waitFor(() =>
      expect(mocks.setAction).toHaveBeenCalledWith(
        "withdraw:1",
        expect.objectContaining({ label: "Retry IC notification" }),
      ),
    )
    const retry = mocks.setAction.mock.calls.find(
      ([, action]) => action?.label === "Retry IC notification",
    )?.[1]

    await act(async () => {
      await retry?.run()
    })

    expect(mocks.notifyWithdrawal).toHaveBeenCalledOnce()
    expect(mocks.setAction).toHaveBeenLastCalledWith("withdraw:1", undefined)
    expect(mocks.update).toHaveBeenCalledWith(
      "withdraw:1",
      expect.objectContaining({ phase: "ic-notification-recorded" }),
    )
    expect(mocks.completeWithdrawalProgress).toHaveBeenCalledWith({
      transactionHash: hash,
      owner: "aaaaa-aa",
      withdrawalId: `0x${"07".repeat(32)}`,
    })
  })

  it("reinstates_the_manual_notification_retry_after_an_explicit_retry_fails", async () => {
    mocks.progress = { ...mocks.progress, phase: "attention" }
    mocks.getBlock.mockResolvedValue({ number: 10n, hash: blockHash })
    mocks.notifyWithdrawal.mockRejectedValue(
      new NotifyWithdrawalCallError("RpcUnavailable", "Base RPC is unavailable"),
    )
    mocks.pendingEntries = [
      {
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
      },
    ]
    render(<SettlementConfirmationCoordinator />)
    await waitFor(() =>
      expect(mocks.setAction).toHaveBeenCalledWith(
        "withdraw:1",
        expect.objectContaining({ label: "Retry IC notification" }),
      ),
    )
    const retry = mocks.setAction.mock.calls.find(
      ([, action]) => action?.label === "Retry IC notification",
    )?.[1]
    mocks.setAction.mockClear()

    await act(async () => {
      await retry?.run()
    })

    expect(mocks.setAction).toHaveBeenNthCalledWith(1, "withdraw:1", undefined)
    expect(mocks.setAction).toHaveBeenLastCalledWith(
      "withdraw:1",
      expect.objectContaining({ label: "Retry IC notification" }),
    )
  })

  it("restores_a_terminal_notification_failure_without_a_retry_action", async () => {
    mocks.progress = { ...mocks.progress, phase: "attention" }
    mocks.pendingEntries = [
      {
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
      },
    ]

    render(<SettlementConfirmationCoordinator />)

    await waitFor(() =>
      expect(mocks.update).toHaveBeenCalledWith(
        "withdraw:1",
        expect.objectContaining({ phase: "attention" }),
      ),
    )
    expect(mocks.setAction).not.toHaveBeenCalledWith(
      "withdraw:1",
      expect.objectContaining({ label: "Retry IC notification" }),
    )
    expect(mocks.getReceipt).not.toHaveBeenCalled()
  })

  it("retries TransactionNotConfirmed only once after the finalized head advances", async () => {
    mocks.getBlock.mockResolvedValue({ number: 10n, hash: blockHash })
    mocks.notifyWithdrawal.mockRejectedValue(
      new NotifyWithdrawalCallError("TransactionNotConfirmed", "not finalized"),
    )

    render(<SettlementConfirmationCoordinator />)
    await waitFor(() =>
      expect(mocks.setFailure).toHaveBeenCalledWith(
        expect.anything(),
        expect.objectContaining({ disposition: "finality-wait" }),
      ),
    )
    await new Promise((resolve) => window.setTimeout(resolve, 0))
    expect(mocks.notifyWithdrawal).toHaveBeenCalledOnce()

    document.dispatchEvent(new Event("visibilitychange"))
    await waitFor(() => expect(mocks.getBlock).toHaveBeenCalledTimes(2))
    await new Promise((resolve) => window.setTimeout(resolve, 0))
    expect(mocks.notifyWithdrawal).toHaveBeenCalledOnce()

    mocks.getBlock.mockResolvedValue({ number: 11n, hash: blockHash })
    document.dispatchEvent(new Event("visibilitychange"))
    await waitFor(() => expect(mocks.notifyWithdrawal).toHaveBeenCalledTimes(2))
    await waitFor(() =>
      expect(mocks.setFailure).toHaveBeenLastCalledWith(
        expect.anything(),
        expect.objectContaining({ disposition: "manual-retry" }),
      ),
    )

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
        .mockResolvedValueOnce({
          Ingested: {
            finalized_checkpoint_block_number: 10n,
            withdrawal_id: new Uint8Array(32).fill(7),
          },
        })

      render(<SettlementConfirmationCoordinator />)
      await act(async () => {
        await vi.advanceTimersByTimeAsync(0)
      })
      expect(mocks.notifyWithdrawal).toHaveBeenCalledOnce()
      await act(async () => {
        await vi.advanceTimersByTimeAsync(4_999)
      })
      expect(mocks.notifyWithdrawal).toHaveBeenCalledOnce()
      await act(async () => {
        await vi.advanceTimersByTimeAsync(1)
      })
      expect(mocks.notifyWithdrawal).toHaveBeenCalledTimes(2)
      expect(mocks.markAttempt).toHaveBeenCalledWith(expect.anything(), "short-retry", 10n)
    } finally {
      vi.useRealTimers()
    }
  })

  it("removes a successful notification immediately when continuation reaches Paid", async () => {
    mocks.getBlock.mockResolvedValue({ number: 10n, hash: blockHash })

    render(<SettlementConfirmationCoordinator />)
    await waitFor(() =>
      expect(mocks.update).toHaveBeenCalledWith(
        "withdraw:1",
        expect.objectContaining({ phase: "ic-notification-recorded" }),
      ),
    )

    expect(mocks.notifyWithdrawal).toHaveBeenCalledOnce()
    expect(mocks.continueWithdrawal).toHaveBeenCalledOnce()
    expect(mocks.completeWithdrawalProgress).toHaveBeenCalledOnce()
    expect(mocks.removePending).toHaveBeenCalledWith(pending)
  })

  it("retains_a_notified_reconciliation_hold_for_explicit_recovery", async () => {
    mocks.readPending.mockReturnValue([notifiedPending])
    mocks.getWithdrawal.mockResolvedValue([{ state: { ReconciliationHold: null } }])

    render(<SettlementConfirmationCoordinator />)

    await waitFor(() =>
      expect(mocks.update).toHaveBeenCalledWith(
        "withdraw:1",
        expect.objectContaining({
          phase: "attention",
          withdrawal: {
            owner: "aaaaa-aa",
            withdrawalId: notifiedPending.notification.withdrawalId,
          },
        }),
      ),
    )
    expect(mocks.removePending).not.toHaveBeenCalled()
  })

  it("does_not_automatically_repeat_an_incomplete_payout_step", async () => {
    mocks.getBlock.mockResolvedValue({ number: 10n, hash: blockHash })
    mocks.continueWithdrawal.mockResolvedValue({
      ReconciliationProgress: {
        state: { Withdrawal: { ReconciliationHold: { phase: { SearchByMemo: null } } } },
      },
    })

    render(<SettlementConfirmationCoordinator />)

    await waitFor(() =>
      expect(mocks.update).toHaveBeenCalledWith(
        "withdraw:1",
        expect.objectContaining({ phase: "attention" }),
      ),
    )
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

const withdrawalId = notifiedPending.notification.withdrawalId
const icRecord = (state: Record<string, null>) => ({
  withdrawal_id: new Uint8Array(32).fill(7),
  state,
})
const wakeObserver = async () => {
  await act(async () => {
    document.dispatchEvent(new Event("visibilitychange"))
  })
}
const unqueueDisplayed = () => {
  mocks.pendingEntries = []
  mocks.progress = { ...mocks.progress, withdrawal: { owner: "aaaaa-aa", withdrawalId } }
}
const expectReadOnly = () => {
  expect(mocks.notifyWithdrawal).not.toHaveBeenCalled()
  expect(mocks.continueWithdrawal).not.toHaveBeenCalled()
  expect(mocks.markAttempt).not.toHaveBeenCalled()
  expect(mocks.markNotified).not.toHaveBeenCalled()
  expect(mocks.removePending).not.toHaveBeenCalled()
}

describe("displayed withdrawal observation after another tab removes the queue", () => {
  it("withdrawal_queue_removal_still_observes_paid", async () => {
    mocks.pendingEntries = [notifiedPending]
    mocks.getWithdrawal.mockResolvedValue([icRecord({ Pending: null })])
    const view = render(<SettlementConfirmationCoordinator />)
    await waitFor(() => expect(mocks.update).toHaveBeenCalled())
    unqueueDisplayed()
    view.rerender(<SettlementConfirmationCoordinator />)
    mocks.getWithdrawal.mockResolvedValue([icRecord({ Paid: null })])
    await wakeObserver()
    await waitFor(() =>
      expect(mocks.completeWithdrawalProgress).toHaveBeenCalledWith({
        transactionHash: hash,
        owner: "aaaaa-aa",
        withdrawalId,
      }),
    )
    expectReadOnly()
  })

  it("unqueued_withdrawal_recovers_validated_id_without_resending", async () => {
    mocks.pendingEntries = []
    mocks.getBlock.mockResolvedValue({ number: 10n, hash: blockHash })
    mocks.getWithdrawal.mockResolvedValue([icRecord({ Paid: null })])
    render(<SettlementConfirmationCoordinator />)
    await waitFor(() => expect(mocks.completeWithdrawalProgress).toHaveBeenCalled())
    expect(mocks.receiptDetails).toHaveBeenCalledWith(hash, "aaaaa-aa")
    expectReadOnly()
  })

  it("unqueued_withdrawal_errors_and_missing_records_never_complete", async () => {
    unqueueDisplayed()
    mocks.getWithdrawal
      .mockRejectedValueOnce(new Error("offline"))
      .mockResolvedValueOnce([])
      .mockResolvedValue([icRecord({ Paid: null })])
    render(<SettlementConfirmationCoordinator />)
    await waitFor(() =>
      expect(mocks.update).toHaveBeenCalledWith(
        "withdraw:1",
        expect.objectContaining({ observationError: expect.any(String) }),
      ),
    )
    expect(mocks.completeWithdrawalProgress).not.toHaveBeenCalled()
    await wakeObserver()
    expect(mocks.getWithdrawal).toHaveBeenCalledTimes(2)
    expect(mocks.completeWithdrawalProgress).not.toHaveBeenCalled()
    await wakeObserver()
    expect(mocks.completeWithdrawalProgress).toHaveBeenCalledTimes(1)
    expectReadOnly()
  })

  it("unqueued_withdrawal_hold_is_attention_and_mismatched_id_is_rejected", async () => {
    unqueueDisplayed()
    mocks.getWithdrawal
      .mockResolvedValueOnce([icRecord({ ReconciliationHold: null })])
      .mockResolvedValue([
        { ...icRecord({ Paid: null }), withdrawal_id: new Uint8Array(32).fill(8) },
      ])
    render(<SettlementConfirmationCoordinator />)
    await waitFor(() =>
      expect(mocks.update).toHaveBeenCalledWith(
        "withdraw:1",
        expect.objectContaining({
          phase: "attention",
          attentionMessage: "Payout needs reconciliation.",
        }),
      ),
    )
    await wakeObserver()
    expect(mocks.update).toHaveBeenLastCalledWith(
      "withdraw:1",
      expect.objectContaining({ issue: "conflict" }),
    )
    expect(mocks.completeWithdrawalProgress).not.toHaveBeenCalled()
    expectReadOnly()
  })

  it("unqueued_withdrawal_visibility_and_interval_share_inflight_read", async () => {
    vi.useFakeTimers()
    unqueueDisplayed()
    let resolve!: (value: unknown) => void
    mocks.getWithdrawal.mockReturnValue(
      new Promise((done) => {
        resolve = done
      }),
    )
    const visibility = vi.spyOn(document, "visibilityState", "get").mockReturnValue("visible")
    render(<SettlementConfirmationCoordinator />)
    await act(async () => {})
    visibility.mockReturnValue("hidden")
    await wakeObserver()
    await act(async () => {
      await vi.advanceTimersByTimeAsync(30_000)
    })
    visibility.mockReturnValue("visible")
    await wakeObserver()
    expect(mocks.getWithdrawal).toHaveBeenCalledTimes(1)
    await act(async () => {
      resolve([icRecord({ Paid: null })])
    })
    expect(mocks.completeWithdrawalProgress).toHaveBeenCalledTimes(1)
    expectReadOnly()
  })

  it("unqueued_withdrawal_stale_response_cannot_change_current_presentation", async () => {
    for (const replacement of [
      { transactionHash: `0x${"55".repeat(32)}` },
      { destination: "2vxsx-fae" },
      { withdrawal: { owner: "aaaaa-aa", withdrawalId: `0x${"08".repeat(32)}` } },
      { destination: "2vxsx-fae", withdrawal: { owner: "2vxsx-fae", withdrawalId } },
      { transfer: { generation: 2 } },
      undefined,
    ]) {
      unqueueDisplayed()
      mocks.progress = {
        ...mocks.progress,
        id: "withdraw:1",
        direction: "withdraw",
        phase: "ledger-payout",
        transactionHash: hash,
        destination: "aaaaa-aa",
        transfer: { generation: 1 },
      }
      let resolve!: (value: unknown) => void
      mocks.getWithdrawal.mockReturnValue(
        new Promise((done) => {
          resolve = done
        }),
      )
      const view = render(<SettlementConfirmationCoordinator />)
      await act(async () => {})
      mocks.progress = replacement ? { ...mocks.progress, ...replacement } : undefined
      view.rerender(<SettlementConfirmationCoordinator />)
      mocks.update.mockClear()
      await act(async () => {
        resolve([icRecord({ Paid: null })])
      })
      expect(mocks.update).not.toHaveBeenCalled()
      expect(mocks.completeWithdrawalProgress).not.toHaveBeenCalled()
      view.unmount()
    }
  })

  it("unqueued_withdrawal_unmount_ignores_inflight_paid", async () => {
    unqueueDisplayed()
    let resolve!: (value: unknown) => void
    mocks.getWithdrawal.mockReturnValue(
      new Promise((done) => {
        resolve = done
      }),
    )
    const view = render(<SettlementConfirmationCoordinator />)
    await act(async () => {})
    view.unmount()
    await act(async () => {
      resolve([icRecord({ Paid: null })])
    })
    expect(mocks.completeWithdrawalProgress).not.toHaveBeenCalled()
    expect(mocks.update).not.toHaveBeenCalled()
  })

  it("unqueued_withdrawal_revert_requires_finality_and_independent_quorum", async () => {
    mocks.pendingEntries = []
    mocks.getReceipt.mockResolvedValue({ status: "reverted", blockNumber: 10n, blockHash })
    render(<SettlementConfirmationCoordinator />)
    await waitFor(() => expect(mocks.update).toHaveBeenCalled())
    expect(mocks.revertQuorum).not.toHaveBeenCalled()
    mocks.getBlock.mockResolvedValue({ number: 10n, hash: blockHash })
    mocks.revertQuorum.mockResolvedValueOnce(false).mockResolvedValueOnce(true)
    await wakeObserver()
    expect(mocks.update).not.toHaveBeenCalledWith(
      "withdraw:1",
      expect.objectContaining({ outcome: "reverted" }),
    )
    await wakeObserver()
    expect(mocks.update).toHaveBeenCalledWith(
      "withdraw:1",
      expect.objectContaining({ outcome: "reverted" }),
    )
    expect(mocks.getWithdrawal).not.toHaveBeenCalled()
    expectReadOnly()
  })
})

it("unqueued_withdrawal_rejects_mismatched_transaction_or_destination_evidence", async () => {
  mocks.pendingEntries = []
  mocks.getBlock.mockResolvedValue({ number: 10n, hash: blockHash })
  mocks.receiptDetails.mockRejectedValue(
    new TransactionEvidenceMismatch("The withdrawal destination does not match this transfer."),
  )
  render(<SettlementConfirmationCoordinator />)
  await waitFor(() =>
    expect(mocks.update).toHaveBeenCalledWith(
      "withdraw:1",
      expect.objectContaining({ issue: "conflict" }),
    ),
  )
  expect(mocks.getWithdrawal).not.toHaveBeenCalled()
  expect(mocks.completeWithdrawalProgress).not.toHaveBeenCalled()
  expectReadOnly()
})

it("queued_withdrawal_late_paid_does_not_complete_replaced_or_dismissed_presentation", async () => {
  for (const replacement of [
    { destination: "2vxsx-fae", withdrawal: { owner: "2vxsx-fae", withdrawalId } },
    { transfer: { generation: 2 } },
    undefined,
  ]) {
    mocks.pendingEntries = [notifiedPending]
    mocks.progress = {
      id: "withdraw:1",
      direction: "withdraw",
      phase: "ledger-payout",
      transactionHash: hash,
      destination: "aaaaa-aa",
      transfer: { generation: 1 },
    }
    let resolve!: (value: unknown) => void
    mocks.getWithdrawal.mockReturnValue(
      new Promise((done) => {
        resolve = done
      }),
    )
    const view = render(<SettlementConfirmationCoordinator />)
    await act(async () => {})
    mocks.progress = replacement ? { ...mocks.progress, ...replacement } : undefined
    view.rerender(<SettlementConfirmationCoordinator />)
    await act(async () => {
      resolve([icRecord({ Paid: null })])
    })
    expect(mocks.completeWithdrawalProgress).not.toHaveBeenCalled()
    expect(mocks.update).not.toHaveBeenCalled()
    view.unmount()
  }
})

function updateWithoutSource(
  id: string,
  { observationSource: _source, ...patch }: Record<string, unknown>,
) {
  mocks.update(id, patch)
}

it("queued_withdrawal_late_evidence_still_reaches_terminal_conflict_reducer", async () => {
  mocks.pendingEntries = [notifiedPending]
  mocks.progress = {
    ...mocks.progress,
    phase: "attention",
    transfer: { generation: 1, outcome: "reverted" },
  }
  mocks.getWithdrawal.mockResolvedValue([icRecord({ Paid: null })])
  render(<SettlementConfirmationCoordinator />)
  await waitFor(() =>
    expect(mocks.completeWithdrawalProgress).toHaveBeenCalledWith({
      transactionHash: hash,
      owner: "aaaaa-aa",
      withdrawalId,
    }),
  )
})
