import type { FinalizedRuntimeObservation } from "@/lib/runtime-validation"
import type { DepositCall, IcAccount } from "@/lib/ic/wallet"

export type BridgeDirection = "deposit" | "withdraw"

export interface UnresolvedDepositAttempt {
  call: DepositCall
  account: IcAccount
  recipient: `0x${string}`
}

export type DepositProgress = "idle" | "checking" | "oisy-action" | "authorization"

export interface DepositWriteGate {
  base: NonNullable<FinalizedRuntimeObservation["snapshot"]>
  ledger: { balance: bigint; fee: bigint; allowance: bigint }
  sequence: bigint
  observation: FinalizedRuntimeObservation
}

export interface ReviewedDeposit {
  amount: bigint
  account: IcAccount
  recipient: `0x${string}`
  gate: DepositWriteGate
}

export type PreflightCheckId = "wallets" | "runtime" | "financials" | "availability"
export type PreflightCheckStatus = "waiting" | "checking" | "passed" | "failed"
export type PreflightPhase = "checking" | "ready" | "failed"
export interface PreflightCheck {
  id: PreflightCheckId
  label: string
  status: PreflightCheckStatus
  error?: string
}
export interface PreflightState {
  runId: number
  direction: BridgeDirection
  phase: PreflightPhase
  checks: PreflightCheck[]
}

export type ReviewState =
  | { status: "closed" }
  | { status: "checking" | "failed"; preflight: PreflightState }
  | {
      status: "ready"
      preflight: PreflightState
      direction: "deposit"
      mode: "new"
      deposit: ReviewedDeposit
      observation: FinalizedRuntimeObservation
      approvalNeeded: boolean
    }
  | {
      status: "ready"
      preflight: PreflightState
      direction: "deposit"
      mode: "retry"
      observation: FinalizedRuntimeObservation
      approvalNeeded: false
    }
  | {
      status: "ready"
      preflight: PreflightState
      direction: "withdraw"
      account: IcAccount
      observation: FinalizedRuntimeObservation
      approvalNeeded: boolean
    }

export type DepositRecoveryState =
  | { status: "resolving" }
  | { status: "clear"; owner?: string }
  | { status: "unresolved"; owner: string; attempt: UnresolvedDepositAttempt; checking: boolean }

export type DepositExecutionState =
  | { status: "idle" }
  | { status: "checking" }
  | { status: "oisy-action" }
  | { status: "authorization"; active: { owner: string; sequence: bigint } }

export interface BridgePageState {
  depositAmount: string
  withdrawAmount: string
  review: ReviewState
  depositRecovery: DepositRecoveryState
  depositExecution: DepositExecutionState
  withdrawal: { status: "idle" | "submitting" }
}

export const initialBridgePageState: BridgePageState = {
  depositAmount: "",
  withdrawAmount: "",
  review: { status: "closed" },
  depositRecovery: { status: "resolving" },
  depositExecution: { status: "idle" },
  withdrawal: { status: "idle" },
}

export type BridgePageEvent =
  | { type: "amount-changed"; direction: BridgeDirection; value: string }
  | { type: "review-started"; preflight: PreflightState }
  | {
      type: "preflight-check-updated"
      runId: number
      id: PreflightCheckId
      status: PreflightCheckStatus
      error?: string
    }
  | {
      type: "deposit-review-ready"
      runId: number
      observation: FinalizedRuntimeObservation
      approvalNeeded: boolean
      deposit?: ReviewedDeposit
    }
  | {
      type: "withdraw-review-ready"
      runId: number
      account: IcAccount
      observation: FinalizedRuntimeObservation
      approvalNeeded: boolean
    }
  | { type: "review-closed" }
  | { type: "deposit-intent-resolved"; owner?: string; attempt?: UnresolvedDepositAttempt }
  | { type: "deposit-intent-saved"; attempt: UnresolvedDepositAttempt }
  | { type: "deposit-intent-check-started"; owner: string }
  | { type: "deposit-intent-check-finished"; owner: string }
  | { type: "deposit-intent-cleared"; owner?: string }
  | { type: "deposit-progress-changed"; progress: Exclude<DepositProgress, "authorization"> }
  | { type: "deposit-accepted"; owner: string; sequence: bigint }
  | { type: "deposit-terminal-reset"; owner: string; sequence: bigint }
  | { type: "withdrawal-submission-started" }
  | { type: "withdrawal-submission-finished"; clearAmount?: boolean }

