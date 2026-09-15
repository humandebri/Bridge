import { useDepositRefund, depositRefundProgress } from "./use-deposit-refund"
import { Principal } from "@icp-sdk/core/principal"
import { useCallback, useEffect, useState, useRef } from "react"
import { toast } from "sonner"
import type { DepositView } from "@/generated/bridge.did"
import { deploymentProfile } from "@/config/profile"
import {
  MintAuthorizationAction,
  type MintConfirmation,
  type MintProgressEvent,
} from "@/features/bridge/mint-authorization-action"
import { useBridgeProgress } from "@/features/bridge/bridge-progress-provider"
import { createBridgeActor } from "@/lib/ic/bridge"
import { depositContinuation, settlementStopReasonName } from "@/lib/settlement-phase"

const DEPOSIT_PROGRESS_POLL_MS = 5_000
const notifiedDepositStops = new Set<string>()

/** Rebuilds the latest deposit presentation from canonical IC and Base facts. */
export function DepositProgressCoordinator() {
  const refundAction = useDepositRefund()
  const bridgeProgress = useBridgeProgress()
  const progress = bridgeProgress.progress
  const progressId = progress?.id
  const setProgressAction = bridgeProgress.setAction
  const rawUpdate = bridgeProgress.update
  const updateProgress = useCallback(
    (id: string, patch: Parameters<typeof rawUpdate>[1]) =>
      rawUpdate(id, { observationSource: "ic", ...patch }),
    [rawUpdate],
  )
  const identity = progress?.direction === "deposit" ? progress.deposit : undefined
  const identityKey = identity ? `${identity.owner}:${identity.ownerSequence}` : undefined
  const [observation, setObservation] = useState<{ identityKey: string; record: DepositView }>()
  const record =
    observation && observation.identityKey === identityKey ? observation.record : undefined
  const registerAction = useCallback(
    (action?: { label: string; run: () => void | Promise<void>; pending?: boolean }) => {
      if (progressId) setProgressAction(progressId, action)
    },
    [progressId, setProgressAction],
  )

  useEffect(() => {
    if (
      !progress ||
      !identity ||
      (progress.phase === "complete" && !progress.transfer?.recordingPending)
    ) {
      return
    }
    let active = true
    let running = false
    const tick = async () => {
      if (!active || running) return
      running = true
      try {
        const actor = await createBridgeActor(
          deploymentProfile.icHost,
          deploymentProfile.bridgeCanisterId as string,
        )
        const result = await actor.get_deposit_by_owner_sequence(
          Principal.fromText(identity.owner),
          BigInt(identity.ownerSequence),
        )
        if (!active || !result[0]) return
        const record = result[0]
        setObservation({ identityKey: `${identity.owner}:${identity.ownerSequence}`, record })
        const continuation = depositContinuation(record)
        if ("Minted" in record.state) {
          updateProgress(progress.id, {
            phase: "complete",
            outcome: "minted",
            recordingPending: false,
            observationError: undefined,
            completionMessage: `${progress.receiveAmount} ${progress.receiveSymbol} was minted on Base.`,
          })
        } else if (
          "AuthorizationAvailable" in record.state &&
          !progress.transactionHash &&
          (progress.phase === "authorization-generating" ||
            progress.phase === "ic-deposit-accepted" ||
            (progress.phase === "attention" &&
              progress.attentionPhase === "authorization-generating"))
        ) {
          updateProgress(progress.id, { phase: "awaiting-base-mint", observationError: undefined })
        } else if (continuation.mode === "stopped") {
          const message = continuation.message ?? "This deposit stopped and needs attention."
          updateProgress(progress.id, {
            phase: "attention",
            attentionPhase: "authorization-generating",
            attentionMessage: message,
          })
          notifyDepositStopOnce(
            `${identity.owner}:${identity.ownerSequence}`,
            continuation.reason ? settlementStopReasonName(continuation.reason) : "Unknown",
            message,
          )
        } else if (continuation.mode === "automatic" && continuation.reason) {
          updateProgress(progress.id, {
            phase: "attention",
            attentionPhase: "authorization-generating",
            attentionMessage:
              continuation.message ??
              "The previous attempt stopped temporarily. The Bridge will retry automatically.",
          })
        } else if ("EscrowedUnquoted" in record.state || "AuthorizationPending" in record.state) {
          updateProgress(progress.id, { phase: "authorization-generating" })
        } else if (
          "RefundAvailable" in record.state ||
          "RefundProcessing" in record.state ||
          "Refunded" in record.state ||
          "FundingReconciliationHold" in record.state ||
          "Cancelled" in record.state
        ) {
          updateProgress(progress.id, depositRefundProgress(record))
        }
      } catch {
        if (active)
          updateProgress(progress.id, {
            observationError: "IC status could not be refreshed. Retrying.",
          })
      } finally {
        running = false
      }
    }
    void tick()
    const interval = window.setInterval(() => void tick(), DEPOSIT_PROGRESS_POLL_MS)
    return () => {
      active = false
      window.clearInterval(interval)
    }
  }, [identity, progress, updateProgress])

  const transactionHash = progress?.transactionHash
  const onProgress = useCallback(
    (event: MintProgressEvent) => {
      if (!progressId) return
      const updateMint = (patch: Parameters<typeof rawUpdate>[1]) =>
        updateProgress(progressId, {
          ...patch,
          observationSource:
            event.phase === "preparing" ||
            event.phase === "awaiting-wallet" ||
            event.phase === "attention"
              ? "wallet"
              : "base",
        })
      if (event.phase === "storage-warning") updateMint({ storageWarning: event.message })
      else if (event.phase === "awaiting-wallet" || event.phase === "preparing")
        updateMint({ phase: "awaiting-base-mint" })
      else if (event.phase === "submitted")
        updateMint({
          phase: "base-mint-submitted",
          transactionHash: event.transactionHash,
          receiptBlockNumber: undefined,
          baseTransactionOutcome: undefined,
        })
      else if (event.phase === "included")
        updateMint({
          phase: "base-mint-included",
          observationError: undefined,
          transactionHash: event.transactionHash,
          receiptBlockNumber: event.blockNumber.toString(),
          baseTransactionOutcome: event.outcome,
        })
      else if (event.phase === "recorded")
        updateMint({
          phase: "complete",
          outcome: "minted",
          recordingPending: false,
          observationError: undefined,
        })
      else if (event.phase === "finalized")
        updateMint({
          phase: "complete",
          outcome: "minted",
          recordingPending: true,
          observationError: undefined,
        })
      else if (event.phase === "finalizing")
        updateMint({
          phase: "base-mint-finalizing",
          transactionHash: event.transactionHash,
          receiptBlockNumber: event.blockNumber.toString(),
        })
      else if (event.phase === "unavailable") updateMint({ observationError: event.message })
      else if (event.phase === "attention")
        updateMint({
          phase: "attention",
          transactionHash: event.transactionHash ?? transactionHash,
          attentionPhase: "awaiting-base-mint",
          attentionMessage: event.message,
          issue: event.issue,
        })
    },
    [progressId, transactionHash, updateProgress],
  )

  const refundRecord = useRef(record)
  const requestRefund = useRef(refundAction.request)
  useEffect(() => {
    refundRecord.current = record
    requestRefund.current = refundAction.request
  }, [record, refundAction.request])
  const canonicalRefund = Boolean(
    record && ("RefundAvailable" in record.state || "RefundProcessing" in record.state),
  )
  useEffect(() => {
    if (!canonicalRefund) return
    registerAction({
      label: refundAction.pending ? "Checking…" : "Check refund",
      pending: refundAction.pending,
      run: async () => {
        if (refundRecord.current) await requestRefund.current(refundRecord.current).catch(() => {})
      },
    })
    return () => registerAction(undefined)
  }, [canonicalRefund, refundAction.pending, registerAction])

  if (
    !progress ||
    progress.direction !== "deposit" ||
    !identity ||
    !record ||
    !("AuthorizationAvailable" in record.state)
  )
    return null
  const onMintConfirmed = (confirmation: MintConfirmation) =>
    bridgeProgress.update(progress.id, {
      phase: "complete",
      transactionHash: confirmation.transactionHash,
      completionMessage: `${progress.receiveAmount} ${progress.receiveSymbol} was minted to ${shortAddress(confirmation.recipient)}.`,
    })

  return (
    <MintAuthorizationAction
      record={record}
      headless
      autoPromptOwner={identity.owner}
      onProgress={onProgress}
      onMintConfirmed={onMintConfirmed}
      registerAction={registerAction}
      onRequestRefund={() => {
        void refundAction.request(record).catch(() => {})
      }}
      claimingRefund={refundAction.pending}
    />
  )
}

function notifyDepositStopOnce(identityKey: string, reason: string, message: string): void {
  const key = `kinic.bridge.deposit-stop-notified:${identityKey}:${reason}`
  if (notifiedDepositStops.has(key)) return
  notifiedDepositStops.add(key)
  toast.error(message, { id: key })
}

function shortAddress(value: string): string {
  return value.length > 18 ? `${value.slice(0, 10)}…${value.slice(-6)}` : value
}
