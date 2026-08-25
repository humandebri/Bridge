import { Principal } from "@icp-sdk/core/principal"
import { useCallback, useEffect, useState } from "react"
import type { DepositView } from "@/generated/bridge.did"
import { deploymentProfile } from "@/config/profile"
import { MintAuthorizationAction, type MintConfirmation, type MintProgressEvent } from "@/features/bridge/mint-authorization-action"
import { useBridgeProgress } from "@/features/bridge/bridge-progress-provider"
import { createBridgeActor } from "@/lib/ic/bridge"

const DEPOSIT_PROGRESS_POLL_MS = 5_000

/** Rebuilds the latest deposit presentation from canonical IC and Base facts. */
export function DepositProgressCoordinator() {
  const bridgeProgress = useBridgeProgress()
  const progress = bridgeProgress.progress
  const progressId = progress?.id
  const setProgressAction = bridgeProgress.setAction
  const identity = progress?.direction === "deposit" ? progress.deposit : undefined
  const identityKey = identity ? `${identity.owner}:${identity.ownerSequence}` : undefined
  const [observation, setObservation] = useState<{ identityKey: string; record: DepositView }>()
  const record = observation && observation.identityKey === identityKey ? observation.record : undefined
  const registerAction = useCallback((action?: { label: string; run: () => void | Promise<void>; pending?: boolean }) => {
    if (progressId) setProgressAction(progressId, action)
  }, [progressId, setProgressAction])

  useEffect(() => {
    if (!progress || !identity || progress.phase === "complete" || progress.phase === "attention") {
      return
    }
    let active = true
    const tick = async () => {
      try {
        const actor = await createBridgeActor(deploymentProfile.icHost, deploymentProfile.bridgeCanisterId as string)
        const result = await actor.get_deposit_by_owner_sequence(Principal.fromText(identity.owner), BigInt(identity.ownerSequence))
        if (!active || !result[0]) return
        setObservation({ identityKey: `${identity.owner}:${identity.ownerSequence}`, record: result[0] })
        if ("Minted" in result[0].state) {
          bridgeProgress.update(progress.id, { phase: "complete", completionMessage: `${progress.receiveAmount} ${progress.receiveSymbol} was minted on Base.` })
        } else if ("AuthorizationAvailable" in result[0].state && !progress.transactionHash) {
          bridgeProgress.update(progress.id, { phase: "awaiting-base-mint" })
        } else if ("EscrowedUnquoted" in result[0].state || "AuthorizationPending" in result[0].state) {
          bridgeProgress.update(progress.id, { phase: "authorization-generating" })
        } else if ("RefundAvailable" in result[0].state || "RefundProcessing" in result[0].state || "Refunded" in result[0].state || "FundingReconciliationHold" in result[0].state || "Cancelled" in result[0].state) {
          bridgeProgress.update(progress.id, { phase: "attention", attentionMessage: "This deposit cannot continue to Base minting. Open History to review its refund or reconciliation state." })
        }
      } catch {
        // Temporary IC query failures keep the last observed state and retry.
      }
    }
    void tick()
    const interval = window.setInterval(() => void tick(), DEPOSIT_PROGRESS_POLL_MS)
    return () => { active = false; window.clearInterval(interval) }
  }, [bridgeProgress, identity, progress])

  if (!progress || progress.direction !== "deposit" || !identity || !record || !("AuthorizationAvailable" in record.state)) return null

  const onProgress = (event: MintProgressEvent) => {
    if (event.phase === "awaiting-wallet") bridgeProgress.update(progress.id, { phase: "awaiting-base-mint" })
    else if (event.phase === "submitted") bridgeProgress.update(progress.id, { phase: "base-mint-submitted", transactionHash: event.transactionHash, receiptBlockNumber: undefined, baseTransactionOutcome: undefined })
    else if (event.phase === "included") bridgeProgress.update(progress.id, { phase: "base-mint-included", transactionHash: event.transactionHash, receiptBlockNumber: event.blockNumber.toString(), baseTransactionOutcome: event.outcome })
    else if (event.phase === "finalizing") bridgeProgress.update(progress.id, { phase: "base-mint-finalizing", transactionHash: event.transactionHash, receiptBlockNumber: event.blockNumber.toString() })
    else if (event.phase === "attention") bridgeProgress.update(progress.id, { phase: "attention", transactionHash: event.transactionHash ?? progress.transactionHash, attentionMessage: event.message })
  }
  const onMintConfirmed = (confirmation: MintConfirmation) => bridgeProgress.update(progress.id, {
    phase: "complete",
    transactionHash: confirmation.transactionHash,
    completionMessage: `${progress.receiveAmount} ${progress.receiveSymbol} was minted to ${shortAddress(confirmation.recipient)}.`,
  })

  return <MintAuthorizationAction
    record={record}
    headless
    autoPromptOwner={identity.owner}
    onProgress={onProgress}
    onMintConfirmed={onMintConfirmed}
    registerAction={registerAction}
  />
}

function shortAddress(value: string): string {
  return value.length > 18 ? `${value.slice(0, 10)}…${value.slice(-6)}` : value
}
