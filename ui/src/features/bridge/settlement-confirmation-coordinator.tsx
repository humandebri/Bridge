import { useEffect, useRef } from "react"
import { hexToBytes } from "viem"
import { toast } from "sonner"
import { deploymentProfile } from "@/config/profile"
import { useBridgeProgress } from "@/features/bridge/bridge-progress-provider"
import { useIcWallet } from "@/features/wallet/ic-wallet-provider"
import { basePublicClient } from "@/lib/evm/client"
import { createBridgeActor } from "@/lib/ic/bridge"
import { withBrowserLock } from "@/lib/browser-lock"
import {
  readPendingConfirmations,
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
  const ic = useIcWallet()
  const bridgeProgress = useBridgeProgress()
  const progressRef = useRef(bridgeProgress.progress)
  const runningRef = useRef(new Set<string>())
  const tickRef = useRef<() => void>(() => undefined)
  const notificationRunsRef = useRef(new Set<string>())
  const completedNotificationsRef = useRef(new Set<string>())
  const observerGenerationRef = useRef(0)
  const registeredActionRef = useRef<{ progressId: string; transactionHash: string } | undefined>(undefined)
  const update = bridgeProgress.update
  const setAction = bridgeProgress.setAction

  useEffect(() => {
    progressRef.current = bridgeProgress.progress
  }, [bridgeProgress.progress])

  useEffect(() => {
    const owner = ic.account?.owner
    const adapter = ic.adapter
    const generation = observerGenerationRef.current + 1
    observerGenerationRef.current = generation
    const isCurrent = () => observerGenerationRef.current === generation
    const clearRegisteredAction = () => {
      const registered = registeredActionRef.current
      if (!registered) return
      setAction(registered.progressId, undefined)
      registeredActionRef.current = undefined
    }
    const activeProgressFor = (entry: PendingWithdrawal) => {
      const current = progressRef.current
      if (current?.direction !== "withdraw"
        || current.transactionHash?.toLowerCase() !== entry.transactionHash.toLowerCase()
        || current.phase === "complete"
        || current.phase === "attention") return undefined
      return current
    }
    const presentationIsCurrent = (progressId: string | undefined, entry: PendingWithdrawal) => {
      return isCurrent() && (progressId === undefined || activeProgressFor(entry)?.id === progressId)
    }

    const tick = () => {
      if (!isCurrent() || document.visibilityState !== "visible") return
      const latest = progressRef.current
      const entries = readPendingConfirmations().filter((entry): entry is PendingWithdrawal => {
        if (entry.kind !== "withdrawal"
          || entry.blocked
          || completedNotificationsRef.current.has(entry.transactionHash.toLowerCase())) return false
        const ownsLatest = latest?.direction === "withdraw"
          && latest.transactionHash?.toLowerCase() === entry.transactionHash.toLowerCase()
        if (ownsLatest && (latest.phase === "complete" || latest.phase === "attention")) return false
        return entry.owner === owner || ownsLatest
      })
      const registered = registeredActionRef.current
      if (registered && !entries.some((entry) => entry.transactionHash.toLowerCase() === registered.transactionHash)) {
        clearRegisteredAction()
      }
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
        if (latest) update(latest.id, { phase: "base-withdrawal-finalizing" })
        return
      }
      if (latest) update(latest.id, { phase: "awaiting-ic-notification" })

      if (!adapter || owner !== entry.owner) return
      if (adapter.requiresUserGesture) {
        if (!latest) return
        const actionProgress = latest
        const runNotification = async () => {
          const transactionKey = entry.transactionHash.toLowerCase()
          if (notificationRunsRef.current.has(transactionKey)) return
          if (!presentationIsCurrent(actionProgress.id, entry)) return
          notificationRunsRef.current.add(transactionKey)
          setAction(actionProgress.id, {
            label: "Confirm with IC wallet",
            pending: true,
            run: runNotification,
          })
          let closeWalletSession: (() => Promise<void>) | undefined
          let shouldRestoreAction = true
          try {
            closeWalletSession = await adapter.prepare()
            if (!presentationIsCurrent(actionProgress.id, entry)) return
            await notifyWithdrawal(entry, adapter, actionProgress.id, update, presentationIsCurrent, completedNotificationsRef.current)
            shouldRestoreAction = false
            if (!presentationIsCurrent(actionProgress.id, entry)) return
            setAction(actionProgress.id, undefined)
            registeredActionRef.current = undefined
          } catch (error) {
            if (notificationFailureIsTerminal(error)) {
              shouldRestoreAction = false
              await setPendingConfirmationBlocked(entry, true).catch(() => undefined)
              if (presentationIsCurrent(actionProgress.id, entry)) {
                setAction(actionProgress.id, undefined)
                registeredActionRef.current = undefined
                update(actionProgress.id, {
                  phase: "attention",
                  attentionMessage: error instanceof Error ? error.message : "The finalized withdrawal identity could not be accepted by the IC.",
                })
              }
            }
            if (presentationIsCurrent(actionProgress.id, entry)) {
              toast.error(error instanceof Error ? error.message : "Withdrawal notification failed")
            }
          } finally {
            await closeWalletSession?.().catch(() => undefined)
            notificationRunsRef.current.delete(transactionKey)
            if (shouldRestoreAction
              && isCurrent()
              && presentationIsCurrent(actionProgress.id, entry)) {
              setAction(actionProgress.id, {
                label: "Confirm with IC wallet",
                pending: false,
                run: runNotification,
              })
            }
          }
        }
        registeredActionRef.current = { progressId: actionProgress.id, transactionHash: entry.transactionHash.toLowerCase() }
        setAction(actionProgress.id, {
          label: "Confirm with IC wallet",
          pending: false,
          run: runNotification,
        })
        return
      }
      try {
        await notifyWithdrawal(entry, adapter, latest?.id, update, presentationIsCurrent, completedNotificationsRef.current)
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
      }
    }

    const interval = window.setInterval(tick, CONFIRMATION_POLL_MS)
    tickRef.current = tick
    document.addEventListener("visibilitychange", tick)
    tick()
    return () => {
      if (observerGenerationRef.current === generation) observerGenerationRef.current += 1
      clearRegisteredAction()
      window.clearInterval(interval)
      document.removeEventListener("visibilitychange", tick)
    }
  }, [ic.account?.owner, ic.adapter, setAction, update])

  const payoutProgress = bridgeProgress.progress
  useEffect(() => {
    const progress = payoutProgress
    const withdrawalId = progress?.direction === "withdraw" ? progress.withdrawal?.withdrawalId : undefined
    if (!progress || !withdrawalId || progress.phase === "complete" || progress.phase === "attention") return
    let active = true
    const tick = async () => {
      try {
        const actor = await createBridgeActor(deploymentProfile.icHost, deploymentProfile.bridgeCanisterId as string)
        const record = await actor.get_withdrawal(hexToBytes(withdrawalId))
        if (!active || !record[0]) return
        if ("Paid" in record[0].state) {
          update(progress.id, { phase: "complete", completionMessage: `${progress.receiveAmount} ${progress.receiveSymbol} was paid to ${shortAddress(progress.destination)}.` })
        } else if ("ReconciliationHold" in record[0].state) {
          update(progress.id, { phase: "attention", attentionMessage: "The withdrawal is recorded but needs reconciliation. Open History to review the available action." })
        } else {
          update(progress.id, { phase: "ledger-payout" })
        }
      } catch {
        // A temporary query failure does not turn a canonical pending payout into a terminal error.
      }
    }
    void tick()
    const interval = window.setInterval(() => void tick(), CONFIRMATION_POLL_MS)
    return () => { active = false; window.clearInterval(interval) }
  }, [payoutProgress, update])

  return null
}

