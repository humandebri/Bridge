import { useEffect, useSyncExternalStore } from "react"
import type { DepositView } from "@/generated/bridge.did"
import type { WithdrawalHistoryItem } from "@/lib/activity-history"
import {
  initialTransferFacts,
  readTransferFacts,
  reduceTransfer,
  publishTransferFacts,
  subscribeTransfers,
  transferPresentation,
  type TransferFacts,
} from "@/lib/transfer-state"

export function useTransferPresentation(fallback: TransferFacts) {
  const current = useSyncExternalStore(
    subscribeTransfers,
    () => readTransferFacts(fallback.identity),
    () => undefined,
  )
  const canonical = JSON.stringify(fallback)
  useEffect(() => {
    const observed = JSON.parse(canonical) as TransferFacts
    const previous =
      readTransferFacts(observed.identity) ??
      initialTransferFacts(observed.identity, observed.direction, observed.phase)
    let next = reduceTransfer(previous, {
      identity: previous.identity,
      generation: previous.generation,
      source: "ic",
      revision: (previous.revisions.ic ?? 0) + 1,
      type: "observed",
      phase: observed.phase,
      issue: observed.issue,
      outcome: observed.outcome,
      message: observed.message,
      recordingPending: observed.recordingPending,
    })
    next = reduceTransfer(next, {
      identity: previous.identity,
      generation: previous.generation,
      source: "base",
      revision: (next.revisions.base ?? 0) + 1,
      type: "warning",
      cause: "transport",
      message: observed.warnings.transport,
    })
    publishTransferFacts(next)
  }, [canonical])
  return transferPresentation(current ?? fallback)
}

export function depositFacts(
  record: DepositView,
  mint: {
    finalized: boolean
    included: boolean
    unavailable: boolean
    submitted?: boolean
    processed?: boolean
  },
): TransferFacts {
  const identity = `deposit:0x${Array.from(record.deposit_id, (b) => b.toString(16).padStart(2, "0")).join("")}`
  const facts = initialTransferFacts(identity, "deposit", "authorization-generating")
  if (mint.finalized && ("Refunded" in record.state || "Cancelled" in record.state))
    return { ...facts, phase: "attention", issue: "conflict" }
  if ("Minted" in record.state || mint.finalized)
    return {
      ...facts,
      phase: "complete",
      outcome: "minted",
      recordingPending: !("Minted" in record.state),
    }
  if ("Refunded" in record.state) return { ...facts, phase: "complete", outcome: "refunded" }
  if ("Cancelled" in record.state) return { ...facts, phase: "complete", outcome: "cancelled" }
  if ("RefundProcessing" in record.state) return { ...facts, phase: "refund-processing" }
  if ("RefundAvailable" in record.state)
    return { ...facts, phase: "attention", issue: "refund-available" }
  if ("FundingReconciliationHold" in record.state)
    return {
      ...facts,
      phase: "attention",
      issue: "stopped",
      message: "Transfer needs reconciliation.",
    }
  if ("AuthorizationAvailable" in record.state)
    facts.phase = mint.included
      ? "base-mint-finalizing"
      : mint.submitted
        ? "base-mint-submitted"
        : "awaiting-base-mint"
  if (mint.processed) facts.issue = "processed"
  if (mint.unavailable) facts.warnings.transport = "Base status could not be refreshed. Retrying."
  return facts
}

export function withdrawalFacts(record: WithdrawalHistoryItem): TransferFacts {
  const facts = initialTransferFacts(
    `withdraw:${record.hash?.toLowerCase() ?? record.id}`,
    "withdraw",
    "base-withdrawal-finalizing",
  )
  if (record.canister && "Paid" in record.canister.state)
    return { ...facts, phase: "complete", outcome: "paid" }
  if (record.canister && "ReconciliationHold" in record.canister.state)
    return {
      ...facts,
      phase: "attention",
      issue: "stopped",
      message: "Payout needs reconciliation.",
    }
  if (record.canister) facts.phase = "ledger-payout"
  if (record.baseNeedsReview) facts.warnings.transport = "Base receipt needs rechecking."
  return facts
}
