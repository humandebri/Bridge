import { cleanup, fireEvent, render, screen } from "@testing-library/react"
import { afterEach, describe, expect, it, vi } from "vitest"
import type { DepositView, SettlementStopReason } from "@/generated/bridge.did"
import type { ActivityItem } from "@/lib/activity-history"
import { DepositActivityRow } from "./history"

afterEach(cleanup)

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
