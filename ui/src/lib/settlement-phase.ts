import type { DepositPhase, SettlementActionResult, SettlementState, WithdrawalPhase } from "@/generated/bridge.did"

const depositNames = ["EscrowedUnquoted", "AuthorizationPending", "AuthorizationAvailable", "ExpiryReconciliation", "Minted", "FundingReconciliationHold", "RefundPending", "RefundReconciliationHold", "Refunded", "Cancelled"] as const
const withdrawalNames = ["Observed", "ReleasePending", "Paid", "ReconciliationHold"] as const

function variantName(value: unknown, allowed: readonly string[]): string | undefined {
  if (typeof value !== "object" || value === null || Array.isArray(value)) return undefined
  const keys = Object.keys(value)
  const key = keys[0]
  if (keys.length !== 1 || key === undefined || !allowed.includes(key)) return undefined
  return Reflect.get(value, key) === null ? key : undefined
}

export function isDepositPhase(value: unknown): value is DepositPhase {
  return variantName(value, depositNames) !== undefined
}

export function isWithdrawalPhase(value: unknown): value is WithdrawalPhase {
  return variantName(value, withdrawalNames) !== undefined
}

export function depositPhaseName(phase: DepositPhase): string {
  const name = variantName(phase, depositNames)
  if (!name) throw new Error("Invalid deposit phase")
  const labels: Record<(typeof depositNames)[number], string> = {
    EscrowedUnquoted: "Checking Base",
    AuthorizationPending: "Signing authorization",
    AuthorizationAvailable: "Ready to mint",
    ExpiryReconciliation: "Checking finalized Base",
    Minted: "Complete",
    FundingReconciliationHold: "Funding needs review",
    RefundPending: "Refunding",
    RefundReconciliationHold: "Refund needs review",
    Refunded: "Refunded",
    Cancelled: "Cancelled",
  }
  return labels[name as (typeof depositNames)[number]]
}

export function withdrawalPhaseName(phase: WithdrawalPhase): string {
  const name = variantName(phase, withdrawalNames)
  if (!name) throw new Error("Invalid withdrawal phase")
  const labels: Record<(typeof withdrawalNames)[number], string> = {
    Observed: "Processing",
    ReleasePending: "Processing",
    Paid: "Paid",
    ReconciliationHold: "Needs attention",
  }
  return labels[name as (typeof withdrawalNames)[number]]
}

export function settlementStateName(state: SettlementState): string {
  if ("Deposit" in state) return depositPhaseName(state.Deposit)
  return withdrawalPhaseName(state.Withdrawal)
}

export function isDepositTerminal(phase: DepositPhase): boolean {
  const name = variantName(phase, depositNames)
  return name === "Minted" || name === "Refunded" || name === "Cancelled"
}

export function isWithdrawalTerminal(phase: WithdrawalPhase): boolean {
  const name = variantName(phase, withdrawalNames)
  return name === "Paid"
}

export function depositPhaseTone(phase: DepositPhase): "good" | "warn" | "neutral" {
  const name = variantName(phase, depositNames)
  if (name === "Minted" || name === "Refunded") return "good"
  if (name === "FundingReconciliationHold" || name === "RefundReconciliationHold") return "warn"
  return isDepositTerminal(phase) ? "warn" : "neutral"
}

export function depositReconciliationMessage(
  phase: DepositPhase,
  stopReason?: string,
): string | undefined {
  if (!("ExpiryReconciliation" in phase)) {
    return stopReason ? "Processing stopped — retry from History" : undefined
  }
  if (!stopReason) return "Confirming the finalized Base state"
  if (["RpcUnavailable", "RpcInconsistent", "InvalidBaseResponse"].includes(stopReason)) {
    return "Base RPC confirmation stopped — retry is safe"
  }
  if (["BaseStateMismatch", "BridgeSignerMismatch"].includes(stopReason)) {
    return "Mint evidence requires audit — refund is blocked"
  }
  return "Finalized Base confirmation stopped — retry from History"
}

export function withdrawalPhaseTone(phase: WithdrawalPhase): "good" | "warn" | "neutral" {
  const name = variantName(phase, withdrawalNames)
  if (name === "Paid") return "good"
  return name === "ReconciliationHold" ? "warn" : "neutral"
}

export function isSettlementState(value: unknown): value is SettlementState {
  if (typeof value !== "object" || value === null || Array.isArray(value)) return false
  const keys = Object.keys(value)
  if (keys.length !== 1) return false
  const key = keys[0]
  if (key === "Deposit") return isDepositPhase(Reflect.get(value, "Deposit"))
  if (key === "Withdrawal") return isWithdrawalPhase(Reflect.get(value, "Withdrawal"))
  return false
}

function hasExactKeys(value: Record<string, unknown>, expected: readonly string[]): boolean {
  const keys = Object.keys(value)
  return keys.length === expected.length && expected.every((key) => keys.includes(key))
}

function isSettlementStopReason(value: unknown): boolean {
  if (typeof value !== "object" || value === null || Array.isArray(value)) return false
  const keys = Object.keys(value)
  if (keys.length !== 1) return false
  const key = keys[0]
  if (key === undefined) return false
  const payload: unknown = Reflect.get(value, key)
  if (key === "LedgerRejected") return typeof payload === "string"
  return [
    "LedgerFeeExceedsServiceFee", "RpcUnavailable",
    "RpcInconsistent", "LedgerAmbiguous", "LedgerUnavailable", "BaseStateMismatch",
    "BridgeSignerMismatch", "SigningUnavailable", "InvalidBaseResponse",
  ].includes(key) && payload === null
}

export function isSettlementActionResult(value: unknown): value is SettlementActionResult {
  if (typeof value !== "object" || value === null || Array.isArray(value)) return false
  const keys = Object.keys(value)
  if (keys.length !== 1) return false
  const key = keys[0]
  if (key === undefined) return false
  const payload: unknown = (value as Record<string, unknown>)[key]
  if (typeof payload !== "object" || payload === null || Array.isArray(payload)) return false
  const record = payload as Record<string, unknown>
  if (!isSettlementState(record.state)) return false
  if (key === "Complete" || key === "ReconciliationProgress") return hasExactKeys(record, ["state"])
  if (key === "Deferred") return hasExactKeys(record, ["state", "next_run_at_ns"]) && typeof record.next_run_at_ns === "bigint"
  if (key === "Stopped") return hasExactKeys(record, ["state", "reason"]) && isSettlementStopReason(record.reason)
  return false
}
