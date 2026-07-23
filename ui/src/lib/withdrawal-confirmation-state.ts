export type WithdrawalFinalizationDecision = "retry" | "notify" | "discard-reverted"
export type NotificationFailureDecision = "retain-pending" | "unhandled"

export function decideWithdrawalFinalization(
  receiptStatus: "success" | "reverted",
  receiptBlockNumber: bigint,
  finalizedBlockNumber: bigint | null,
): WithdrawalFinalizationDecision {
  if (finalizedBlockNumber === null || finalizedBlockNumber < receiptBlockNumber) return "retry"
  return receiptStatus === "reverted" ? "discard-reverted" : "notify"
}

export function decideNotificationFailure(
  kind: "deposit" | "withdrawal",
  errorCode: string,
): NotificationFailureDecision {
  return kind === "withdrawal" && errorCode === "LedgerFeeExceedsServiceFee"
    ? "retain-pending"
    : "unhandled"
}
