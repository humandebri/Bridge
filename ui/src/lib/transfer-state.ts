import type { BridgeProgressPhase, BridgeProgressRecord } from "./bridge-progress"

export type TransferIssue =
  | "processed"
  | "unknown"
  | "expired"
  | "refund-ready"
  | "refund-available"
  | "reverted"
  | "conflict"
  | "stopped"
export type TransferOutcome = "minted" | "paid" | "refunded" | "cancelled" | "reverted"
export type TransferSource = "wallet" | "base" | "ic" | "operation"
export interface TransferFacts {
  identity: string
  generation: number
  revisions: Partial<Record<TransferSource, number>>
  direction: "deposit" | "withdraw"
  phase: BridgeProgressPhase
  issue?: TransferIssue
  outcome?: TransferOutcome
  warnings: { transport?: string; storage?: string; wallet?: string }
  transportErrors?: Partial<Record<TransferSource, string>>
  message?: string
  transactionHash?: `0x${string}`
  recordingPending?: boolean
}
export type TransferEvent = {
  identity: string
  generation: number
  source: TransferSource
  revision: number
} & (
  | {
      type: "observed"
      phase: BridgeProgressPhase
      issue?: TransferIssue
      outcome?: TransferOutcome
      message?: string
      transactionHash?: `0x${string}`
      recordingPending?: boolean
    }
  | { type: "warning"; cause: keyof TransferFacts["warnings"]; message?: string }
)
export interface TransferPresentation {
  code: string
  title: string
  description: string
  icon: "spinner" | "warning" | "success"
  terminal: boolean
  actions: Array<"check-refund" | "review" | "retry-notification" | "continue-payout">
}

export function initialTransferFacts(
  identity: string,
  direction: TransferFacts["direction"],
  phase: BridgeProgressPhase,
): TransferFacts {
  return { identity, direction, phase, generation: 0, revisions: {}, warnings: {} }
}

/** Presentation reducer: this never authorizes a send, refund, or notification. */
export function reduceTransfer(facts: TransferFacts, event: TransferEvent): TransferFacts {
  if (
    event.identity !== facts.identity ||
    event.generation !== facts.generation ||
    event.revision <= (facts.revisions[event.source] ?? -1)
  )
    return facts
  let next = { ...facts, revisions: { ...facts.revisions, [event.source]: event.revision } }
  if (event.type === "warning") {
    if (event.cause === "transport") {
      const transportErrors = { ...facts.transportErrors, [event.source]: event.message }
      return {
        ...next,
        transportErrors,
        warnings: { ...facts.warnings, transport: Object.values(transportErrors).find(Boolean) },
      }
    }
    return { ...next, warnings: { ...facts.warnings, [event.cause]: event.message } }
  }
  if (event.source === "base" || event.source === "ic") {
    const transportErrors = { ...facts.transportErrors, [event.source]: undefined }
    next = {
      ...next,
      transportErrors,
      warnings: { ...facts.warnings, transport: Object.values(transportErrors).find(Boolean) },
    }
  }
  // Conflicting confirmed evidence requires review, even when first discovered together.
  if (facts.issue === "conflict") return next
  if (facts.outcome) {
    if ((event.outcome && event.outcome !== facts.outcome) || event.issue === "conflict")
      return {
        ...next,
        issue: "conflict",
        message: "Confirmed evidence conflicts. Review required.",
      }
    // A later inclusion/transport observation cannot undo a verified terminal fact.
    return { ...next, recordingPending: event.recordingPending ?? facts.recordingPending }
  }
  // IC authorization availability is not evidence that an existing wallet attempt resumed.
  if (
    event.source === "ic" &&
    event.phase === "awaiting-base-mint" &&
    [
      "awaiting-base-mint",
      "base-mint-submitted",
      "base-mint-included",
      "base-mint-finalizing",
      "attention",
    ].includes(facts.phase) &&
    (facts.phase !== "attention" || facts.issue !== "stopped")
  )
    return next
  return {
    ...next,
    phase: event.phase,
    issue: event.issue,
    outcome: event.outcome,
    message: event.message,
    transactionHash: event.transactionHash ?? facts.transactionHash,
    recordingPending: event.recordingPending ?? facts.recordingPending,
    warnings: {
      ...next.warnings,
      wallet:
        event.source === "wallet" || event.transactionHash || event.phase !== facts.phase
          ? undefined
          : facts.warnings.wallet,
    },
  }
}

