import { describe, expect, it } from "vitest"
import type { DepositView } from "@/generated/bridge.did"
import {
  authorizationDeadlineRefundStatus,
  depositContinuation,
  depositPhaseName,
  depositPhaseTone,
  depositReconciliationMessage,
  depositUsesPendingMintStatus,
  isDepositPhase,
  isDepositTerminal,
  isSettlementActionResult,
  isWithdrawalTerminal,
  settlementStateName,
  withdrawalPhaseName,
} from "./settlement-phase"

describe("settlement phase helpers", () => {
  it("preserves public display names and terminal tones", () => {
    expect(depositPhaseName({ Minted: null })).toBe("Complete")
    expect(depositPhaseTone({ Minted: null })).toBe("good")
    expect(depositPhaseName({ EscrowedUnquoted: null })).toBe("Checking Base")
    expect(depositPhaseName({ Refunded: null })).toBe("Refunded")
    expect(depositPhaseTone({ RefundProcessing: null })).toBe("warn")
    expect(isDepositTerminal({ Refunded: null })).toBe(true)
    expect(isDepositTerminal({ Cancelled: null })).toBe(true)
    expect(withdrawalPhaseName({ ReleasePending: null })).toBe("Processing")
    expect(isWithdrawalTerminal({ Paid: null })).toBe(true)
    expect(settlementStateName({ Withdrawal: { Paid: null } })).toBe("Paid")
  })

  it("rejects unknown and malformed runtime variants", () => {
    expect(isDepositPhase({ FuturePhase: null })).toBe(false)
    expect(isDepositPhase({ Minted: "not null" })).toBe(false)
    expect(isSettlementActionResult({ Complete: { state: { Deposit: { Minted: null } } } })).toBe(
      true,
    )
    expect(
      isSettlementActionResult({ Complete: { state: { Deposit: { FuturePhase: null } } } }),
    ).toBe(false)
    expect(
      isSettlementActionResult({ Complete: { state: { Deposit: { Minted: null } }, extra: true } }),
    ).toBe(false)
    expect(
      isSettlementActionResult({ Stopped: { state: { Withdrawal: { Observed: null } } } }),
    ).toBe(false)
    expect(
      isSettlementActionResult({
        Stopped: {
          state: { Withdrawal: { Observed: null } },
          reason: { LedgerFeeExceedsServiceFee: null },
        },
      }),
    ).toBe(true)
    expect(
      isSettlementActionResult({
        Stopped: { state: { Withdrawal: { Observed: null } }, reason: { RpcUnavailable: null } },
      }),
    ).toBe(true)
    expect(
      isSettlementActionResult({
        Stopped: {
          state: { Deposit: { AuthorizationPending: null } },
          reason: { AuthorizationExpired: null },
        },
      }),
    ).toBe(true)
    expect(
      isSettlementActionResult({
        Stopped: {
          state: { Deposit: { AuthorizationPending: null } },
          reason: { AuthorizationWindowTooShort: null },
        },
      }),
    ).toBe(true)
    expect(
      isSettlementActionResult({
        Stopped: { state: { Withdrawal: { Observed: null } }, reason: { LedgerRejected: null } },
      }),
    ).toBe(false)
    expect(
      isSettlementActionResult({
        Submitted: {
          state: { Deposit: { AuthorizationPending: null } },
          transaction_hash: new Uint8Array(32),
        },
      }),
    ).toBe(false)
  })

  it("distinguishes finalized confirmation, RPC stops, and audit stops", () => {
    const phase = { RefundAvailable: null } as const
    expect(depositReconciliationMessage(phase)).toBeUndefined()
    expect(depositReconciliationMessage(phase, { RpcUnavailable: null })).toBe(
      "Base RPC confirmation stopped — requesting again is safe",
    )
    expect(depositReconciliationMessage(phase, { BaseStateMismatch: null })).toBe(
      "Mint evidence requires audit — refund is blocked",
    )
  })

  it("derives_automatic_retry_refund_and_blocked_authorization_recovery_from_canonical_facts", () => {
    const record = (reason: DepositView["last_settlement_stop_reason"], automatic = false) =>
      ({
        state: { AuthorizationPending: null },
        last_settlement_stop_reason: reason,
        automatic_progress: automatic ? [{ state: { Scheduled: { next_run_at_ns: 10n } } }] : [],
      }) as DepositView

    expect(depositContinuation(record([{ SigningUnavailable: null }], true)).mode).toBe("automatic")
    expect(depositContinuation(record([{ RpcInconsistent: null }])).action).toBe(
      "retry-authorization",
    )
    expect(depositContinuation(record([{ AuthorizationExpired: null }])).action).toBe(
      "request-refund",
    )
    const shortWindow = depositContinuation(record([{ AuthorizationWindowTooShort: null }]))
    expect(shortWindow).toMatchObject({
      mode: "stopped",
      action: "request-refund",
    })
    expect(shortWindow.message).toContain("Wait for finalized Base time")
    expect(depositContinuation(record([{ BridgeSignerMismatch: null }]))).toMatchObject({
      mode: "stopped",
    })
    expect(depositContinuation(record([{ BridgeSignerMismatch: null }])).action).toBeUndefined()
    expect(depositContinuation(record([{ Unknown: "future stop" }])).message).toContain(
      "future stop",
    )

    const refundRecord = {
      ...record([{ RpcUnavailable: null }]),
      state: { RefundProcessing: null },
    } as DepositView
    expect(depositContinuation(refundRecord)).toMatchObject({ mode: "stopped" })
    expect(depositContinuation(refundRecord).action).toBeUndefined()
  })

  it("uses a browser pending mint only while the canonical Deposit remains mintable", () => {
    expect(depositUsesPendingMintStatus({ AuthorizationAvailable: null }, true, false)).toBe(true)
    expect(depositUsesPendingMintStatus({ AuthorizationAvailable: null }, true, true)).toBe(false)
    for (const phase of [
      { Minted: null },
      { RefundAvailable: null },
      { RefundProcessing: null },
      { Refunded: null },
      { FundingReconciliationHold: null },
      { Cancelled: null },
    ] as const) {
      expect(depositUsesPendingMintStatus(phase, true, false)).toBe(false)
    }
  })

  it("enables_an_expired_authorization_refund_only_after_finalized_Base_time_passes_the_deadline", () => {
    const record = {
      mint_authorization: [{ deadline: 1_000n }],
    } as DepositView

    expect(authorizationDeadlineRefundStatus(record)).toBe("checking-finality")
    expect(authorizationDeadlineRefundStatus(record, 999n)).toBe("waiting-finality")
    expect(authorizationDeadlineRefundStatus(record, 1_000n)).toBe("waiting-finality")
    expect(authorizationDeadlineRefundStatus(record, 1_001n)).toBe("ready")
  })
})
