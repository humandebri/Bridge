import { useEffect, useRef } from "react"
import { hexToBytes } from "viem"
import { toast } from "sonner"
import { deploymentProfile } from "@/config/profile"
import { useBridgeProgress } from "@/features/bridge/bridge-progress-provider"
import { basePublicClient } from "@/lib/evm/client"
import { createBridgeActor } from "@/lib/ic/bridge"
import { continueWithdrawalWithBrowserIdentity, NotifyWithdrawalCallError, notifyWithdrawalWithBrowserIdentity } from "@/lib/ic/withdrawal-notification-client"
import {
  readPendingConfirmations,
  markPendingConfirmationNotified,
  removePendingConfirmation,
  setPendingConfirmationBlocked,
  type PendingConfirmation,
} from "@/lib/pending-confirmations"
import { withdrawalNotificationPresentation } from "@/lib/withdrawal-notification"

export const CONFIRMATION_POLL_MS = 15_000

type PendingWithdrawal = Extract<PendingConfirmation, { kind: "withdrawal" }>

/**
 * Continues withdrawal observation independently of the active route. Base
 * inclusion, finality, IC notification, and Ledger payout remain distinct facts.
 */
export function SettlementConfirmationCoordinator() {
  const bridgeProgress = useBridgeProgress()
  const progressRef = useRef(bridgeProgress.progress)
  const runningRef = useRef(new Set<string>())
  const tickRef = useRef<() => void>(() => undefined)
  const notificationRunsRef = useRef(new Set<string>())
  const observerGenerationRef = useRef(0)
  const update = bridgeProgress.update

  useEffect(() => {
    progressRef.current = bridgeProgress.progress
  }, [bridgeProgress.progress])

  useEffect(() => {
    const generation = observerGenerationRef.current + 1
    observerGenerationRef.current = generation
    const isCurrent = () => observerGenerationRef.current === generation
    const activeProgressFor = (entry: PendingWithdrawal) => {
      const current = progressRef.current
      if (current?.direction !== "withdraw"
        || current.transactionHash?.toLowerCase() !== entry.transactionHash.toLowerCase()
        || current.phase === "complete"
        || current.phase === "attention") return undefined
      return current
    }
    const matchingProgressFor = (entry: PendingWithdrawal) => {
      const current = progressRef.current
      return current?.direction === "withdraw"
        && current.transactionHash?.toLowerCase() === entry.transactionHash.toLowerCase()
        ? current
        : undefined
    }
    const presentationIsCurrent = (progressId: string | undefined, entry: PendingWithdrawal) => {
      return isCurrent() && (progressId === undefined || activeProgressFor(entry)?.id === progressId)
    }

    const tick = () => {
      if (!isCurrent() || document.visibilityState !== "visible") return
      const latest = progressRef.current
      const entries = readPendingConfirmations().filter((entry): entry is PendingWithdrawal => {
        if (entry.kind !== "withdrawal" || entry.blocked) return false
        const ownsLatest = latest?.direction === "withdraw"
          && latest.transactionHash?.toLowerCase() === entry.transactionHash.toLowerCase()
        if (entry.notification.status === "awaiting-notification"
          && ownsLatest
          && (latest.phase === "complete" || latest.phase === "attention")) return false
        return true
      })
      for (const entry of entries) {
        const transactionKey = entry.transactionHash.toLowerCase()
        if (runningRef.current.has(transactionKey)) continue
        runningRef.current.add(transactionKey)
        void observeWithdrawal(entry, activeProgressFor(entry)?.id)
          .catch(() => undefined)
          .finally(() => {
            runningRef.current.delete(transactionKey)
            if (!isCurrent()) window.queueMicrotask(() => tickRef.current())
          })
      }
    }

    const observeWithdrawal = async (entry: PendingWithdrawal, observedProgressId: string | undefined) => {
      if (entry.notification.status === "notified") {
        const progress = matchingProgressFor(entry)
        const actor = await createBridgeActor(deploymentProfile.icHost, deploymentProfile.bridgeCanisterId as string)
        const record = await actor.get_withdrawal(hexToBytes(entry.notification.withdrawalId))
        if (!isCurrent() || !record[0]) return
        if ("Paid" in record[0].state) {
          if (progress) update(progress.id, {
            phase: "complete",
            withdrawal: { owner: entry.owner, withdrawalId: entry.notification.withdrawalId },
            completionMessage: `${progress.receiveAmount} ${progress.receiveSymbol} was paid to ${shortAddress(progress.destination)}.`,
          })
          await removePendingConfirmation(entry)
        } else if ("ReconciliationHold" in record[0].state) {
          if (progress) update(progress.id, {
            phase: "attention",
            withdrawal: { owner: entry.owner, withdrawalId: entry.notification.withdrawalId },
            attentionMessage: "The withdrawal is recorded but needs reconciliation. Open History to review the available action.",
          })
        } else if (progress) {
          update(progress.id, {
            phase: "ledger-payout",
            withdrawal: { owner: entry.owner, withdrawalId: entry.notification.withdrawalId },
          })
        }
        return
      }
      const receipt = await basePublicClient.getTransactionReceipt({ hash: entry.transactionHash })
      if (!isCurrent()) return
      if (observedProgressId && activeProgressFor(entry)?.id !== observedProgressId) return
      let latest = activeProgressFor(entry)
      if (receipt.status === "reverted") {
        await removePendingConfirmation(entry)
        if (!isCurrent()) return
        if (observedProgressId && activeProgressFor(entry)?.id !== observedProgressId) return
        latest = activeProgressFor(entry)
        if (latest) update(latest.id, {
          phase: "attention",
          receiptBlockNumber: receipt.blockNumber.toString(),
          attentionMessage: "The Base withdrawal transaction reverted. No withdrawal was recorded on the IC; you can close this transfer and try again.",
        })
        toast.warning("The Base withdrawal transaction reverted. You can try again.")
        return
      }
      if (latest) update(latest.id, {
        phase: "base-withdrawal-included",
        receiptBlockNumber: receipt.blockNumber.toString(),
      })
      const finalized = await basePublicClient.getBlock({ blockTag: "finalized" })
      if (!isCurrent()) return
      if (observedProgressId && activeProgressFor(entry)?.id !== observedProgressId) return
      latest = activeProgressFor(entry)
      if (finalized.number === null || finalized.number < receipt.blockNumber) {
        if (latest) update(latest.id, {
          phase: "base-withdrawal-finalizing",
          finalizedBlockNumber: finalized.number?.toString(),
        })
        return
      }
      if (latest) update(latest.id, {
        phase: "awaiting-ic-notification",
        finalizedBlockNumber: finalized.number.toString(),
      })

      const transactionKey = entry.transactionHash.toLowerCase()
      if (notificationRunsRef.current.has(transactionKey)) return
      notificationRunsRef.current.add(transactionKey)
      try {
        await notifyWithdrawal(entry, latest?.id, update, presentationIsCurrent)
      } catch (error) {
        if (!isCurrent()) return
        if (notificationFailureIsTerminal(error)) {
          await setPendingConfirmationBlocked(entry, true).catch(() => undefined)
          if (latest && presentationIsCurrent(latest.id, entry)) update(latest.id, {
            phase: "attention",
            attentionMessage: error instanceof Error ? error.message : "The finalized withdrawal identity could not be accepted by the IC.",
          })
        }
        throw error
      } finally {
        notificationRunsRef.current.delete(transactionKey)
      }
    }

    const interval = window.setInterval(tick, CONFIRMATION_POLL_MS)
    tickRef.current = tick
    document.addEventListener("visibilitychange", tick)
    tick()
    return () => {
      if (observerGenerationRef.current === generation) observerGenerationRef.current += 1
      window.clearInterval(interval)
      document.removeEventListener("visibilitychange", tick)
    }
  }, [update])

  return null
}