export function transferPresentation(facts: TransferFacts): TransferPresentation {
  const base = { terminal: Boolean(facts.outcome), actions: [] as TransferPresentation["actions"] }
  if (facts.issue === "processed")
    return {
      ...base,
      code: "processed",
      title: "Processed on Base",
      description: "Looking for the transaction receipt.",
      icon: "warning",
      actions: [],
    }
  if (facts.issue === "conflict")
    return {
      ...base,
      code: "conflict",
      title: "Transfer needs review",
      description: "Conflicting evidence. Do not submit again.",
      icon: "warning",
      actions: ["review"],
    }
  if (facts.outcome) {
    const title = {
      minted: "Mint complete",
      paid: "Withdrawal complete",
      refunded: "Refund complete",
      cancelled: "Transfer cancelled",
      reverted: "Transaction reverted",
    }[facts.outcome]
    return {
      ...base,
      code: facts.outcome,
      title,
      description: facts.recordingPending ? "Waiting for IC recording." : (facts.message ?? ""),
      icon: facts.outcome === "reverted" || facts.outcome === "cancelled" ? "warning" : "success",
    }
  }
  const warning = facts.warnings.wallet ?? facts.warnings.transport
  if (warning)
    return {
      ...base,
      code: facts.warnings.wallet ? "wallet-pending" : "unavailable",
      title: facts.warnings.wallet ? "Wallet response pending" : "Status unavailable",
      description: warning,
      icon: "warning",
    }
  if (facts.issue === "refund-available")
    return {
      ...base,
      code: "refund-available",
      title: "Refund available",
      description: "Check your refund to continue.",
      icon: "warning",
      actions: ["check-refund"],
    }
  if (facts.issue === "refund-ready" || facts.issue === "expired")
    return {
      ...base,
      code: facts.issue,
      title: facts.issue === "refund-ready" ? "Mint status unknown" : "Mint authorization expired",
      description: "The deadline has passed. Check if you can get a refund.",
      icon: "warning",
      actions: ["check-refund"],
    }
  if (facts.issue === "unknown")
    return {
      ...base,
      code: "unknown",
      title: "Submission status unknown",
      description: "Check your wallet. No automatic retry.",
      icon: "warning",
      actions: ["review"],
    }
  if (facts.phase === "attention")
    return {
      ...base,
      code: facts.issue ?? "stopped",
      title: "Transfer needs attention",
      description: facts.message ?? "Review this transfer before continuing.",
      icon: "warning",
      actions: ["review"],
    }
  const titles: Partial<Record<BridgeProgressPhase, string>> = {
    "awaiting-base-mint": "Confirm mint in your wallet",
    "base-mint-submitted": "Confirming mint",
    "base-mint-included": "Mint included",
    "base-mint-finalizing": "Waiting for Base finality",
    "base-withdrawal-submitted": "Confirming withdrawal",
    "base-withdrawal-included": "Withdrawal included",
    "base-withdrawal-finalizing": "Waiting for Base finality",
    "awaiting-ic-notification": "Recording withdrawal on IC",
    "ic-notification-recorded": "Withdrawal recorded",
    "ledger-payout": "Payout pending",
    "refund-checking": "Checking refund",
    "refund-processing": "Refund processing",
    "refund-waiting": "Waiting to check refund",
    "authorization-generating": "Preparing mint authorization",
    "awaiting-base-withdrawal": "Confirm withdrawal in your wallet",
    "awaiting-base-allowance": "Confirm token approval",
    "awaiting-ic-allowance": "Confirm token approval",
    "awaiting-ic-deposit": "Confirm deposit in your wallet",
    "ic-deposit-accepted": "Deposit accepted",
    "verifying-ic-destination": "Checking IC destination",
    "awaiting-base-approval-reflection": "Confirming token approval",
  }
  return {
    ...base,
    code: facts.phase,
    title: titles[facts.phase] ?? "Checking transfer",
    description: facts.phase === "refund-waiting" ? "Waiting for finalized Base time." : "",
    icon: "spinner",
  }
}

export function transferIdentity(
  record: Pick<BridgeProgressRecord, "direction" | "deposit" | "transactionHash" | "id">,
): string {
  return record.direction === "deposit"
    ? `deposit:${record.deposit?.depositId?.toLowerCase() ?? record.id}`
    : `withdraw:${record.transactionHash?.toLowerCase() ?? record.id}`
}

const snapshots = new Map<string, TransferFacts>()
const listeners = new Set<() => void>()
export const subscribeTransfers = (listener: () => void) => {
  listeners.add(listener)
  return () => {
    listeners.delete(listener)
  }
}
export const readTransferFacts = (identity: string) => snapshots.get(identity)
export function publishTransferFacts(facts: TransferFacts): void {
  const current = snapshots.get(facts.identity)
  if (current === facts) return
  if (
    current &&
    (current.generation > facts.generation ||
      (current.generation === facts.generation &&
        Object.entries(current.revisions).some(
          ([source, revision]) => revision > (facts.revisions[source as TransferSource] ?? -1),
        )))
  )
    return
  snapshots.set(facts.identity, facts)
  listeners.forEach((listener) => listener())
}

export function clearTransferFacts(): void {
  snapshots.clear()
  listeners.forEach((listener) => listener())
}

/** Recheck immediately before a wallet or continuation operation. */
export function assertTransferActionAllowed(identity: string): void {
  const facts = readTransferFacts(identity)
  if (facts?.outcome || facts?.issue === "conflict" || facts?.issue === "processed")
    throw new Error("Transfer already resolved or requires review.")
}
