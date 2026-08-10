import { describe, expect, it } from "vitest"
import type { NotifyWithdrawalReceipt } from "@/generated/bridge.did"
import { withdrawalNotificationPresentation } from "./withdrawal-notification"

describe("withdrawal notification presentation", () => {
  it("does not claim that a duplicate started a transfer", () => {
    const receipt: NotifyWithdrawalReceipt = { Duplicate: { withdrawal_id: new Uint8Array(32) } }

    expect(withdrawalNotificationPresentation(receipt)).toEqual({
      tone: "info",
      message: "Withdrawal was already recorded. Check History for its current status.",
    })
  })

  it("reports that one payout step follows a new notification", () => {
    const receipt: NotifyWithdrawalReceipt = {
      Ingested: {
        finalized_head_block_number: 42n,
        withdrawal_id: new Uint8Array(32),
      },
    }

    expect(withdrawalNotificationPresentation(receipt)).toEqual({
      tone: "info",
      message: "Withdrawal is recorded. One payout step will now be attempted.",
    })
  })
})