function currentPreflight(review: ReviewState, runId: number): PreflightState | undefined {
  if (review.status === "closed" || review.preflight.runId !== runId) return undefined
  return review.preflight
}

export function bridgePageReducer(state: BridgePageState, event: BridgePageEvent): BridgePageState {
  switch (event.type) {
    case "amount-changed":
      return event.direction === "deposit"
        ? { ...state, depositAmount: event.value }
        : { ...state, withdrawAmount: event.value }
    case "review-started":
      return { ...state, review: { status: "checking", preflight: event.preflight } }
    case "preflight-check-updated": {
      const preflight = currentPreflight(state.review, event.runId)
      if (!preflight || state.review.status === "ready") return state
      const nextPreflight = {
        ...preflight,
        phase: event.status === "failed" ? ("failed" as const) : preflight.phase,
        checks: preflight.checks.map((check) =>
          check.id === event.id ? { ...check, status: event.status, error: event.error } : check,
        ),
      }
      return {
        ...state,
        review: {
          status: event.status === "failed" ? "failed" : state.review.status,
          preflight: nextPreflight,
        },
      }
    }
    case "deposit-review-ready": {
      const preflight = currentPreflight(state.review, event.runId)
      if (!preflight || state.review.status === "ready" || preflight.direction !== "deposit")
        return state
      const readyPreflight = { ...preflight, phase: "ready" as const }
      return {
        ...state,
        review: event.deposit
          ? {
              status: "ready",
              preflight: readyPreflight,
              direction: "deposit",
              mode: "new",
              deposit: event.deposit,
              observation: event.observation,
              approvalNeeded: event.approvalNeeded,
            }
          : {
              status: "ready",
              preflight: readyPreflight,
              direction: "deposit",
              mode: "retry",
              observation: event.observation,
              approvalNeeded: false,
            },
      }
    }
    case "withdraw-review-ready": {
      const preflight = currentPreflight(state.review, event.runId)
      if (!preflight || state.review.status === "ready" || preflight.direction !== "withdraw")
        return state
      return {
        ...state,
        review: {
          status: "ready",
          preflight: { ...preflight, phase: "ready" },
          direction: "withdraw",
          account: event.account,
          observation: event.observation,
          approvalNeeded: event.approvalNeeded,
        },
      }
    }
    case "review-closed":
      return { ...state, review: { status: "closed" } }
    case "deposit-intent-resolved":
      return {
        ...state,
        depositRecovery: event.attempt
          ? {
              status: "unresolved",
              owner: event.owner ?? event.attempt.account.owner,
              attempt: event.attempt,
              checking: false,
            }
          : { status: "clear", owner: event.owner },
      }
    case "deposit-intent-saved":
      return {
        ...state,
        depositRecovery: {
          status: "unresolved",
          owner: event.attempt.account.owner,
          attempt: event.attempt,
          checking: false,
        },
      }
    case "deposit-intent-check-started":
      return state.depositRecovery.status === "unresolved" &&
        state.depositRecovery.attempt.account.owner === event.owner
        ? { ...state, depositRecovery: { ...state.depositRecovery, checking: true } }
        : state
    case "deposit-intent-check-finished":
      return state.depositRecovery.status === "unresolved" &&
        state.depositRecovery.attempt.account.owner === event.owner
        ? { ...state, depositRecovery: { ...state.depositRecovery, checking: false } }
        : state
    case "deposit-intent-cleared":
      return {
        ...state,
        depositRecovery: { status: "clear", owner: event.owner },
      }
    case "deposit-progress-changed":
      return {
        ...state,
        depositExecution: { status: event.progress },
      }
    case "deposit-accepted":
      return {
        ...state,
        depositRecovery: { status: "clear", owner: event.owner },
        depositExecution: {
          status: "authorization",
          active: { owner: event.owner, sequence: event.sequence },
        },
      }
    case "deposit-terminal-reset":
      if (
        state.depositExecution.status !== "authorization" ||
        state.depositExecution.active.owner !== event.owner ||
        state.depositExecution.active.sequence !== event.sequence
      )
        return state
      return {
        ...state,
        depositAmount: "",
        review: { status: "closed" },
        depositExecution: { status: "idle" },
      }
    case "withdrawal-submission-started":
      return { ...state, withdrawal: { status: "submitting" } }
    case "withdrawal-submission-finished":
      return {
        ...state,
        withdrawAmount: event.clearAmount ? "" : state.withdrawAmount,
        withdrawal: { status: "idle" },
      }
  }
}