async function notifyWithdrawal(
  entry: PendingWithdrawal,
  adapter: NonNullable<ReturnType<typeof useIcWallet>["adapter"]>,
  progressId: string | undefined,
  update: ReturnType<typeof useBridgeProgress>["update"],
  presentationIsCurrent: (progressId: string | undefined, entry: PendingWithdrawal) => boolean,
  completedNotifications: Set<string>,
) {
  const notified = await withBrowserLock(
    `kinic-wallet-prompt:ic:${entry.owner}`,
    () => adapter.notifyWithdrawal(hexToBytes(entry.transactionHash)),
  )
  completedNotifications.add(entry.transactionHash.toLowerCase())
  const canPresentAfterWallet = presentationIsCurrent(progressId, entry)
  const withdrawalId = "Duplicate" in notified ? notified.Duplicate.withdrawal_id : notified.Ingested.withdrawal_id
  await removePendingConfirmation(entry).catch(() => undefined)
  const canPresent = canPresentAfterWallet
    && presentationIsCurrent(progressId, entry)
  if (!canPresent) return
  if (progressId) update(progressId, {
    phase: "ic-notification-recorded",
    withdrawal: { owner: entry.owner, withdrawalId: bytesHex(withdrawalId) },
  })
  const presentation = withdrawalNotificationPresentation(notified)
  if (presentation.tone === "success") toast.success(presentation.message)
  else if (presentation.tone === "warning") toast.warning(presentation.message)
  else toast.info(presentation.message)
}

function bytesHex(bytes: Uint8Array | number[]): `0x${string}` {
  return `0x${Array.from(bytes, (value) => Number(value).toString(16).padStart(2, "0")).join("")}`
}

function shortAddress(value: string): string {
  return value.length > 18 ? `${value.slice(0, 10)}…${value.slice(-6)}` : value
}

function notificationFailureIsTerminal(error: unknown): boolean {
  return error instanceof Error
    && /conflict|reverted|invalid transaction hash|bridge signer mismatch|base state mismatch/i.test(error.message)
}
