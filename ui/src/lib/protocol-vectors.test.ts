import { describe, expect, it } from "vitest"
import vectors from "../../../verification/generated/protocol-vectors.json"
import { decideWithdrawalFinalization } from "./withdrawal-confirmation-state"
import { pendingConfirmationKey, upsertPendingConfirmation, type PendingConfirmation } from "./pending-confirmations"

const deployment = {
  bridgeCanisterId: "aaaaa-aa",
  chainId: 8453,
  bridgeAddress: "0x1111111111111111111111111111111111111111",
}

function withdrawal(byte: string, blocked: boolean, owner: string): PendingConfirmation {
  return {
    kind: "withdrawal",
    transactionHash: `0x${byte.repeat(64)}`,
    owner,
    blocked,
    ...deployment,
  }
}

describe("Lean protocol conformance vectors", () => {
  it("accepts exactly the current nonempty vector schema", () => {
    expect(vectors.schema_version).toBe(2)
    expect(Object.keys(vectors).sort()).toEqual([
      "canonical_probe_cases",
      "canonical_probe_count",
      "deposit_admission_cases",
      "deposit_admission_count",
      "fee_payout_cases",
      "fee_payout_count",
      "fee_rotation_cases",
      "fee_rotation_count",
      "finalization_cases",
      "finalization_count",
      "funding_attempt_cases",
      "funding_attempt_count",
      "funding_reconciliation_cases",
      "funding_reconciliation_count",
      "hold_cases",
      "hold_count",
      "lease_cases",
      "lease_count",
      "lease_lane_cases",
      "lease_lane_count",
      "manual_claim_cases",
      "manual_claim_count",
      "notification_admission_cases",
      "notification_admission_count",
      "payment_cases",
      "payment_count",
      "queue_cases",
      "queue_count",
      "quote_cases",
      "quote_count",
      "reservation_cases",
      "reservation_count",
      "schema_version",
      "service_fee_cases",
      "service_fee_count",
      "settlement_cases",
      "settlement_count",
    ])
    expect(vectors.quote_cases).toHaveLength(vectors.quote_count)
    expect(vectors.settlement_cases).toHaveLength(vectors.settlement_count)
    expect(vectors.payment_cases).toHaveLength(vectors.payment_count)
    expect(vectors.deposit_admission_cases).toHaveLength(vectors.deposit_admission_count)
    expect(vectors.reservation_cases).toHaveLength(vectors.reservation_count)
    expect(vectors.service_fee_cases).toHaveLength(vectors.service_fee_count)
    expect(vectors.fee_rotation_cases).toHaveLength(vectors.fee_rotation_count)
    expect(vectors.fee_payout_cases).toHaveLength(vectors.fee_payout_count)
    expect(vectors.hold_cases).toHaveLength(vectors.hold_count)
    expect(vectors.lease_cases).toHaveLength(vectors.lease_count)
    expect(vectors.manual_claim_cases).toHaveLength(vectors.manual_claim_count)
    expect(vectors.notification_admission_cases).toHaveLength(vectors.notification_admission_count)
    expect(vectors.lease_lane_cases).toHaveLength(vectors.lease_lane_count)
    expect(vectors.funding_attempt_cases).toHaveLength(vectors.funding_attempt_count)
    expect(vectors.funding_reconciliation_cases).toHaveLength(vectors.funding_reconciliation_count)
    expect(vectors.finalization_cases).toHaveLength(vectors.finalization_count)
    expect(vectors.queue_cases).toHaveLength(vectors.queue_count)
    expect(vectors.canonical_probe_cases).toHaveLength(vectors.canonical_probe_count)
    expect(vectors.quote_count).toBeGreaterThan(0)
    expect(vectors.settlement_count).toBeGreaterThan(0)
    expect(vectors.payment_count).toBeGreaterThan(0)
    expect(vectors.deposit_admission_count).toBeGreaterThan(0)
    expect(vectors.reservation_count).toBeGreaterThan(0)
    expect(vectors.service_fee_count).toBeGreaterThan(0)
    expect(vectors.fee_rotation_count).toBeGreaterThan(0)
    expect(vectors.fee_payout_count).toBeGreaterThan(0)
    expect(vectors.hold_count).toBeGreaterThan(0)
    expect(vectors.lease_count).toBeGreaterThan(0)
    expect(vectors.manual_claim_count).toBeGreaterThan(0)
    expect(vectors.notification_admission_count).toBeGreaterThan(0)
    expect(vectors.lease_lane_count).toBeGreaterThan(0)
    expect(vectors.funding_attempt_count).toBeGreaterThan(0)
    expect(vectors.funding_reconciliation_count).toBeGreaterThan(0)
    expect(vectors.finalization_count).toBeGreaterThan(0)
    expect(vectors.queue_count).toBeGreaterThan(0)
    expect(vectors.canonical_probe_count).toBeGreaterThan(0)
  })

  it("protocol_finalization_cases_matches_production", () => {
    for (const testCase of vectors.finalization_cases) {
      expect(decideWithdrawalFinalization(
        testCase.receipt_succeeded ? "success" : "reverted",
        BigInt(testCase.receipt_block),
        testCase.finalized_block === null ? null : BigInt(testCase.finalized_block),
      )).toBe(testCase.decision)
    }
  })

  it("protocol_queue_cases_matches_production", () => {
    for (const testCase of vectors.queue_cases) {
      const other = withdrawal("2", testCase.other_blocked, "other")
      const incoming = withdrawal("1", testCase.incoming_blocked, "incoming")
      const existing = testCase.existing_blocked === null
        ? []
        : [withdrawal("1", testCase.existing_blocked, "existing")]
      const restored = upsertPendingConfirmation([...existing, other], incoming, true)
      const target = restored.find((entry) => pendingConfirmationKey(entry) === pendingConfirmationKey(incoming))
      const preservedOther = restored.find((entry) => pendingConfirmationKey(entry) === pendingConfirmationKey(other))
      expect(target?.blocked).toBe(testCase.expected_blocked)
      expect(preservedOther?.blocked).toBe(testCase.expected_other_blocked)
    }
  })
})
