import {
  assertTransferActionAllowed,
  readTransferFacts,
  initialTransferFacts,
  reduceTransfer,
  publishTransferFacts,
} from "./transfer-state"
import type { WithdrawalView } from "@/generated/bridge.did"
import { continueWithdrawalWithBrowserIdentity } from "./ic/withdrawal-notification-client"

const payouts = new Map<string, ReturnType<typeof continueWithdrawalWithBrowserIdentity>>()
export function payoutFeeGuardBlocked(record?: WithdrawalView): boolean {
  return Boolean(
    record?.last_settlement_stop_reason[0] &&
    "LedgerFeeExceedsServiceFee" in record.last_settlement_stop_reason[0],
  )
}
/** Runtime and existing canister rules remain authoritative; repeated clicks share one operation. */
export function continueTransferPayout(
  record: WithdrawalView,
  verifyRuntime: () => Promise<unknown>,
  identity: string,
) {
  assertTransferActionAllowed(identity)
  const key = Array.from(record.withdrawal_id, (b) => b.toString(16).padStart(2, "0")).join("")
  const existing = payouts.get(key)
  if (existing) return existing
  const generation = readTransferFacts(identity)?.generation ?? 0
  const run = Promise.resolve()
    .then(async () => {
      if (!payoutFeeGuardBlocked(record)) await verifyRuntime()
      assertTransferActionAllowed(identity)
      const result = await continueWithdrawalWithBrowserIdentity(
        Uint8Array.from(record.withdrawal_id),
      )
      const paid =
        "Complete" in result &&
        "Withdrawal" in result.Complete.state &&
        "Paid" in result.Complete.state.Withdrawal
      const facts =
        readTransferFacts(identity) ?? initialTransferFacts(identity, "withdraw", "ledger-payout")
      publishTransferFacts(
        reduceTransfer(facts, {
          identity,
          generation,
          source: "operation",
          revision: (facts.revisions.operation ?? 0) + 1,
          type: "observed",
          phase: paid ? "complete" : "ledger-payout",
          outcome: paid ? "paid" : undefined,
        }),
      )
      return result
    })
    .finally(() => payouts.delete(key))
  payouts.set(key, run)
  return run
}
