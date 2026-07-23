export type DepositRecoverySequenceStatus = "not-accepted" | "accepted-or-conflicted" | "invalid"

export function classifyDepositRecoverySequence(attemptSequence: bigint, nextSequence: bigint | undefined): DepositRecoverySequenceStatus {
  if (nextSequence === undefined) return "invalid"
  if (nextSequence === attemptSequence) return "not-accepted"
  if (nextSequence > attemptSequence) return "accepted-or-conflicted"
  return "invalid"
}
