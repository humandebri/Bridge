import { cleanup, render, waitFor } from "@testing-library/react"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"

const mocks = vi.hoisted(() => ({
  notifyWithdrawal: vi.fn(),
  continueWithdrawal: vi.fn(),
  getReceipt: vi.fn(),
  getBlock: vi.fn(),
  getWithdrawal: vi.fn(),
  update: vi.fn(),
  completeWithdrawalProgress: vi.fn(),
  setAction: vi.fn(),
  progress: undefined as undefined | Record<string, unknown>,
}))

vi.mock("@/features/bridge/bridge-progress-provider", () => ({
  useBridgeProgress: () => ({ progress: mocks.progress, update: mocks.update, setAction: mocks.setAction, completeWithdrawal: mocks.completeWithdrawalProgress }),
}))
vi.mock("@/lib/evm/client", () => ({
  basePublicClient: { getTransactionReceipt: mocks.getReceipt, getBlock: mocks.getBlock },
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
vi.mock("sonner", () => ({ toast: { info: vi.fn(), warning: vi.fn(), error: vi.fn(), success: vi.fn() } }))
vi.mock("@/config/profile", () => ({
  deploymentProfile: {
    chainId: 84_532,
    bridgeAddress: "0x1111111111111111111111111111111111111111",
    icHost: "https://ic.example",
    bridgeCanisterId: "aaaaa-aa",
  },
}))

import {
  markPendingConfirmationNotified,
  readPendingConfirmations,
  removePendingConfirmation,
  savePendingConfirmation,
} from "@/lib/pending-confirmations"
import { SettlementConfirmationCoordinator } from "../settlement-confirmation-coordinator"

const transactionHash = `0x${"33".repeat(32)}` as const
const withdrawalId = `0x${"07".repeat(32)}` as const
const pending = { kind: "withdrawal" as const, transactionHash, owner: "aaaaa-aa" }

async function clearRecoveryQueue() {
  for (const entry of readPendingConfirmations()) await removePendingConfirmation(entry)
  window.localStorage.clear()
}

beforeEach(async () => {
  await clearRecoveryQueue()
  vi.clearAllMocks()
  mocks.progress = {
    id: "withdraw:recovery",
    direction: "withdraw",
    phase: "base-withdrawal-submitted",
    transactionHash,
    receiveAmount: "1.5",
    receiveSymbol: "TICRC1",
    destination: "aaaaa-aa",
  }
  mocks.getReceipt.mockResolvedValue({ status: "success", blockNumber: 10n })
  mocks.getBlock.mockResolvedValue({ number: 10n })
  mocks.notifyWithdrawal.mockResolvedValue({
    Ingested: { finalized_checkpoint_block_number: 10n, withdrawal_id: new Uint8Array(32).fill(7) },
  })
  mocks.continueWithdrawal.mockResolvedValue({ Complete: { state: { Withdrawal: { Paid: null } } } })
  mocks.getWithdrawal.mockResolvedValue([{ state: { ReleasePending: null } }])
})

afterEach(async () => {
  cleanup()
  await clearRecoveryQueue()
})

describe("withdrawal interruption recovery", () => {
  it("reloads_a_durably_notified_withdrawal_without_repeating_notification", async () => {
    mocks.continueWithdrawal.mockResolvedValue({ Deferred: { state: { Withdrawal: { ReleasePending: null } } } })
    await savePendingConfirmation(pending)
    const first = render(<SettlementConfirmationCoordinator />)

    await waitFor(() => expect(readPendingConfirmations()[0]?.notification).toEqual({
      status: "notified",
      withdrawalId,
    }))
    expect(mocks.notifyWithdrawal).toHaveBeenCalledOnce()
    expect(mocks.continueWithdrawal).toHaveBeenCalledOnce()

    first.unmount()
    mocks.progress = { ...mocks.progress, phase: "base-withdrawal-submitted" }
    render(<SettlementConfirmationCoordinator />)

    await waitFor(() => expect(mocks.getWithdrawal).toHaveBeenCalled())
    expect(mocks.notifyWithdrawal).toHaveBeenCalledOnce()
    expect(mocks.continueWithdrawal).toHaveBeenCalledOnce()
    expect(mocks.update).toHaveBeenCalledWith("withdraw:recovery", {
      phase: "ledger-payout",
      withdrawal: { owner: "aaaaa-aa", withdrawalId },
    })
    expect(readPendingConfirmations()).toHaveLength(1)
  })

  it("keeps_a_storage_interrupted_notification_in_session_across_remount", async () => {
    await savePendingConfirmation(pending)
    const originalSetItem = window.localStorage.setItem.bind(window.localStorage)
    const setItem = vi.spyOn(Storage.prototype, "setItem").mockImplementation((key, value) => {
      if (key.startsWith("kinic.bridge.pending-confirmations")) throw new Error("storage unavailable")
      return originalSetItem(key, value)
    })

    try {
      const first = render(<SettlementConfirmationCoordinator />)
      await waitFor(() => expect(readPendingConfirmations()[0]?.notification).toEqual({
        status: "notified",
        withdrawalId,
      }))
      expect(mocks.continueWithdrawal).not.toHaveBeenCalled()

      first.unmount()
      render(<SettlementConfirmationCoordinator />)
      await waitFor(() => expect(mocks.getWithdrawal).toHaveBeenCalled())

      expect(mocks.notifyWithdrawal).toHaveBeenCalledOnce()
      expect(mocks.continueWithdrawal).not.toHaveBeenCalled()
      expect(readPendingConfirmations()).toHaveLength(1)
    } finally {
      setItem.mockRestore()
    }
  })

  it("keeps_a_notified_withdrawal_during_query_failure_then_completes_after_remount", async () => {
    await savePendingConfirmation(pending)
    await markPendingConfirmationNotified(pending, withdrawalId)
    mocks.getWithdrawal.mockRejectedValueOnce(new Error("IC query unavailable"))

    const first = render(<SettlementConfirmationCoordinator />)
    await waitFor(() => expect(mocks.getWithdrawal).toHaveBeenCalledOnce())
    expect(mocks.update).not.toHaveBeenCalledWith("withdraw:recovery", expect.objectContaining({ phase: "complete" }))
    expect(readPendingConfirmations()).toHaveLength(1)

    first.unmount()
    mocks.getWithdrawal.mockResolvedValue([{ state: { Paid: null } }])
    render(<SettlementConfirmationCoordinator />)

    await waitFor(() => expect(mocks.completeWithdrawalProgress).toHaveBeenCalledWith({
      transactionHash,
      owner: "aaaaa-aa",
      withdrawalId,
    }))
    await waitFor(() => expect(readPendingConfirmations()).toEqual([]))
    expect(mocks.notifyWithdrawal).not.toHaveBeenCalled()
  })

  it("removes_only_a_reverted_Base_transaction_without_notifying", async () => {
    await savePendingConfirmation(pending)
    mocks.getReceipt.mockResolvedValue({ status: "reverted", blockNumber: 10n })

    render(<SettlementConfirmationCoordinator />)

    await waitFor(() => expect(readPendingConfirmations()).toEqual([]))
    expect(mocks.notifyWithdrawal).not.toHaveBeenCalled()
    expect(mocks.continueWithdrawal).not.toHaveBeenCalled()
    expect(mocks.update).toHaveBeenCalledWith("withdraw:recovery", expect.objectContaining({
      phase: "attention",
      receiptBlockNumber: "10",
    }))
  })
})
