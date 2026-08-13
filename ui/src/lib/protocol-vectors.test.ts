import { describe, expect, it } from "vitest"
import vectors from "../../../verification/generated/protocol-vectors.json"

const sections = [
  "canonical_probe_cases",
  "deposit_admission_cases",
  "deposit_identity_cases",
  "deposit_nonterminal_index_cases",
  "fee_payout_cases",
  "fee_rotation_cases",
  "finalization_cases",
  "funding_attempt_cases",
  "funding_reconciliation_cases",
  "hold_cases",
  "lease_cases",
  "lease_lane_cases",
  "ledger_block_provenance_cases",
  "manual_claim_cases",
  "notification_admission_cases",
  "payment_cases",
  "queue_cases",
  "quote_cases",
  "refund_request_identity_cases",
  "reservation_cases",
  "service_fee_cases",
  "settlement_cases",
] as const

describe("Lean protocol vector schema", () => {
  it("accepts exactly the current nonempty vector schema", () => {
    expect(vectors.schema_version).toBe(3)
    expect(Object.keys(vectors)).toHaveLength(1 + sections.length * 2)
    for (const section of sections) {
      const count = section.replace(/_cases$/, "_count") as keyof typeof vectors
      expect(vectors[section].length).toBe(vectors[count])
      expect(vectors[section].length).toBeGreaterThan(0)
    }
  })
})
