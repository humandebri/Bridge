import type { DepositPhase, SettlementActionResult, SettlementState, WithdrawalPhase } from "@/generated/bridge.did"

const depositNames = ["EscrowedUnquoted", "AuthorizationPending", "AuthorizationAvailable", "RefundAvailable", "Minted", "FundingReconciliationHold", "RefundProcessing", "Refunded", "Cancelled"] as const
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
    AuthorizationPending: "Signing",
    AuthorizationAvailable: "Ready to mint",
    RefundAvailable: "Refund ready",
    Minted: "Complete",
    FundingReconciliationHold: "Review needed",
    RefundProcessing: "Refunding",
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

export function depositUsesPendingMintStatus(
  phase: DepositPhase,
  hasPendingMint: boolean,
  mintedOnBase: boolean,
): boolean {
  return hasPendingMint && !mintedOnBase && "AuthorizationAvailable" in phase
}

export function isWithdrawalTerminal(phase: WithdrawalPhase): boolean {
  const name = variantName(phase, withdrawalNames)
  return name === "Paid"
}

export function depositPhaseTone(phase: DepositPhase): "good" | "warn" | "neutral" {
  const name = variantName(phase, depositNames)
  if (name === "Minted" || name === "Refunded") return "good"
  if (name === "FundingReconciliationHold" || name === "RefundProcessing") return "warn"
  return isDepositTerminal(phase) ? "warn" : "neutral"
}

export function depositReconciliationMessage(
  phase: DepositPhase,
  stopReason?: string,
): string | undefined {
  if (!("RefundAvailable" in phase) && !("RefundProcessing" in phase)) {
    return stopReason ? "Processing stopped — retry from History" : undefined
  }
  if (!stopReason) return undefined
  if (["RpcUnavailable", "RpcInconsistent", "InvalidBaseResponse"].includes(stopReason)) {
    return "Base RPC confirmation stopped — requesting again is safe"
  }
  if (["BaseStateMismatch", "BridgeSignerMismatch"].includes(stopReason)) {
    return "Mint evidence requires audit — refund is blocked"
  }
  return "Finalized Base confirmation stopped — request again from History"
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
