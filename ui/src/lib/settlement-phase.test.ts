import { describe, expect, it } from "vitest"
import { depositPhaseName, depositPhaseTone, depositReconciliationMessage, isDepositPhase, isDepositTerminal, isSettlementActionResult, isWithdrawalTerminal, settlementStateName, withdrawalPhaseName } from "./settlement-phase"

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
    expect(isSettlementActionResult({ Complete: { state: { Deposit: { Minted: null } } } })).toBe(true)
    expect(isSettlementActionResult({ Complete: { state: { Deposit: { FuturePhase: null } } } })).toBe(false)
    expect(isSettlementActionResult({ Complete: { state: { Deposit: { Minted: null } }, extra: true } })).toBe(false)
    expect(isSettlementActionResult({ Stopped: { state: { Withdrawal: { Observed: null } } } })).toBe(false)
    expect(isSettlementActionResult({ Stopped: { state: { Withdrawal: { Observed: null } }, reason: { LedgerFeeExceedsServiceFee: null } } })).toBe(true)
    expect(isSettlementActionResult({ Stopped: { state: { Withdrawal: { Observed: null } }, reason: { RpcUnavailable: null } } })).toBe(true)
    expect(isSettlementActionResult({ Stopped: { state: { Withdrawal: { Observed: null } }, reason: { LedgerRejected: null } } })).toBe(false)
    expect(isSettlementActionResult({ Submitted: { state: { Deposit: { AuthorizationPending: null } }, transaction_hash: new Uint8Array(32) } })).toBe(false)
  })

  it("distinguishes finalized confirmation, RPC stops, and audit stops", () => {
    const phase = { RefundAvailable: null } as const
    expect(depositReconciliationMessage(phase)).toBeUndefined()
    expect(depositReconciliationMessage(phase, "RpcUnavailable")).toBe("Base RPC confirmation stopped — requesting again is safe")
    expect(depositReconciliationMessage(phase, "BaseStateMismatch")).toBe("Mint evidence requires audit — refund is blocked")
  })
})
