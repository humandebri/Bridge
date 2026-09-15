import { describe, expect, it } from "vitest"
import type { FinalizedRuntimeObservation } from "@/lib/runtime-validation"
import {
  bridgePageReducer,
  initialBridgePageState,
  type PreflightState,
  type ReviewedDeposit,
  type UnresolvedDepositAttempt,
} from "./bridge-page-state"

const observation = { snapshot: {} } as FinalizedRuntimeObservation
const account = { owner: "aaaaa-aa" }
const attempt = {
  account,
  recipient: `0x${"11".repeat(20)}`,
  call: {
    ownerSequence: 7n,
    baseRecipient: new Uint8Array(20),
    grossAmount: 100n,
    maxServiceFee: 10n,
  },
} satisfies UnresolvedDepositAttempt

function preflight(runId = 1, direction: "deposit" | "withdraw" = "deposit"): PreflightState {
  return {
    runId,
    direction,
    phase: "checking",
    checks: [
      { id: "wallets", label: "Wallets connected", status: "waiting" },
      { id: "runtime", label: "Bridge configuration check", status: "waiting" },
      { id: "financials", label: "Balance and fees checked", status: "waiting" },
      { id: "availability", label: "Transfer availability checked", status: "waiting" },
    ],
  }
}

describe("bridgePageReducer", () => {
  it("ignores stale preflight results after a newer review starts", () => {
    const current = bridgePageReducer(initialBridgePageState, {
      type: "review-started",
      preflight: preflight(2),
    })
    const next = bridgePageReducer(current, {
      type: "preflight-check-updated",
      runId: 1,
      id: "wallets",
      status: "failed",
      error: "stale",
    })

    expect(next).toBe(current)
  })

  it("keeps a failed check visible and permits a fresh review", () => {
    const checking = bridgePageReducer(initialBridgePageState, {
      type: "review-started",
      preflight: preflight(),
    })
    const failed = bridgePageReducer(checking, {
      type: "preflight-check-updated",
      runId: 1,
      id: "runtime",
      status: "failed",
      error: "unavailable",
    })

    expect(failed.review.status).toBe("failed")
    if (failed.review.status === "failed") {
      expect(failed.review.preflight.phase).toBe("failed")
      expect(failed.review.preflight.checks[1]).toMatchObject({
        status: "failed",
        error: "unavailable",
      })
    }

    const retried = bridgePageReducer(failed, {
      type: "review-started",
      preflight: preflight(2),
    })
    expect(retried.review.status).toBe("checking")
  })

  it("creates a typed deposit review only for its active run", () => {
    const checking = bridgePageReducer(initialBridgePageState, {
      type: "review-started",
      preflight: preflight(),
    })
    const deposit = {
      amount: 100n,
      account,
      recipient: attempt.recipient,
      gate: {
        base: {} as ReviewedDeposit["gate"]["base"],
        ledger: { balance: 200n, fee: 1n, allowance: 0n },
        sequence: 7n,
        observation,
      },
    } satisfies ReviewedDeposit
    const ready = bridgePageReducer(checking, {
      type: "deposit-review-ready",
      runId: 1,
      observation,
      approvalNeeded: true,
      deposit,
    })

    expect(ready.review).toMatchObject({
      status: "ready",
      direction: "deposit",
      mode: "new",
      approvalNeeded: true,
    })
  })

  it("atomically replaces a saved intent with the accepted deposit identity", () => {
    const saved = bridgePageReducer(initialBridgePageState, {
      type: "deposit-intent-saved",
      attempt,
    })
    const accepted = bridgePageReducer(saved, {
      type: "deposit-accepted",
      owner: account.owner,
      sequence: 7n,
    })

    expect(accepted.depositRecovery).toEqual({ status: "clear", owner: account.owner })
    expect(accepted.depositExecution).toEqual({
      status: "authorization",
      active: { owner: account.owner, sequence: 7n },
    })
  })

  it("does not let an old terminal observation clear another active deposit", () => {
    const accepted = bridgePageReducer(initialBridgePageState, {
      type: "deposit-accepted",
      owner: account.owner,
      sequence: 8n,
    })
    const next = bridgePageReducer(accepted, {
      type: "deposit-terminal-reset",
      owner: account.owner,
      sequence: 7n,
    })

    expect(next).toBe(accepted)
  })

  it("ignores a saved-intent completion from a previously connected owner", () => {
    const currentAttempt = {
      ...attempt,
      account: { owner: "rrkah-fqaaa-aaaaa-aaaaq-cai" },
    }
    const current = bridgePageReducer(initialBridgePageState, {
      type: "deposit-intent-resolved",
      owner: currentAttempt.account.owner,
      attempt: currentAttempt,
    })
    const next = bridgePageReducer(current, {
      type: "deposit-intent-check-finished",
      owner: attempt.account.owner,
    })

    expect(next).toBe(current)
  })

  it("keeps the withdrawal amount on failure and clears it only after broadcast", () => {
    const entered = bridgePageReducer(initialBridgePageState, {
      type: "amount-changed",
      direction: "withdraw",
      value: "12.5",
    })
    const submitting = bridgePageReducer(entered, { type: "withdrawal-submission-started" })
    const failed = bridgePageReducer(submitting, { type: "withdrawal-submission-finished" })
    const succeeded = bridgePageReducer(submitting, {
      type: "withdrawal-submission-finished",
      clearAmount: true,
    })

    expect(failed.withdrawAmount).toBe("12.5")
    expect(succeeded.withdrawAmount).toBe("")
  })
})
