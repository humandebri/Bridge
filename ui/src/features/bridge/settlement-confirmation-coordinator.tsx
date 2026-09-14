import { continueTransferPayout } from "@/lib/transfer-operations"
import { useRuntimeValidation, useRuntimeHeartbeat } from "@/features/status/use-status"
import { refetchRuntimeAttestedWriteReady } from "@/lib/runtime-validation"
import { useChainId } from "wagmi"
import { readBaseReceipt, readBaseBlock } from "@/lib/base-transaction-observation"
import { TransactionEvidenceMismatch, withdrawalReceiptDetails } from "@/lib/transaction-recovery"
import { useEffect, useRef } from "react"
import { hexToBytes, toHex } from "viem"
import { toast } from "sonner"
import { deploymentProfile } from "@/config/profile"
import { useBridgeProgress } from "@/features/bridge/bridge-progress-provider"
import { hasIndependentFinalizedRevertQuorum } from "@/lib/evm/client"
import { finalizedCheckpointMatches } from "@/lib/finalized-checkpoint"
import { createBridgeActor } from "@/lib/ic/bridge"
import {
  continueWithdrawalWithBrowserIdentity,
  NotifyWithdrawalCallError,
  notifyWithdrawalWithBrowserIdentity,
} from "@/lib/ic/withdrawal-notification-client"
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
import { decideWithdrawalFinalization } from "@/lib/withdrawal-confirmation-state"

export const CONFIRMATION_POLL_MS = 15_000
export const NOTIFICATION_RETRY_DELAY_MS = 5_000

type PendingWithdrawal = Extract<PendingConfirmation, { kind: "withdrawal" }>

/**
 * Continues withdrawal observation independently of the active route. Base
 * inclusion, finality, IC notification, and Ledger payout remain distinct facts.
 */
