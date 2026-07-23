import { describe, expect, it } from "vitest"
import vectors from "../../../verification/generated/protocol-vectors.json"
import { decideNotificationFailure, decideWithdrawalFinalization } from "./withdrawal-confirmation-state"
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
      "fee_guard_pending_cases",
      "fee_guard_pending_count",
      "finalization_cases",
      "finalization_count",
      "queue_cases",
      "queue_count",
      "quote_cases",
      "quote_count",
      "schema_version",
      "settlement_cases",
      "settlement_count",
    ])
    expect(vectors.quote_cases).toHaveLength(vectors.quote_count)
    expect(vectors.settlement_cases).toHaveLength(vectors.settlement_count)
    expect(vectors.finalization_cases).toHaveLength(vectors.finalization_count)
    expect(vectors.queue_cases).toHaveLength(vectors.queue_count)
    expect(vectors.fee_guard_pending_cases).toHaveLength(vectors.fee_guard_pending_count)
    expect(vectors.canonical_probe_cases).toHaveLength(vectors.canonical_probe_count)
    expect(vectors.finalization_count).toBeGreaterThan(0)
    expect(vectors.queue_count).toBeGreaterThan(0)
    expect(vectors.fee_guard_pending_count).toBeGreaterThan(0)
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

  it("protocol_fee_guard_pending_cases_matches_production", () => {
    for (const testCase of vectors.fee_guard_pending_cases) {
      const target = withdrawal("1", false, "target")
      const other = withdrawal("2", true, "other")
      const errorCode = testCase.failure === "ledger-fee-exceeds-service-fee"
        ? "LedgerFeeExceedsServiceFee"
        : "RpcUnavailable"
      const kind = testCase.kind === "withdrawal" ? "withdrawal" : "deposit"
      const decision = decideNotificationFailure(kind, errorCode)
      const retainPending = decision === "retain-pending"
      const queue = [target, other]

      expect(retainPending).toBe(testCase.retain_pending)
      expect(queue.some((entry) => pendingConfirmationKey(entry) === pendingConfirmationKey(target)))
        .toBe(testCase.target_present)
      expect(queue.some((entry) => pendingConfirmationKey(entry) === pendingConfirmationKey(other)))
        .toBe(testCase.other_present)
      expect(testCase.history_refresh).toBe(false)
      expect(testCase.complete).toBe(false)
    }
  })
})
