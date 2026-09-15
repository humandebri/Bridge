import { describe, expect, it } from "vitest"
import {
  initialTransferFacts,
  reduceTransfer,
  transferPresentation,
  type TransferEvent,
  type TransferFacts,
  type TransferOutcome,
} from "./transfer-state"

const initial = () => initialTransferFacts("deposit:one", "deposit", "awaiting-base-mint")
const observed = (
  facts: TransferFacts,
  patch: Partial<Extract<TransferEvent, { type: "observed" }>> = {},
): TransferEvent => ({
  type: "observed",
  identity: facts.identity,
  generation: facts.generation,
  source: "base",
  revision: (facts.revisions.base ?? 0) + 1,
  phase: "base-mint-included",
  ...patch,
})

describe("transfer state", () => {
  it("transfer_state_rejects_stale_events", function transfer_state_rejects_stale_events() {
    const facts = reduceTransfer(initial(), observed(initial()))
    for (const patch of [
      { identity: "deposit:other" },
      { generation: -1 },
      { generation: 1 },
      { revision: 0 },
      { revision: 1 },
    ])
      expect(reduceTransfer(facts, observed(facts, patch))).toBe(facts)
    expect(reduceTransfer(facts, observed(facts, { phase: "base-mint-submitted" })).phase).toBe(
      "base-mint-submitted",
    )
  })
  it("transfer_terminal_evidence_is_absorbing", function transfer_terminal_evidence_is_absorbing() {
    const conflictFirst = reduceTransfer(
      initial(),
      observed(initial(), { phase: "attention", issue: "conflict" }),
    )
    for (const outcome of [undefined, "minted", "refunded"] as const) {
      const stillConflicting = reduceTransfer(
        conflictFirst,
        observed(conflictFirst, { phase: "complete", outcome }),
      )
      expect(transferPresentation(stillConflicting).code).toBe("conflict")
      expect(stillConflicting.outcome).toBeUndefined()
      expect(transferPresentation(stillConflicting).actions).toEqual(["review"])
    }

    for (const outcome of [
      "minted",
      "paid",
      "refunded",
      "cancelled",
      "reverted",
    ] as TransferOutcome[]) {
      const facts = reduceTransfer(initial(), observed(initial(), { outcome, phase: "complete" }))
      for (const phase of [
        "base-mint-submitted",
        "base-mint-included",
        "authorization-generating",
        "ledger-payout",
        "refund-processing",
      ] as const) {
        const next = reduceTransfer(facts, observed(facts, { phase }))
        expect(next.outcome).toBe(outcome)
        expect(next.phase).toBe("complete")
      }
      const conflict = reduceTransfer(
        facts,
        observed(facts, {
          outcome: outcome === "minted" ? "refunded" : "minted",
          phase: "complete",
        }),
      )
      expect(transferPresentation(conflict).code).toBe("conflict")
      expect(transferPresentation(conflict).actions).not.toContain("check-refund")
    }
  })
  it("transfer_warnings_recover_independently", function transfer_warnings_recover_independently() {
    let facts = initial()
    for (const cause of ["storage", "transport", "wallet"] as const) {
      facts = reduceTransfer(facts, {
        identity: facts.identity,
        generation: 0,
        revision: (facts.revisions.base ?? 0) + 1,
        source: "base",
        type: "warning",
        cause,
        message: cause,
      })
    }
    expect(transferPresentation(facts).icon).toBe("warning")
    facts = reduceTransfer(facts, observed(facts, { transactionHash: `0x${"1".repeat(64)}` }))
    expect(facts.warnings).toEqual({ transport: undefined, wallet: undefined, storage: "storage" })
    expect(facts.phase).toBe("base-mint-included")
    facts = reduceTransfer(facts, {
      ...observed(facts),
      type: "warning",
      cause: "transport",
      message: "Base offline",
    })
    facts = reduceTransfer(
      facts,
      observed(facts, { source: "ic", revision: 1, phase: "awaiting-base-mint" }),
    )
    expect(facts.warnings.transport).toBe("Base offline")
    facts = reduceTransfer(facts, observed(facts))
    expect(facts.warnings.transport).toBeUndefined()
    expect(facts.warnings.storage).toBe("storage")
  })
  it("transfer_refund_and_payout_states_are_distinct", function transfer_refund_and_payout_states_are_distinct() {
    const seen = new Set<string>()
    for (const phase of [
      "verifying-ic-destination",
      "awaiting-ic-allowance",
      "awaiting-ic-deposit",
      "ic-deposit-accepted",
      "authorization-generating",
      "awaiting-base-mint",
      "base-mint-submitted",
      "base-mint-included",
      "base-mint-finalizing",
      "awaiting-base-allowance",
      "awaiting-base-approval-reflection",
      "awaiting-base-withdrawal",
      "base-withdrawal-submitted",
      "base-withdrawal-included",
      "ic-notification-recorded",
      "refund-waiting",
      "refund-checking",
      "refund-processing",
      "base-withdrawal-finalizing",
      "awaiting-ic-notification",
      "ledger-payout",
    ] as const) {
      const p = transferPresentation({ ...initial(), phase })
      expect(p.terminal).toBe(false)
      seen.add(p.code)
    }
    expect(seen.size).toBe(21)
    const ready = transferPresentation({ ...initial(), phase: "attention", issue: "refund-ready" })
    expect(ready.title).toBe("Mint status unknown")
    expect(ready.actions).toEqual(["check-refund"])
    expect(transferPresentation({ ...initial(), issue: "expired" }).title).toBe(
      "Mint authorization expired",
    )
    expect(
      transferPresentation({ ...initial(), outcome: "paid", phase: "complete" }).terminal,
    ).toBe(true)
  })
})