export function SettlementConfirmationCoordinator() {
  const chainId = useChainId()
  const runtime = useRuntimeValidation(chainId, { enabled: false })
  const heartbeat = useRuntimeHeartbeat(chainId, runtime.data, { enabled: false })
  const verifyRuntime = useRef(() =>
    refetchRuntimeAttestedWriteReady(runtime.data, runtime.refetch, heartbeat.refetch),
  )
  useEffect(() => {
    verifyRuntime.current = () =>
      refetchRuntimeAttestedWriteReady(runtime.data, runtime.refetch, heartbeat.refetch)
  }, [runtime.data, runtime.refetch, heartbeat.refetch])
  const bridgeProgress = useBridgeProgress()
  const progressRef = useRef(bridgeProgress.progress)
  const runningRef = useRef(new Set<string>())
  const tickRef = useRef<() => void>(() => undefined)
  const notificationRunsRef = useRef(new Set<string>())
  const notificationRetryAfterRef = useRef(new Map<string, number>())
  const observerGenerationRef = useRef(0)
  const update = bridgeProgress.update
  const setAction = bridgeProgress.setAction
  const completeWithdrawal = bridgeProgress.completeWithdrawal

  useEffect(() => {
    progressRef.current = bridgeProgress.progress
  }, [bridgeProgress.progress])

  useEffect(() => {
    const generation = observerGenerationRef.current + 1
    observerGenerationRef.current = generation
    const isCurrent = () => observerGenerationRef.current === generation
    const activeProgressFor = (entry: PendingWithdrawal) => {
      const current = progressRef.current
      if (
        current?.direction !== "withdraw" ||
        current.transactionHash?.toLowerCase() !== entry.transactionHash.toLowerCase() ||
        (current.withdrawal?.owner ?? current.destination) !== entry.owner ||
        current.phase === "complete" ||
        (current.phase === "attention" && !current.observationError)
      )
        return undefined
      return current
    }
    const matchingProgressFor = (entry: PendingWithdrawal) => {
      const current = progressRef.current
      return current?.direction === "withdraw" &&
        current.transactionHash?.toLowerCase() === entry.transactionHash.toLowerCase() &&
        (current.withdrawal?.owner ?? current.destination) === entry.owner
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
      return (
        isCurrent() &&
        (progressId === undefined || recoverableProgressFor(entry)?.id === progressId)
      )
    }

    const displayedWithdrawalIsCurrent = (
      progress: NonNullable<typeof bridgeProgress.progress>,
    ) => {
      const hash = progress.transactionHash!
      const owner = progress.withdrawal?.owner ?? progress.destination
      const current = progressRef.current
      return (
        isCurrent() &&
        current?.id === progress.id &&
        current.direction === "withdraw" &&
        current.transactionHash?.toLowerCase() === hash.toLowerCase() &&
        (current.withdrawal?.owner ?? current.destination) === owner &&
        current.destination === progress.destination &&
        current.transfer?.generation === progress.transfer?.generation &&
        current.withdrawal?.withdrawalId === progress.withdrawal?.withdrawalId &&
        (current.phase !== "complete" || progress.phase === "complete") &&
        current.transfer?.outcome === progress.transfer?.outcome
      )
    }

    // The shared queue can be removed by a different tab before this tab sees Paid.
    // Reconcile the displayed transfer without recreating work or sending updates to IC.
    const observeUnqueuedWithdrawal = async (
      progress: NonNullable<typeof bridgeProgress.progress>,
    ) => {
      const hash = progress.transactionHash!
      const owner = progress.withdrawal?.owner ?? progress.destination
      const stillCurrent = () => displayedWithdrawalIsCurrent(progress)
      let observationSource: "base" | "ic" = progress.withdrawal?.withdrawalId ? "ic" : "base"
      try {
        let withdrawalId = progress.withdrawal?.withdrawalId
        if (!withdrawalId) {
          const receipt = await readBaseReceipt(hash)
          if (!stillCurrent() || receipt.blockHash === null) return
          const finalized = await readBaseBlock("finalized")
          if (!stillCurrent()) return
          if (
            finalized.number === null ||
            finalized.hash === null ||
            finalized.number < receipt.blockNumber
          ) {
            update(progress.id, {
              phase: "base-withdrawal-finalizing",
              observationSource,
              observationError: undefined,
            })
            return
          }
          const canonical = await finalizedCheckpointMatches({
            finalizedBlock: finalized.number,
            finalizedBlockHash: finalized.hash,
            checkpointBlock: receipt.blockNumber,
            checkpointBlockHash: receipt.blockHash,
            fetchCheckpointBlockHash: async (number) => (await readBaseBlock(number)).hash,
          })
          if (!stillCurrent()) return
          const decision = decideWithdrawalFinalization(
            receipt.status,
            receipt.blockNumber,
            finalized.number,
            canonical,
          )
          if (decision === "retry") {
            update(progress.id, {
              phase: "base-withdrawal-submitted",
              baseTransactionOutcome: undefined,
              receiptBlockNumber: undefined,
            })
            return
          }
          if (decision === "discard-reverted") {
            const agreed = await hasIndependentFinalizedRevertQuorum(hash)
            if (stillCurrent() && agreed)
              update(progress.id, {
                phase: "attention",
                outcome: "reverted",
                observationSource,
                observationError: undefined,
                attentionMessage:
                  "The finalized Base withdrawal transaction reverted. No withdrawal was recorded on the IC; you can close this transfer and try again.",
              })
            return
          }
          const details = await withdrawalReceiptDetails(hash, owner)
          if (!stillCurrent()) return
          withdrawalId = toHex(details.id, { size: 32 })
          update(progress.id, { observationSource: "base", observationError: undefined })
        }
        observationSource = "ic"
        const actor = await createBridgeActor(
          deploymentProfile.icHost,
          deploymentProfile.bridgeCanisterId as string,
        )
        if (!stillCurrent()) return
        const [record] = await actor.get_withdrawal(hexToBytes(withdrawalId))
        if (!stillCurrent()) return
        if (!record) throw new Error("The IC withdrawal record is not available yet. Retrying.")
        if (bytesHex(record.withdrawal_id).toLowerCase() !== withdrawalId.toLowerCase())
          throw new TransactionEvidenceMismatch("The IC record does not match this withdrawal.")
        if ("Paid" in record.state) {
          update(progress.id, { observationSource: "ic", observationError: undefined })
          completeWithdrawal({ transactionHash: hash, owner, withdrawalId })
        } else {
          update(progress.id, {
            phase: "ReconciliationHold" in record.state ? "attention" : "ledger-payout",
            observationSource,
            observationError: undefined,
            withdrawal: { owner, withdrawalId },
            attentionMessage:
              "ReconciliationHold" in record.state ? "Payout needs reconciliation." : undefined,
          })
        }
      } catch (error) {
        if (!stillCurrent()) return
        if (error instanceof TransactionEvidenceMismatch)
          update(progress.id, {
            phase: "attention",
            issue: "conflict",
            observationSource,
            observationError: undefined,
            attentionMessage: error.message,
          })
        else
          update(progress.id, {
            observationSource,
            observationError: "Transfer status could not be refreshed. Retrying.",
          })
      }
    }

    const tick = () => {
      if (!isCurrent()) return
      const latest = progressRef.current
      const entries = readPendingConfirmations().filter((entry): entry is PendingWithdrawal => {
        if (entry.kind !== "withdrawal") return false
        const ownsLatest =
          latest?.direction === "withdraw" &&
          latest.transactionHash?.toLowerCase() === entry.transactionHash.toLowerCase()
        if (
          entry.notification.status === "awaiting-notification" &&
          ownsLatest &&
          latest.phase === "complete"
        )
          return false
        return true
      })
      const displayedHash = latest?.transactionHash?.toLowerCase()
      if (
        latest?.direction === "withdraw" &&
        displayedHash &&
        latest.phase !== "complete" &&
        !latest.transfer?.outcome &&
        !entries.some((entry) => entry.transactionHash.toLowerCase() === displayedHash) &&
        !runningRef.current.has(displayedHash)
      ) {
        runningRef.current.add(displayedHash)
        void observeUnqueuedWithdrawal(latest).finally(() => {
          runningRef.current.delete(displayedHash)
          if (!isCurrent()) window.queueMicrotask(() => tickRef.current())
        })
      }
      for (const entry of entries) {
        const transactionKey = entry.transactionHash.toLowerCase()
        let resumeNotification = false
        if (entry.notification.status === "awaiting-notification") {
          const failure = entry.notification.failure
          if (failure?.disposition === "manual-retry") {
            const retryAfter = notificationRetryAfterRef.current.get(transactionKey)
            if (retryAfter === undefined)
              notificationRetryAfterRef.current.set(transactionKey, Date.now() + 30_000)
            resumeNotification = retryAfter !== undefined && Date.now() >= retryAfter
          }
          if (
            (failure?.disposition === "manual-retry" && !resumeNotification) ||
            failure?.disposition === "terminal"
          ) {
            presentNotificationFailure(
              entry,
              failure,
              recoverableProgressFor(entry)?.id,
              update,
              setAction,
              observeWithdrawal,
            )
            continue
          }
          if (
            matchingProgressFor(entry)?.phase === "attention" &&
            !matchingProgressFor(entry)?.observationError &&
            !resumeNotification
          )
            continue
        }
        if (runningRef.current.has(transactionKey)) continue
        runningRef.current.add(transactionKey)
        const observedProgress = matchingProgressFor(entry)
        if (resumeNotification)
          notificationRetryAfterRef.current.set(transactionKey, Date.now() + 120_000)
        void observeWithdrawal(
          entry,
          (resumeNotification ? recoverableProgressFor(entry) : activeProgressFor(entry))?.id,
          resumeNotification ? "manual" : "automatic",
        )
          .catch(() => {
            if (observedProgress && displayedWithdrawalIsCurrent(observedProgress))
              update(observedProgress.id, {
                observationSource: entry.notification.status === "notified" ? "ic" : "base",
                observationError: "Transfer status could not be refreshed. Retrying.",
              })
          })
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
        const actor = await createBridgeActor(
          deploymentProfile.icHost,
          deploymentProfile.bridgeCanisterId as string,
        )
        const record = await actor.get_withdrawal(hexToBytes(entry.notification.withdrawalId))
        if (!isCurrent() || !record[0]) return
        const presentationCurrent = progress && displayedWithdrawalIsCurrent(progress)
        if ("Paid" in record[0].state) {
          if (presentationCurrent)
            completeWithdrawal({
              transactionHash: entry.transactionHash,
              owner: entry.owner,
              withdrawalId: entry.notification.withdrawalId,
            })
          await removePendingConfirmation(entry)
        } else if ("ReconciliationHold" in record[0].state) {
          if (presentationCurrent)
            update(progress.id, {
              phase: "attention",
              observationSource: "ic",
              observationError: undefined,
              withdrawal: { owner: entry.owner, withdrawalId: entry.notification.withdrawalId },
              attentionMessage: "Payout needs reconciliation.",
            })
        } else if (presentationCurrent) {
          update(progress.id, {
            phase: "ledger-payout",
            observationError: undefined,
            withdrawal: { owner: entry.owner, withdrawalId: entry.notification.withdrawalId },
          })
        }
        if (presentationCurrent && !("Paid" in record[0].state)) {
          const observedRecord = record[0]
          setAction(progress.id, {
            label: "Continue payout",
            run: async () => {
              setAction(progress.id, { label: "Continuing…", pending: true, run: () => {} })
              try {
                const result = await continueTransferPayout(
                  observedRecord,
                  () => verifyRuntime.current(),
                  `withdraw:${entry.transactionHash.toLowerCase()}`,
                )
                if (!isCurrent()) return
                if (
                  "Complete" in result &&
                  "Withdrawal" in result.Complete.state &&
                  "Paid" in result.Complete.state.Withdrawal
                ) {
                  completeWithdrawal({
                    transactionHash: entry.transactionHash,
                    owner: entry.owner,
                    withdrawalId:
                      entry.notification.status === "notified"
                        ? entry.notification.withdrawalId
                        : undefined,
                  })
                  await removePendingConfirmation(entry)
                } else update(progress.id, { phase: "ledger-payout", observationError: undefined })
              } catch (error) {
                if (isCurrent())
                  update(progress.id, {
                    observationError:
                      error instanceof Error ? error.message : "Payout unavailable. Try again.",
                  })
              } finally {
                if (isCurrent()) {
                  setAction(progress.id, undefined)
                  tickRef.current()
                }
              }
            },
          })
        }
        return
      }
      const receipt = await readBaseReceipt(entry.transactionHash).catch((error: unknown) => {
        if (
          error instanceof Error &&
          error.name === "TransactionReceiptNotFoundError" &&
          isCurrent()
        ) {
          const progress = progressForTrigger(entry, trigger)
          if (progress)
            update(progress.id, {
              phase: "base-withdrawal-submitted",
              baseTransactionOutcome: undefined,
              receiptBlockNumber: undefined,
            })
        }
        throw error
      })
      if (!isCurrent()) return
      if (observedProgressId && progressForTrigger(entry, trigger)?.id !== observedProgressId)
        return
      let latest = progressForTrigger(entry, trigger)
      if (receipt.blockHash === null) return
      if (receipt.status === "success") {
        try {
          await withdrawalReceiptDetails(entry.transactionHash, entry.owner)
        } catch (error) {
          if (error instanceof TransactionEvidenceMismatch && latest) {
            update(latest.id, {
              phase: "attention",
              attentionMessage:
                "The Base receipt does not match this withdrawal. Review its transaction hash before continuing.",
            })
          }
          return
        }
      }
      if (latest)
        update(latest.id, {
          phase: "base-withdrawal-included",
          observationError: undefined,
          baseTransactionOutcome: receipt.status,
          receiptBlockNumber: receipt.blockNumber.toString(),
        })
      const finalized = await readBaseBlock("finalized")
      if (!isCurrent()) return
      if (observedProgressId && progressForTrigger(entry, trigger)?.id !== observedProgressId)
        return
      latest = progressForTrigger(entry, trigger)
      if (
        finalized.number === null ||
        finalized.hash === null ||
        finalized.number < receipt.blockNumber
      ) {
        if (latest)
          update(latest.id, {
            phase: "base-withdrawal-finalizing",
            finalizedBlockNumber: finalized.number?.toString(),
          })
        return
      }
      const canonical = await finalizedCheckpointMatches({
        finalizedBlock: finalized.number,
        finalizedBlockHash: finalized.hash,
        checkpointBlock: receipt.blockNumber,
        checkpointBlockHash: receipt.blockHash,
        fetchCheckpointBlockHash: async (blockNumber) => {
          const block = await readBaseBlock(blockNumber)
          return block.hash
        },
      })
      if (!isCurrent()) return
      const decision = decideWithdrawalFinalization(
        receipt.status,
        receipt.blockNumber,
        finalized.number,
        canonical,
      )
      if (decision === "retry") {
        if (!canonical && latest)
          update(latest.id, {
            phase: "base-withdrawal-submitted",
            baseTransactionOutcome: undefined,
          })
        return
      }
      if (decision === "discard-reverted") {
        if (!(await hasIndependentFinalizedRevertQuorum(entry.transactionHash))) return
        if (!isCurrent()) return
        await removePendingConfirmation(entry)
        if (!isCurrent()) return
        if (observedProgressId && progressForTrigger(entry, trigger)?.id !== observedProgressId)
          return
        latest = progressForTrigger(entry, trigger)
        if (latest)
          update(latest.id, {
            phase: "attention",
            outcome: "reverted",
            receiptBlockNumber: receipt.blockNumber.toString(),
            attentionMessage:
              "The finalized Base withdrawal transaction reverted. No withdrawal was recorded on the IC; you can close this transfer and try again.",
          })
        toast.warning("The finalized Base withdrawal transaction reverted. You can try again.")
        return
      }

      const refreshed = readPendingConfirmations().find(
        (candidate) =>
          candidate.kind === "withdrawal" &&
          candidate.transactionHash.toLowerCase() === entry.transactionHash.toLowerCase(),
      )
      if (!refreshed || refreshed.notification.status !== "awaiting-notification") return
      let attemptKind: "automatic" | "manual" | "finality-readvance"
      if (trigger === "manual") {
        attemptKind = "manual"
      } else if (refreshed.notification.failure?.disposition === "finality-wait") {
        const previousBlock =
          refreshed.notification.lastAttemptedFinalizedBlock === undefined
            ? undefined
            : BigInt(refreshed.notification.lastAttemptedFinalizedBlock)
        if (
          refreshed.notification.finalityReadvanceUsed ||
          previousBlock === undefined ||
          finalized.number <= previousBlock
        )
          return
        attemptKind = "finality-readvance"
      } else if (!refreshed.notification.automaticAttemptUsed || !refreshed.notification.failure) {
        attemptKind = "automatic"
      } else {
        return
      }
      if (latest)
        update(latest.id, {
          phase: "awaiting-ic-notification",
          finalizedBlockNumber: finalized.number.toString(),
        })

      const transactionKey = entry.transactionHash.toLowerCase()
      if (notificationRunsRef.current.has(transactionKey)) return
      notificationRunsRef.current.add(transactionKey)
      try {
        if (latest && trigger === "manual") setAction(latest.id, undefined)
        await markPendingConfirmationNotificationAttempt(
          refreshed,
          attemptKind,
          finalized.number,
        ).catch(() => undefined)
        await notifyWithdrawalWithRetry(
          refreshed,
          finalized.number,
          latest?.id,
          update,
          completeWithdrawal,
          presentationIsCurrent,
          isCurrent,
        )
      } catch (error) {
        if (!isCurrent()) return
        const latestEntry = readPendingConfirmations().find(
          (candidate) =>
            candidate.kind === "withdrawal" &&
            candidate.transactionHash.toLowerCase() === entry.transactionHash.toLowerCase(),
        )
        const automaticRetryExhausted =
          attemptKind === "manual" ||
          attemptKind === "finality-readvance" ||
          (latestEntry?.notification.status === "awaiting-notification" &&
            latestEntry.notification.shortRetryUsed)
        const failure = notificationFailure(error, automaticRetryExhausted)
        await setPendingConfirmationNotificationFailure(latestEntry ?? refreshed, failure).catch(
          () => undefined,
        )
        presentNotificationFailure(
          latestEntry ?? refreshed,
          failure,
          latest?.id,
          update,
          setAction,
          observeWithdrawal,
        )
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
  }, [completeWithdrawal, setAction, update])

  return null
}

async function notifyWithdrawal(
  entry: PendingWithdrawal,
  progressId: string | undefined,
  update: ReturnType<typeof useBridgeProgress>["update"],
  completeWithdrawal: ReturnType<typeof useBridgeProgress>["completeWithdrawal"],
  presentationIsCurrent: (progressId: string | undefined, entry: PendingWithdrawal) => boolean,
) {
  const notified = await notifyWithdrawalWithBrowserIdentity(hexToBytes(entry.transactionHash))
  const canPresentAfterNotification = presentationIsCurrent(progressId, entry)
  const withdrawalId =
    "Duplicate" in notified ? notified.Duplicate.withdrawal_id : notified.Ingested.withdrawal_id
  const withdrawalIdHex = bytesHex(withdrawalId)
  await markPendingConfirmationNotified(entry, withdrawalIdHex)
  if (progressId && canPresentAfterNotification)
    update(progressId, {
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
  const canPresent = canPresentAfterNotification && presentationIsCurrent(progressId, entry)
  if (!canPresent) return
  const presentation = withdrawalNotificationPresentation(notified)
  if (presentation.tone === "success") toast.success(presentation.message)
  else if (presentation.tone === "warning") toast.warning(presentation.message)
  else toast.info(presentation.message)
  if (continuationError) {
    if (progressId)
      update(progressId, {
        phase: "attention",
        attentionMessage:
          continuationError instanceof Error
            ? continuationError.message
            : "The payout needs another attempt from History.",
      })
    toast.warning("The withdrawal was recorded, but the payout needs another attempt from History.")
  } else if (
    continuation &&
    "Complete" in continuation &&
    "Withdrawal" in continuation.Complete.state &&
    "Paid" in continuation.Complete.state.Withdrawal
  ) {
    completeWithdrawal({
      transactionHash: entry.transactionHash,
      owner: entry.owner,
      withdrawalId: withdrawalIdHex,
    })
    await removePendingConfirmation(entry)
  } else if (continuation && !("Complete" in continuation)) {
    if (progressId)
      update(progressId, {
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
  completeWithdrawal: ReturnType<typeof useBridgeProgress>["completeWithdrawal"],
  presentationIsCurrent: (progressId: string | undefined, entry: PendingWithdrawal) => boolean,
  isCurrent: () => boolean,
) {
  try {
    return await notifyWithdrawal(
      entry,
      progressId,
      update,
      completeWithdrawal,
      presentationIsCurrent,
    )
  } catch (error) {
    const current = readPendingConfirmations().find(
      (candidate) =>
        candidate.kind === "withdrawal" &&
        candidate.transactionHash.toLowerCase() === entry.transactionHash.toLowerCase(),
    )
    const canRetry =
      current?.notification.status === "awaiting-notification" &&
      !current.notification.shortRetryUsed &&
      notificationAllowsShortRetry(error)
    if (!canRetry) throw error
    await delay(NOTIFICATION_RETRY_DELAY_MS)
    if (!isCurrent()) return
    await markPendingConfirmationNotificationAttempt(current, "short-retry", finalizedBlock).catch(
      () => undefined,
    )
    return notifyWithdrawal(current, progressId, update, completeWithdrawal, presentationIsCurrent)
  }
}

function bytesHex(bytes: Uint8Array | number[]): `0x${string}` {
  return `0x${Array.from(bytes, (value) => Number(value).toString(16).padStart(2, "0")).join("")}`
}

function notificationAllowsShortRetry(error: unknown): boolean {
  return error instanceof NotifyWithdrawalCallError ? error.code === "Busy" : true
}

function notificationFailure(
  error: unknown,
  automaticRetryExhausted: boolean,
): PendingNotificationFailure {
  const message = error instanceof Error ? error.message : "The IC notification failed."
  if (error instanceof NotifyWithdrawalCallError) {
    if (error.code === "TransactionNotConfirmed")
      return {
        code: error.code,
        message: automaticRetryExhausted
          ? "Base finality could not be confirmed within the automatic retry budget. Retry the IC notification explicitly."
          : message,
        disposition: automaticRetryExhausted ? "manual-retry" : "finality-wait",
      }
    if (
      [
        "AnonymousCaller",
        "BaseStateMismatch",
        "BridgeSignerMismatch",
        "InvalidTransactionHash",
        "LedgerFeeExceedsServiceFee",
        "TransactionReverted",
        "WithdrawalBeforeAdmissionBoundary",
        "WithdrawalConflict",
      ].includes(error.code)
    )
      return { code: error.code, message, disposition: "terminal" }
    return { code: error.code, message, disposition: "manual-retry" }
  }
  if (
    /conflict|reverted|invalid transaction hash|bridge signer mismatch|base state mismatch/i.test(
      message,
    )
  ) {
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
  observeWithdrawal: (
    entry: PendingWithdrawal,
    progressId: string | undefined,
    trigger: "automatic" | "manual",
  ) => Promise<void>,
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
