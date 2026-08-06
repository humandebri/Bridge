import { cleanup, render, waitFor } from "@testing-library/react"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"

const mocks = vi.hoisted(() => ({
  adapter: {
    requiresUserGesture: false,
    notifyWithdrawal: vi.fn(),
    prepare: vi.fn(),
  },
  wallet: undefined as undefined | { account?: { owner: string }; adapter?: { requiresUserGesture: boolean; notifyWithdrawal: ReturnType<typeof vi.fn>; prepare: ReturnType<typeof vi.fn> } },
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

vi.mock("@/features/wallet/ic-wallet-provider", () => ({
  useIcWallet: () => mocks.wallet,
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
vi.mock("@/lib/browser-lock", () => ({ withBrowserLock: (_name: string, action: () => unknown) => action() }))
vi.mock("@/lib/withdrawal-notification", () => ({
  withdrawalNotificationPresentation: () => ({ tone: "info", message: "recorded" }),
}))
vi.mock("sonner", () => ({ toast: { info: mocks.toastInfo, warning: mocks.toastWarning, error: mocks.toastError, success: vi.fn() } }))
vi.mock("@/config/profile", () => ({ deploymentProfile: { icHost: "https://ic.example", bridgeCanisterId: "aaaaa-aa" } }))

import { SettlementConfirmationCoordinator } from "./settlement-confirmation-coordinator"

const hash = `0x${"33".repeat(32)}` as const
const pending = { kind: "withdrawal", transactionHash: hash, owner: "aaaaa-aa", blocked: false }
type ProgressAction = { label: string; pending?: boolean; run: () => Promise<void> }

function progressActionCalls(): Array<[string, ProgressAction | undefined]> {
  return mocks.setAction.mock.calls as Array<[string, ProgressAction | undefined]>
}

beforeEach(() => {
  vi.clearAllMocks()
  mocks.adapter.requiresUserGesture = false
  mocks.wallet = { account: { owner: "aaaaa-aa" }, adapter: mocks.adapter }
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
  mocks.adapter.notifyWithdrawal.mockResolvedValue({ Ingested: { finalized_head_block_number: 10n, withdrawal_id: new Uint8Array(32).fill(7) } })
  mocks.adapter.prepare.mockResolvedValue(vi.fn().mockResolvedValue(undefined))
})

afterEach(cleanup)

describe("SettlementConfirmationCoordinator", () => {
  it("keeps_a_successful_included_withdrawal_pending_until_the_Base_finalized_head_reaches_its_block", async () => {
    render(<SettlementConfirmationCoordinator />)

    await waitFor(() => expect(mocks.update).toHaveBeenCalledWith("withdraw:1", expect.objectContaining({ phase: "base-withdrawal-included", receiptBlockNumber: "10" })))
    expect(mocks.update).toHaveBeenCalledWith("withdraw:1", { phase: "base-withdrawal-finalizing" })
    expect(mocks.adapter.notifyWithdrawal).not.toHaveBeenCalled()
    expect(mocks.removePending).not.toHaveBeenCalled()
  })

  it("records the IC withdrawal identity after Base finality without declaring payout complete", async () => {
    mocks.getBlock.mockResolvedValue({ number: 10n })
    render(<SettlementConfirmationCoordinator />)

    await waitFor(() => expect(mocks.adapter.notifyWithdrawal).toHaveBeenCalled())
    expect(mocks.update).toHaveBeenCalledWith("withdraw:1", { phase: "awaiting-ic-notification" })
    expect(mocks.update).toHaveBeenCalledWith("withdraw:1", {
      phase: "ic-notification-recorded",
      withdrawal: { owner: "aaaaa-aa", withdrawalId: `0x${"07".repeat(32)}` },
    })
    expect(mocks.update).not.toHaveBeenCalledWith("withdraw:1", expect.objectContaining({ phase: "complete" }))
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

  it("does not start a second notification observer when progress rerenders during receipt lookup", async () => {
    let resolveReceipt!: (receipt: { status: "success"; blockNumber: bigint }) => void
    mocks.getReceipt.mockReturnValue(new Promise((resolve) => { resolveReceipt = resolve }))
    mocks.getBlock.mockResolvedValue({ number: 10n })
    const view = render(<SettlementConfirmationCoordinator />)
    await waitFor(() => expect(mocks.getReceipt).toHaveBeenCalledOnce())

    mocks.progress = { ...mocks.progress, phase: "base-withdrawal-included" }
    view.rerender(<SettlementConfirmationCoordinator />)
    resolveReceipt({ status: "success", blockNumber: 10n })

    await waitFor(() => expect(mocks.adapter.notifyWithdrawal).toHaveBeenCalledOnce())
    expect(mocks.getReceipt).toHaveBeenCalledOnce()
  })

  it("clears its owned wallet action when the wallet disconnects", async () => {
    mocks.adapter.requiresUserGesture = true
    mocks.getBlock.mockResolvedValue({ number: 10n })
    const view = render(<SettlementConfirmationCoordinator />)
    await waitFor(() => expect(mocks.setAction).toHaveBeenCalledWith("withdraw:1", expect.objectContaining({
      label: "Confirm with IC wallet",
    })))

    mocks.wallet = {}
    view.rerender(<SettlementConfirmationCoordinator />)

    expect(mocks.setAction).toHaveBeenLastCalledWith("withdraw:1", undefined)
  })

  it("terminal_notification_failure_blocks_the_pending_observer_without_reopening_attention", async () => {
    mocks.adapter.requiresUserGesture = true
    mocks.getBlock.mockResolvedValue({ number: 10n })
    mocks.adapter.notifyWithdrawal.mockRejectedValue(new Error("Withdrawal identity conflict"))
    const view = render(<SettlementConfirmationCoordinator />)
    await waitFor(() => expect(mocks.setAction).toHaveBeenCalledWith("withdraw:1", expect.objectContaining({
      label: "Confirm with IC wallet",
      pending: false,
    })))
    const registered = progressActionCalls().find(([, action]) => action?.label === "Confirm with IC wallet" && action.pending === false)?.[1]
    expect(registered).toBeDefined()

    await registered!.run()

    await waitFor(() => expect(mocks.setBlocked).toHaveBeenCalledWith(pending, true))
    expect(mocks.update).toHaveBeenCalledWith("withdraw:1", expect.objectContaining({ phase: "attention" }))
    expect(mocks.setAction).toHaveBeenCalledWith("withdraw:1", undefined)
    const actionRegistrations = progressActionCalls().filter(([, action]) => action?.label === "Confirm with IC wallet").length
    mocks.progress = { ...mocks.progress, phase: "attention" }
    view.rerender(<SettlementConfirmationCoordinator />)
    document.dispatchEvent(new Event("visibilitychange"))
    await Promise.resolve()

    expect(mocks.getReceipt).toHaveBeenCalledOnce()
    expect(progressActionCalls().filter(([, action]) => action?.label === "Confirm with IC wallet")).toHaveLength(actionRegistrations)
  })

  it("wallet_generation_change_suppresses_stale_progress_updates_after_notification", async () => {
    mocks.getBlock.mockResolvedValue({ number: 10n })
    let resolveNotification!: (value: { Ingested: { finalized_head_block_number: bigint; withdrawal_id: Uint8Array } }) => void
    mocks.adapter.notifyWithdrawal.mockReturnValue(new Promise((resolve) => { resolveNotification = resolve }))
    const view = render(<SettlementConfirmationCoordinator />)
    await waitFor(() => expect(mocks.adapter.notifyWithdrawal).toHaveBeenCalledOnce())

    mocks.wallet = {}
    view.rerender(<SettlementConfirmationCoordinator />)
    resolveNotification({ Ingested: { finalized_head_block_number: 10n, withdrawal_id: new Uint8Array(32).fill(7) } })

    await waitFor(() => expect(mocks.removePending).toHaveBeenCalledOnce())
    expect(mocks.adapter.notifyWithdrawal).toHaveBeenCalledOnce()
    expect(mocks.update).not.toHaveBeenCalledWith("withdraw:1", expect.objectContaining({ phase: "ic-notification-recorded" }))
    expect(mocks.toastInfo).not.toHaveBeenCalled()
  })

  it("does not repeat a successful notification when pending storage cleanup fails", async () => {
    mocks.getBlock.mockResolvedValue({ number: 10n })
    mocks.removePending.mockRejectedValue(new Error("storage unavailable"))

    render(<SettlementConfirmationCoordinator />)
    await waitFor(() => expect(mocks.update).toHaveBeenCalledWith("withdraw:1", expect.objectContaining({ phase: "ic-notification-recorded" })))
    document.dispatchEvent(new Event("visibilitychange"))
    await Promise.resolve()

    expect(mocks.adapter.notifyWithdrawal).toHaveBeenCalledOnce()
    expect(mocks.removePending).toHaveBeenCalledOnce()
  })

  it.each(["complete", "attention"])("does not observe a %s active transfer", async (phase) => {
    mocks.progress = { ...mocks.progress, phase }

    render(<SettlementConfirmationCoordinator />)
    await Promise.resolve()

    expect(mocks.getReceipt).not.toHaveBeenCalled()
  })
})
