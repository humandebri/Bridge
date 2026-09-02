import type { DepositPhase, DepositView, SettlementActionResult, SettlementState, SettlementStopReason, WithdrawalPhase } from "@/generated/bridge.did"

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

export type DepositContinuation = {
  mode: "active" | "automatic" | "stopped"
  action?: "retry-authorization" | "request-refund"
  message?: string
  reason?: SettlementStopReason
}

export function settlementStopReasonName(reason: SettlementStopReason): string {
  return Object.keys(reason)[0] ?? "Unknown"
}

export function depositContinuation(record: DepositView): DepositContinuation {
  const reason = record.last_settlement_stop_reason?.[0]
  if ((record.automatic_progress?.length ?? 0) > 0) {
    return { mode: "automatic", reason, message: reason ? "The previous attempt stopped temporarily. The Bridge will retry automatically." : undefined }
  }
  if (!reason) return { mode: "active" }
  const name = settlementStopReasonName(reason)
  const authorizationPhase = "EscrowedUnquoted" in record.state || "AuthorizationPending" in record.state
  if (authorizationPhase && ["RpcUnavailable", "RpcInconsistent", "SigningUnavailable"].includes(name)) {
    return {
      mode: "stopped",
      action: "retry-authorization",
      reason,
      message: name === "SigningUnavailable"
        ? "Bridge signing stopped temporarily. Retry the same authorization."
        : "Base confirmation stopped temporarily. Retrying checks the same deposit again.",
    }
  }
  if (authorizationPhase && name === "AuthorizationExpired") {
    return {
      mode: "stopped",
      action: "request-refund",
      reason,
      message: "Mint authorization expired before signing completed. Request the finalized refund from History.",
    }
  }
  if (authorizationPhase && name === "AuthorizationWindowTooShort") {
    return {
      mode: "stopped",
      action: "request-refund",
      reason,
      message: "Less than five minutes remained before signing completed. Wait for finalized Base time to pass the deadline, then request a refund from History.",
    }
  }
  if (name === "BridgeSignerMismatch") {
    return { mode: "stopped", reason, message: "Bridge signer verification failed. Bridge configuration review is required before this deposit can continue." }
  }
  if (name === "InvalidBaseResponse" || name === "BaseStateMismatch") {
    return { mode: "stopped", reason, message: "Base verification failed. Bridge review is required before funds can move." }
  }
  const unknown = "Unknown" in reason ? reason.Unknown : undefined
  return { mode: "stopped", reason, message: unknown ? `Processing stopped: ${unknown}` : "Processing stopped. Bridge review is required." }
}

export type AuthorizationDeadlineRefundStatus = "checking-finality" | "waiting-finality" | "ready"

export function authorizationDeadlineRefundStatus(
  record: DepositView,
  finalizedBlockTimestamp?: bigint,
): AuthorizationDeadlineRefundStatus {
  const authorization = record.mint_authorization[0]
  if (!authorization || finalizedBlockTimestamp === undefined) return "checking-finality"
  return finalizedBlockTimestamp > authorization.deadline ? "ready" : "waiting-finality"
}

export function depositReconciliationMessage(
  phase: DepositPhase,
  stopReason?: SettlementStopReason,
): string | undefined {
  if (!("RefundAvailable" in phase) && !("RefundProcessing" in phase)) {
    if (!stopReason) return undefined
    const name = settlementStopReasonName(stopReason)
    if (["RpcUnavailable", "RpcInconsistent", "SigningUnavailable"].includes(name)) return "Processing stopped — retry from History"
    if (name === "AuthorizationExpired") return "Authorization expired — request a refund from History"
    if (name === "AuthorizationWindowTooShort") return "Authorization window closed — wait for finality, then request a refund from History"
    return "Processing stopped — Bridge review required"
  }
  if (!stopReason) return undefined
  const name = settlementStopReasonName(stopReason)
  if (["RpcUnavailable", "RpcInconsistent", "InvalidBaseResponse"].includes(name)) {
    return "Base RPC confirmation stopped — requesting again is safe"
  }
  if (["BaseStateMismatch", "BridgeSignerMismatch"].includes(name)) {
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
  if (key === "LedgerRejected" || key === "Unknown") return typeof payload === "string"
  return [
    "LedgerFeeExceedsServiceFee", "RpcUnavailable",
    "RpcInconsistent", "LedgerAmbiguous", "LedgerUnavailable", "BaseStateMismatch",
    "BridgeSignerMismatch", "SigningUnavailable", "InvalidBaseResponse",
    "AuthorizationExpired", "AuthorizationWindowTooShort",
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
