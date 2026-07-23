import type { DepositPhase, SettlementActionResult, SettlementState, WithdrawalPhase } from "@/generated/bridge.did"

const depositNames = ["PullPending", "Escrowed", "MintPending", "Minted", "MintReverted", "ReconciliationHold", "Cancelled"] as const
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
    PullPending: "Starting",
    Escrowed: "Processing",
    MintPending: "Processing",
    Minted: "Complete",
    MintReverted: "Needs attention",
    ReconciliationHold: "On hold",
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
  return name === "Minted" || name === "MintReverted" || name === "Cancelled"
}

export function isWithdrawalTerminal(phase: WithdrawalPhase): boolean {
  const name = variantName(phase, withdrawalNames)
  return name === "Paid"
}

export function depositPhaseTone(phase: DepositPhase): "good" | "warn" | "neutral" {
  const name = variantName(phase, depositNames)
  if (name === "Minted") return "good"
  return isDepositTerminal(phase) ? "warn" : "neutral"
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

function isTransactionHash(value: unknown): boolean {
  if (!(value instanceof Uint8Array) && !Array.isArray(value)) return false
  return value.length === 32 && Array.from(value).every((byte) => Number.isInteger(byte) && byte >= 0 && byte <= 255)
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
    "RpcUnavailable", "TransactionNotConfirmed",
    "RpcInconsistent", "LedgerAmbiguous", "LedgerUnavailable", "NonceConflict", "NonceUnavailable",
    "TransactionReverted", "NonceBlocked", "BaseStateMismatch", "TransactionNotFound",
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
  if (key === "Stopped") return hasExactKeys(record, ["state", "reason"]) && isSettlementStopReason(record.reason)
  if (key === "Submitted" || key === "WaitingForConfirmation") {
    return hasExactKeys(record, ["state", "transaction_hash"]) && isTransactionHash(record.transaction_hash)
  }
  return false
}
