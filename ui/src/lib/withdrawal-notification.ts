import type { NotifyWithdrawalReceipt } from "@/generated/bridge.did"

export interface WithdrawalNotificationPresentation {
  tone: "success" | "info" | "warning"
  message: string
}

export function withdrawalNotificationPresentation(
  receipt: NotifyWithdrawalReceipt,
): WithdrawalNotificationPresentation {
  const duplicate = "Duplicate" in receipt
  if (duplicate) {
    return {
      tone: "info",
      message: "Withdrawal was already recorded. Check History for its current status.",
    }
  }
  return { tone: "info", message: "Withdrawal is recorded. One payout step will now be attempted." }
}
