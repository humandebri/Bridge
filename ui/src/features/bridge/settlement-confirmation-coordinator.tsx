import { useEffect } from "react"
import { hexToBytes } from "viem"
import { toast } from "sonner"
import { useIcWallet } from "@/features/wallet/ic-wallet-provider"
import type { SettlementActionResult } from "@/generated/bridge.did"
import { basePublicClient } from "@/lib/evm/client"
import { NotifyWithdrawalCallError, SettlementActionCallError, type IcWalletAdapter } from "@/lib/ic/wallet"
import {
  PENDING_CONFIRMATIONS_CHANGED,
  pendingConfirmationKey,
  readPendingConfirmations,
  removePendingConfirmation,
  savePendingConfirmation,
  setPendingConfirmationBlocked,
  type PendingConfirmation,
} from "@/lib/pending-confirmations"
import { withdrawalNotificationPresentation } from "@/lib/withdrawal-notification"

export const CONFIRMATION_POLL_MS = 15_000

export type ConfirmationOutcome =
  | { status: "retry"; retryAt?: number }
  | { status: "complete" }
  | { status: "blocked" }

export function SettlementConfirmationCoordinator() {
  const ic = useIcWallet()

  useEffect(() => {
    const owner = ic.account?.owner
    if (!ic.adapter || !owner) return

    let active = true
    const running = new Set<string>()
    const retryAt = new Map<string, number>()

    const tick = () => {
      if (!active || document.visibilityState !== "visible") return
      const now = Date.now()
      const entries = readPendingConfirmations().filter((entry) => !entry.blocked && entry.owner === owner)
      for (const entry of entries) {
        const key = pendingConfirmationKey(entry)
        if (running.has(key) || (retryAt.get(key) ?? 0) > now) continue
        running.add(key)
        void confirmWhenFinalized(
          entry,
          ic.adapter!,
          () => active && document.visibilityState === "visible",
        )
          .then((outcome) => {
            if (!active) return
            if (outcome.status === "retry") retryAt.set(key, outcome.retryAt ?? Date.now() + CONFIRMATION_POLL_MS)
            else retryAt.delete(key)
          })
          .finally(() => {
            running.delete(key)
          })
      }
    }

    const wake = () => tick()
    const interval = window.setInterval(tick, CONFIRMATION_POLL_MS)
    window.addEventListener(PENDING_CONFIRMATIONS_CHANGED, wake)
    window.addEventListener("storage", wake)
    document.addEventListener("visibilitychange", wake)
    tick()

    return () => {
      active = false
      window.clearInterval(interval)
      window.removeEventListener(PENDING_CONFIRMATIONS_CHANGED, wake)
      window.removeEventListener("storage", wake)
      document.removeEventListener("visibilitychange", wake)
    }
  }, [ic.account?.owner, ic.adapter])

  return null
}

export async function confirmWhenFinalized(
  entry: PendingConfirmation,
  adapter: IcWalletAdapter,
  isActive: () => boolean = () => true,
): Promise<ConfirmationOutcome> {
  let receipt: Awaited<ReturnType<typeof basePublicClient.getTransactionReceipt>>
  let finalized: Awaited<ReturnType<typeof basePublicClient.getBlock>>
  try {
    receipt = await basePublicClient.getTransactionReceipt({ hash: entry.transactionHash })
    finalized = await basePublicClient.getBlock({ blockTag: "finalized" })
  } catch {
    return { status: "retry" }
  }
  if (!isActive()) return { status: "retry" }
  if (finalized.number === null || finalized.number < receipt.blockNumber) return { status: "retry" }

  try {
    toast.info("Base transaction is finalized. Review the IC wallet confirmation.")
    if (entry.kind === "withdrawal") {
      const receipt = await adapter.notifyWithdrawal(hexToBytes(entry.transactionHash))
      removePendingConfirmation(entry)
      toastWithdrawalNotification(receipt)
      return { status: "complete" }
    }
    const result = await adapter.confirmDeposit({
      settlementId: hexToBytes(entry.settlementId),
      transactionHash: hexToBytes(entry.transactionHash),
      receiptBlockNumber: receipt.blockNumber,
      observedFinalizedBlockNumber: finalized.number,
    })
    return handleDepositResult(entry, result)
  } catch (error) {
    if (!isActive()) return { status: "retry" }
    if (isRetryableConfirmationError(error)) {
      return { status: "retry", retryAt: error instanceof SettlementActionCallError ? error.retryAt : undefined }
    }
    setPendingConfirmationBlocked(entry, true)
    toast.warning("Confirmation was not completed. Resume it from History.")
    return { status: "blocked" }
  }
}

export function isRetryableConfirmationError(error: unknown): boolean {
  if (error instanceof SettlementActionCallError) {
    return ["Busy", "AutomaticProgressPending", "RateLimited", "StorageFailure"].includes(error.code)
  }
  if (error instanceof NotifyWithdrawalCallError) {
    return ["Busy", "RpcUnavailable", "RpcInconsistent", "TransactionNotFound", "TransactionNotConfirmed", "LedgerFeeUnavailable", "StorageFailure"].includes(error.code)
  }
  if (!(error instanceof Error)) return true
  return !/reject|declin|denied|cancel|account changed|does not own|reverted|payload already|hash is invalid|state does not match|signer does not match|connect (?:an ic wallet|oisy|plug)|not connected|not installed|reconnect|unauthorized|invalid .*reply|wallet reply.*invalid|response .*mismatch|certifi/i.test(error.message)
}

function toastWithdrawalNotification(receipt: Awaited<ReturnType<IcWalletAdapter["notifyWithdrawal"]>>): void {
  const presentation = withdrawalNotificationPresentation(receipt)
  if (presentation.tone === "success") toast.success(presentation.message)
  else if (presentation.tone === "warning") toast.warning(presentation.message)
  else toast.info(presentation.message)
}

function handleDepositResult(entry: Extract<PendingConfirmation, { kind: "deposit" }>, result: SettlementActionResult): ConfirmationOutcome {
  if ("Submitted" in result) {
    savePendingConfirmation({ ...entry, transactionHash: bytesHex(result.Submitted.transaction_hash), blocked: false })
    toast.success("The next Base transaction was submitted and is being monitored.")
    return { status: "retry" }
  }
  if ("WaitingForConfirmation" in result) return { status: "retry" }
  removePendingConfirmation(entry)
  if ("Stopped" in result) toast.warning("Settlement stopped and needs attention in History.")
  else toast.success("Base confirmation was verified by the bridge.")
  return { status: "complete" }
}

function bytesHex(bytes: Uint8Array | number[]): `0x${string}` {
  return `0x${Array.from(bytes, (value) => Number(value).toString(16).padStart(2, "0")).join("")}`
}
