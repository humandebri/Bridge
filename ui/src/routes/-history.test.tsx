import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"
import { QueryClient, QueryClientProvider } from "@tanstack/react-query"
import { Principal } from "@icp-sdk/core/principal"
import type * as Wagmi from "wagmi"
import type { DepositView, SettlementStopReason, WithdrawalView } from "@/generated/bridge.did"
import { deploymentProfile } from "@/config/profile"
import type { ActivityItem } from "@/lib/activity-history"
import { DepositActivityRow, Route, type WithdrawalHistoryData } from "./history"

const mocks = vi.hoisted(() => ({
  actor: {
    list_deposit_ids: vi.fn(),
    list_nonterminal_deposit_refs: vi.fn(),
    get_deposit: vi.fn(),
    list_withdrawals: vi.fn(),
    get_withdrawals: vi.fn(),
  },
  block: vi.fn(),
  complete: vi.fn(),
  autoRefresh: undefined as (() => void) | undefined,
}))
vi.mock("wagmi", async (importOriginal) => ({
  ...(await importOriginal<typeof Wagmi>()),
  useAccount: () => ({ address: `0x${"03".repeat(20)}` }),
  useChainId: () => 8453,
}))
vi.mock("@/features/wallet/ic-wallet-provider", () => ({
  useIcWallet: () => ({ historyAccount: { owner: "aaaaa-aa" } }),
}))
vi.mock("@/features/status/use-status", () => ({
  useRuntimeValidation: () => ({}),
  useRuntimeHeartbeat: () => ({}),
}))
vi.mock("@/features/bridge/bridge-progress-provider", () => ({
  useBridgeProgress: () => ({ completeWithdrawal: mocks.complete }),
}))
vi.mock("@/lib/ic/bridge", () => ({ createBridgeActor: async () => mocks.actor }))
vi.mock("@/lib/base-transaction-observation", () => ({
  readBaseBlock: () => mocks.block(),
  readBaseReceipt: vi.fn(),
}))
vi.mock("@/lib/mint-observation", () => ({
  observeDeposit: async () => ({ status: "unsubmitted", finalized: false, recorded: false }),
}))
vi.mock("@/lib/activity-auto-refresh", () => ({
  useActivityAutoRefresh: (enabled: boolean, refresh: () => void) => {
    mocks.autoRefresh = enabled ? refresh : undefined
  },
}))

beforeEach(() => {
  vi.clearAllMocks()
  mocks.block.mockResolvedValue({ timestamp: 1_000n })
  mocks.actor.list_deposit_ids.mockResolvedValue({
    Ok: {
      deposit_ids: [new Uint8Array(32).fill(1)],
      next_cursor: [],
      oldest_available_cursor: [],
      history_truncated: false,
    },
  })
  mocks.actor.list_nonterminal_deposit_refs.mockResolvedValue({
    Ok: { deposits: [], next_cursor: [] },
  })
  mocks.actor.get_deposit.mockResolvedValue([depositItem({ AuthorizationExpired: null }).deposit])
  mocks.actor.list_withdrawals.mockResolvedValue({
    Ok: { items: [], next_cursor: [], history_truncated: false },
  })
})

afterEach(cleanup)

function renderHistory(
  client = new QueryClient({ defaultOptions: { queries: { retry: false, gcTime: 0 } } }),
) {
  const Page = Route.options.component!
  render(
    <QueryClientProvider client={client}>
      <Page />
    </QueryClientProvider>,
  )
  return client
}

describe("History refresh", () => {
  it.each(["automatic", "manual"])("refreshes finalized time through %s refresh", async (mode) => {
    renderHistory()
    expect(screen.queryByRole("textbox", { name: "Base transaction hash" })).not.toBeInTheDocument()
    expect(screen.queryByRole("button", { name: "Restore transaction" })).not.toBeInTheDocument()
    await screen.findByText("Waiting for Base finality")
    mocks.block.mockResolvedValue({ timestamp: 1_001n })
    if (mode === "manual") fireEvent.click(screen.getByRole("button", { name: "Refresh" }))
    else
      await act(async () => {
        mocks.autoRefresh!()
      })
    await screen.findByRole("button", { name: "Request refund" })
    expect(mocks.block).toHaveBeenCalledTimes(2)
  })

  it("refreshes an older pending payout without changing the pagination cursor", async () => {
    const view = (tag: number, paid: boolean): WithdrawalView => ({
      withdrawal_id: new Uint8Array(32).fill(tag),
      amount: 100n,
      amount_out: 90n,
      charged_service_fee: 10n,
      max_service_fee: 10n,
      ledger_fee: 1n,
      release_ledger_block_index: paid ? [7n] : [],
      state: paid ? { Paid: null } : { ReleasePending: null },
      last_settlement_stop_reason: [],
    })
    const latest = view(2, true)
    const older = view(1, false)
    const item = (record: WithdrawalView) => ({
      id: BigInt(
        `0x${Array.from(record.withdrawal_id, (n) => n.toString(16).padStart(2, "0")).join("")}`,
      ),
      createdAtNs: 1n,
      destinationAccount: { owner: "aaaaa-aa", subaccount: new Uint8Array(32) },
      canister: record,
    })
    const client = new QueryClient({ defaultOptions: { queries: { retry: false, gcTime: 0 } } })
    const key = [
      "withdraw-history",
      deploymentProfile.chainId,
      deploymentProfile.bridgeAddress,
      `0x${"03".repeat(20)}`,
    ]
    const cursor = new Uint8Array(40).fill(9)
    client.setQueryData(key, {
      items: [item(latest), item(older)],
      nextCursor: cursor,
      olderBoundaryNs: 1n,
    })
    mocks.actor.list_withdrawals.mockResolvedValue({
      Ok: {
        items: [
          {
            withdrawal: latest,
            observed_at_ns: 2n,
            owner: Principal.fromText("aaaaa-aa"),
            subaccount: new Uint8Array(32),
            transaction_hash: [],
          },
        ],
        next_cursor: [new Uint8Array(40).fill(2)],
      },
    })
    mocks.actor.get_withdrawals.mockResolvedValue({ Ok: [[view(1, true)]] })
    renderHistory(client)
    await waitFor(() =>
      expect(
        client
          .getQueryData<WithdrawalHistoryData>(key)
          ?.items.find((row) => row.id === item(older).id)?.canister?.state,
      ).toEqual({ Paid: null }),
    )
    expect(mocks.actor.get_withdrawals).toHaveBeenCalledWith([older.withdrawal_id])
    expect(client.getQueryData<WithdrawalHistoryData>(key)?.nextCursor).toEqual(cursor)
  })
})