async function notifyWithdrawal(
  entry: PendingWithdrawal,
  progressId: string | undefined,
  update: ReturnType<typeof useBridgeProgress>["update"],
  presentationIsCurrent: (progressId: string | undefined, entry: PendingWithdrawal) => boolean,
) {
  const notified = await notifyWithdrawalWithBrowserIdentity(hexToBytes(entry.transactionHash))
  const canPresentAfterNotification = presentationIsCurrent(progressId, entry)
  const withdrawalId = "Duplicate" in notified ? notified.Duplicate.withdrawal_id : notified.Ingested.withdrawal_id
  const withdrawalIdHex = bytesHex(withdrawalId)
  await markPendingConfirmationNotified(entry, withdrawalIdHex)
  if (progressId && canPresentAfterNotification) update(progressId, {
    phase: "ic-notification-recorded",
    withdrawal: { owner: entry.owner, withdrawalId: withdrawalIdHex },
  })
  let continuation: Awaited<ReturnType<typeof continueWithdrawalWithBrowserIdentity>> | undefined
  let continuationError: unknown
  try {
    continuation = await continueWithdrawalWithBrowserIdentity(Uint8Array.from(withdrawalId))
  } catch (error) {
    continuationError = error
  }
  const canPresent = canPresentAfterNotification
    && presentationIsCurrent(progressId, entry)
  if (!canPresent) return
  const presentation = withdrawalNotificationPresentation(notified)
  if (presentation.tone === "success") toast.success(presentation.message)
  else if (presentation.tone === "warning") toast.warning(presentation.message)
  else toast.info(presentation.message)
  if (continuationError) {
    if (progressId) update(progressId, {
      phase: "attention",
      attentionMessage: continuationError instanceof Error ? continuationError.message : "The payout needs another attempt from History.",
    })
    toast.warning("The withdrawal was recorded, but the payout needs another attempt from History.")
  } else if (continuation && !("Complete" in continuation)) {
    if (progressId) update(progressId, {
      phase: "attention",
      attentionMessage: "The payout needs another explicit step from History.",
    })
  }
}

function bytesHex(bytes: Uint8Array | number[]): `0x${string}` {
  return `0x${Array.from(bytes, (value) => Number(value).toString(16).padStart(2, "0")).join("")}`
}

function shortAddress(value: string): string {
  return value.length > 18 ? `${value.slice(0, 10)}…${value.slice(-6)}` : value
}

function notificationFailureIsTerminal(error: unknown): boolean {
  if (error instanceof NotifyWithdrawalCallError) {
    return [
      "AnonymousCaller",
      "BaseStateMismatch",
      "BridgeSignerMismatch",
      "InvalidTransactionHash",
      "LedgerFeeExceedsServiceFee",
      "TransactionReverted",
      "WithdrawalConflict",
    ].includes(error.code)
  }
  return error instanceof Error
    && /conflict|reverted|invalid transaction hash|bridge signer mismatch|base state mismatch/i.test(error.message)
}
