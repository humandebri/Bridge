import type { NotifyWithdrawalReceipt } from "@/generated/bridge.did"

export interface WithdrawalNotificationPresentation {
  tone: "success" | "info" | "warning"
  message: string
}

export function withdrawalNotificationPresentation(receipt: NotifyWithdrawalReceipt): WithdrawalNotificationPresentation {
  const duplicate = "Duplicate" in receipt
  const value = duplicate ? receipt.Duplicate : receipt.Ingested
  const settlement = value.settlement[0]

  if (settlement && "Stopped" in settlement) {
    return { tone: "warning", message: "Withdrawal is recorded but needs attention in History." }
  }
  if (settlement && "Complete" in settlement) {
    return { tone: "success", message: "Withdrawal is recorded and the transfer completed." }
  }
  if (settlement) {
    return { tone: "info", message: "Withdrawal is recorded and processing will continue automatically." }
  }
  if (duplicate) {
    return { tone: "info", message: "Withdrawal was already recorded. Check History for its current status." }
  }
  return { tone: "info", message: "Withdrawal is recorded. Processing will continue automatically." }
}