describe("History finalized authorization deadline recovery", () => {
  it("holds_AuthorizationExpired_until_finalized_Base_time_passes_the_deadline", () => {
    expectRefundOnlyAfterDeadline({ AuthorizationExpired: null })
  })

  it("holds_AuthorizationWindowTooShort_until_finalized_Base_time_passes_the_deadline", () => {
    expectRefundOnlyAfterDeadline({ AuthorizationWindowTooShort: null })
  })
})

function expectRefundOnlyAfterDeadline(reason: SettlementStopReason): void {
  const item = depositItem(reason)
  const onRequestRefund = vi.fn(() => Promise.resolve())
  const props = {
    item,
    mintFinalization: "absent" as const,
    writesEnabled: true,
    actioningId: undefined,
    onRequestRefund,
    onContinue: vi.fn(() => Promise.resolve()),
  }
  const view = render(<DepositActivityRow {...props} finalizedBlockTimestamp={1_000n} />)

  expect(screen.getByText("Waiting for Base finality")).toBeInTheDocument()
  expect(screen.queryByRole("button", { name: "Request refund" })).not.toBeInTheDocument()
  expect(onRequestRefund).not.toHaveBeenCalled()

  view.rerender(<DepositActivityRow {...props} finalizedBlockTimestamp={1_001n} />)
  fireEvent.click(screen.getByRole("button", { name: "Request refund" }))
  expect(onRequestRefund).toHaveBeenCalledOnce()

  view.rerender(<DepositActivityRow {...props} processedWithoutReceipt />)
  expect(screen.getByText("Processed on Base")).toBeInTheDocument()
  expect(screen.queryByText("Ready to mint")).not.toBeInTheDocument()
  expect(screen.queryByText(/Restore.*History/)).not.toBeInTheDocument()
  if (!deploymentProfile.mintRecoveryUrl)
    expect(screen.getByText(/recovery without a saved transaction hash is not supported/)).toBeInTheDocument()
  expect(
    screen.getByText(
      deploymentProfile.mintRecoveryUrl ? "取引を自動検索中" : "Transaction confirmation unavailable",
    ),
  ).toBeInTheDocument()
}

function depositItem(
  reason: SettlementStopReason,
): Extract<ActivityItem, { direction: "to-base" }> {
  const deposit: DepositView = {
    base_recipient: new Uint8Array(20).fill(3),
    deposit_id: new Uint8Array(32).fill(1),
    quote: [{ net_amount: 90n, service_fee: 10n }],
    max_service_fee: 10n,
    funding_ledger_block_index: [1n],
    from_subaccount: [],
    last_settlement_stop_reason: [reason],
    created_at_ns: 1n,
    state: { AuthorizationPending: null },
    available_refund_amount: [100n],
    owner_sequence: 1n,
    mint_receipt: [],
    mint_authorization: [
      {
        finalized_block_number: 10n,
        signature: [],
        deposit_id: new Uint8Array(32).fill(1),
        issued_at_timestamp: 900n,
        domain_name: "KINIC Bridge",
        charged_service_fee: 10n,
        recipient: new Uint8Array(20).fill(3),
        domain_version: "1",
        authorization_epoch: 1n,
        max_service_fee: 10n,
        deadline: 1_000n,
        signature_dispatch_attempt: 1,
        chain_id: 84_532n,
        finalized_block_hash: new Uint8Array(32).fill(2),
        finalized_block_timestamp: 900n,
        verifying_contract: new Uint8Array(20).fill(4),
        digest: new Uint8Array(32).fill(5),
        gross_amount: 100n,
      },
    ],
    automatic_progress: [],
    gross_amount: 100n,
    refund: [],
  }
  return {
    key: "deposit:history-deadline",
    direction: "to-base",
    createdAtNs: deposit.created_at_ns,
    deposit,
  }
}
