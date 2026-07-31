import { useEffect } from "react"
import { hexToBytes } from "viem"
import { toast } from "sonner"
import { useIcWallet } from "@/features/wallet/ic-wallet-provider"
import { basePublicClient } from "@/lib/evm/client"
import {
  readPendingConfirmations,
  removePendingConfirmation,
  type PendingConfirmation,
} from "@/lib/pending-confirmations"
import { withdrawalNotificationPresentation } from "@/lib/withdrawal-notification"

export const CONFIRMATION_POLL_MS = 15_000

type PendingWithdrawal = Extract<PendingConfirmation, { kind: "withdrawal" }>

/**
 * Withdrawal-only coordinator. Deposit mint receipts are intentionally never
 * sent back to the canister; expiry reconciliation uses finalized Base state.
 */
export function SettlementConfirmationCoordinator() {
  const ic = useIcWallet()

  useEffect(() => {
    const owner = ic.account?.owner
    if (!ic.adapter || !owner || ic.adapter.requiresUserGesture) return
    let active = true
    const running = new Set<string>()

    const tick = () => {
      if (!active || document.visibilityState !== "visible") return
      const entries = readPendingConfirmations().filter(
        (entry): entry is PendingWithdrawal =>
          entry.kind === "withdrawal" && !entry.blocked && entry.owner === owner,
      )
      for (const entry of entries) {
        if (running.has(entry.transactionHash)) continue
        running.add(entry.transactionHash)
        void finalizeWithdrawal(entry, ic.adapter!)
          .catch(() => undefined)
          .finally(() => running.delete(entry.transactionHash))
      }
    }

    const interval = window.setInterval(tick, CONFIRMATION_POLL_MS)
    document.addEventListener("visibilitychange", tick)
    tick()
    return () => {
      active = false
      window.clearInterval(interval)
      document.removeEventListener("visibilitychange", tick)
    }
  }, [ic.account?.owner, ic.adapter])

  return null
}

async function finalizeWithdrawal(entry: PendingWithdrawal, adapter: NonNullable<ReturnType<typeof useIcWallet>["adapter"]>) {
  const [receipt, finalized] = await Promise.all([
    basePublicClient.getTransactionReceipt({ hash: entry.transactionHash }),
    basePublicClient.getBlock({ blockTag: "finalized" }),
  ])
  if (receipt.status === "reverted") {
    await removePendingConfirmation(entry)
    toast.warning("The Base withdrawal transaction reverted. You can try again.")
    return
  }
  if (finalized.number === null || finalized.number < receipt.blockNumber) return
  const notified = await adapter.notifyWithdrawal(hexToBytes(entry.transactionHash))
  await removePendingConfirmation(entry)
  const presentation = withdrawalNotificationPresentation(notified)
  if (presentation.tone === "success") toast.success(presentation.message)
  else if (presentation.tone === "warning") toast.warning(presentation.message)
  else toast.info(presentation.message)
}
