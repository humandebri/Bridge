export type WithdrawalFinalizationDecision = "retry" | "notify" | "discard-reverted"

export function decideWithdrawalFinalization(
  receiptStatus: "success" | "reverted",
  receiptBlockNumber: bigint,
  finalizedBlockNumber: bigint | null,
): WithdrawalFinalizationDecision {
  if (finalizedBlockNumber === null || finalizedBlockNumber < receiptBlockNumber) return "retry"
  return receiptStatus === "reverted" ? "discard-reverted" : "notify"
}
