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
  markPendingConfirmationNotificationAttempt,
  markPendingConfirmationNotified,
  removePendingConfirmation,
  setPendingConfirmationNotificationFailure,
  type PendingConfirmation,
  type PendingNotificationFailure,
} from "@/lib/pending-confirmations"
import { withdrawalNotificationPresentation } from "@/lib/withdrawal-notification"

export const CONFIRMATION_POLL_MS = 15_000
export const NOTIFICATION_RETRY_DELAY_MS = 5_000

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
  const setAction = bridgeProgress.setAction

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
    const recoverableProgressFor = (entry: PendingWithdrawal) => {
      const current = matchingProgressFor(entry)
      return current?.phase === "complete" ? undefined : current
    }
    const progressForTrigger = (entry: PendingWithdrawal, trigger: "automatic" | "manual") => {
      return trigger === "manual" ? recoverableProgressFor(entry) : activeProgressFor(entry)
    }
    const presentationIsCurrent = (progressId: string | undefined, entry: PendingWithdrawal) => {
      return isCurrent() && (progressId === undefined || activeProgressFor(entry)?.id === progressId)
    }

    const tick = () => {
      if (!isCurrent() || document.visibilityState !== "visible") return
      const latest = progressRef.current
      const entries = readPendingConfirmations().filter((entry): entry is PendingWithdrawal => {
        if (entry.kind !== "withdrawal") return false
        const ownsLatest = latest?.direction === "withdraw"
          && latest.transactionHash?.toLowerCase() === entry.transactionHash.toLowerCase()
        if (entry.notification.status === "awaiting-notification"
          && ownsLatest
          && latest.phase === "complete") return false
        return true
      })
      for (const entry of entries) {
        if (entry.notification.status === "awaiting-notification") {
          const failure = entry.notification.failure
          if (failure?.disposition === "manual-retry" || failure?.disposition === "terminal") {
            presentNotificationFailure(entry, failure, recoverableProgressFor(entry)?.id, update, setAction, observeWithdrawal)
            continue
          }
          if (entry.notification.automaticAttemptUsed && !failure) {
            const interrupted = {
              code: "Interrupted",
              message: "The automatic IC notification was interrupted. Retry it explicitly.",
              disposition: "manual-retry" as const,
            }
            void setPendingConfirmationNotificationFailure(entry, interrupted)
            presentNotificationFailure(entry, interrupted, recoverableProgressFor(entry)?.id, update, setAction, observeWithdrawal)
            continue
          }
          if (matchingProgressFor(entry)?.phase === "attention") continue
        }
        const transactionKey = entry.transactionHash.toLowerCase()
        if (runningRef.current.has(transactionKey)) continue
        runningRef.current.add(transactionKey)
        void observeWithdrawal(entry, activeProgressFor(entry)?.id, "automatic")
          .catch(() => undefined)
          .finally(() => {
            runningRef.current.delete(transactionKey)
            if (!isCurrent()) window.queueMicrotask(() => tickRef.current())
          })
      }
    }

    const observeWithdrawal = async (
      entry: PendingWithdrawal,
      observedProgressId: string | undefined,
      trigger: "automatic" | "manual",
    ) => {
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
      if (observedProgressId && progressForTrigger(entry, trigger)?.id !== observedProgressId) return
      let latest = progressForTrigger(entry, trigger)
      if (receipt.status === "reverted") {
        await removePendingConfirmation(entry)
        if (!isCurrent()) return
        if (observedProgressId && progressForTrigger(entry, trigger)?.id !== observedProgressId) return
        latest = progressForTrigger(entry, trigger)
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
      if (observedProgressId && progressForTrigger(entry, trigger)?.id !== observedProgressId) return
      latest = progressForTrigger(entry, trigger)
      if (finalized.number === null || finalized.number < receipt.blockNumber) {
        if (latest) update(latest.id, {
          phase: "base-withdrawal-finalizing",
          finalizedBlockNumber: finalized.number?.toString(),
        })
        return
      }

      const refreshed = readPendingConfirmations().find((candidate) => candidate.kind === "withdrawal"
        && candidate.transactionHash.toLowerCase() === entry.transactionHash.toLowerCase())
      if (!refreshed || refreshed.notification.status !== "awaiting-notification") return
      let attemptKind: "automatic" | "manual" | "finality-readvance"
      if (trigger === "manual") {
        attemptKind = "manual"
      } else if (refreshed.notification.failure?.disposition === "finality-wait") {
        const previousBlock = refreshed.notification.lastAttemptedFinalizedBlock === undefined
          ? undefined
          : BigInt(refreshed.notification.lastAttemptedFinalizedBlock)
        if (refreshed.notification.finalityReadvanceUsed || previousBlock === undefined || finalized.number <= previousBlock) return
        attemptKind = "finality-readvance"
      } else if (!refreshed.notification.automaticAttemptUsed) {
        attemptKind = "automatic"
      } else {
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
        if (latest && trigger === "manual") setAction(latest.id, undefined)
        await markPendingConfirmationNotificationAttempt(refreshed, attemptKind, finalized.number).catch(() => undefined)
        await notifyWithdrawalWithRetry(refreshed, finalized.number, latest?.id, update, presentationIsCurrent, isCurrent)
      } catch (error) {
        if (!isCurrent()) return
        const latestEntry = readPendingConfirmations().find((candidate) => candidate.kind === "withdrawal"
          && candidate.transactionHash.toLowerCase() === entry.transactionHash.toLowerCase())
        const automaticRetryExhausted = attemptKind === "manual"
          || attemptKind === "finality-readvance"
          || (latestEntry?.notification.status === "awaiting-notification" && latestEntry.notification.shortRetryUsed)
        const failure = notificationFailure(error, automaticRetryExhausted)
        await setPendingConfirmationNotificationFailure(latestEntry ?? refreshed, failure).catch(() => undefined)
        presentNotificationFailure(latestEntry ?? refreshed, failure, latest?.id, update, setAction, observeWithdrawal)
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
  }, [setAction, update])

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

async function notifyWithdrawalWithRetry(
  entry: PendingWithdrawal,
  finalizedBlock: bigint,
  progressId: string | undefined,
  update: ReturnType<typeof useBridgeProgress>["update"],
  presentationIsCurrent: (progressId: string | undefined, entry: PendingWithdrawal) => boolean,
  isCurrent: () => boolean,
) {
  try {
    return await notifyWithdrawal(entry, progressId, update, presentationIsCurrent)
  } catch (error) {
    const current = readPendingConfirmations().find((candidate) => candidate.kind === "withdrawal"
      && candidate.transactionHash.toLowerCase() === entry.transactionHash.toLowerCase())
    const canRetry = current?.notification.status === "awaiting-notification"
      && !current.notification.shortRetryUsed
      && notificationAllowsShortRetry(error)
    if (!canRetry) throw error
    await delay(NOTIFICATION_RETRY_DELAY_MS)
    if (!isCurrent()) return
    await markPendingConfirmationNotificationAttempt(current, "short-retry", finalizedBlock).catch(() => undefined)
    return notifyWithdrawal(current, progressId, update, presentationIsCurrent)
  }
}

function bytesHex(bytes: Uint8Array | number[]): `0x${string}` {
  return `0x${Array.from(bytes, (value) => Number(value).toString(16).padStart(2, "0")).join("")}`
}

function shortAddress(value: string): string {
  return value.length > 18 ? `${value.slice(0, 10)}…${value.slice(-6)}` : value
}

function notificationAllowsShortRetry(error: unknown): boolean {
  return error instanceof NotifyWithdrawalCallError ? error.code === "Busy" : true
}

function notificationFailure(error: unknown, automaticRetryExhausted: boolean): PendingNotificationFailure {
  const message = error instanceof Error ? error.message : "The IC notification failed."
  if (error instanceof NotifyWithdrawalCallError) {
    if (error.code === "TransactionNotConfirmed") return {
      code: error.code,
      message: automaticRetryExhausted
        ? "Base finality could not be confirmed within the automatic retry budget. Retry the IC notification explicitly."
        : message,
      disposition: automaticRetryExhausted ? "manual-retry" : "finality-wait",
    }
    if ([
      "AnonymousCaller",
      "BaseStateMismatch",
      "BridgeSignerMismatch",
      "InvalidTransactionHash",
      "LedgerFeeExceedsServiceFee",
      "TransactionReverted",
      "WithdrawalConflict",
    ].includes(error.code)) return { code: error.code, message, disposition: "terminal" }
    return { code: error.code, message, disposition: "manual-retry" }
  }
  if (/conflict|reverted|invalid transaction hash|bridge signer mismatch|base state mismatch/i.test(message)) {
    return { code: "TerminalNotificationError", message, disposition: "terminal" }
  }
  return { code: "TransportError", message, disposition: "manual-retry" }
}

function presentNotificationFailure(
  entry: PendingWithdrawal,
  failure: PendingNotificationFailure,
  progressId: string | undefined,
  update: ReturnType<typeof useBridgeProgress>["update"],
  setAction: ReturnType<typeof useBridgeProgress>["setAction"],
  observeWithdrawal: (entry: PendingWithdrawal, progressId: string | undefined, trigger: "automatic" | "manual") => Promise<void>,
) {
  if (!progressId) return
  if (failure.disposition === "finality-wait") {
    update(progressId, { phase: "base-withdrawal-finalizing", attentionMessage: undefined })
    return
  }
  update(progressId, { phase: "attention", attentionMessage: failure.message })
  if (failure.disposition === "manual-retry") {
    setAction(progressId, {
      label: "Retry IC notification",
      run: () => observeWithdrawal(entry, progressId, "manual"),
    })
  }
}

function delay(milliseconds: number): Promise<void> {
  return new Promise((resolve) => window.setTimeout(resolve, milliseconds))
}
