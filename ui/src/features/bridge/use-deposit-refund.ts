import {
  assertTransferActionAllowed,
  readTransferFacts,
  initialTransferFacts,
  reduceTransfer,
  publishTransferFacts,
} from "@/lib/transfer-state"
import { useCallback, useSyncExternalStore } from "react"
import { useChainId } from "wagmi"
import { useQueryClient } from "@tanstack/react-query"
import { toast } from "sonner"
import type { DepositView } from "@/generated/bridge.did"
import { useIcWallet } from "@/features/wallet/ic-wallet-provider"
import { useRuntimeValidation, useRuntimeHeartbeat } from "@/features/status/use-status"
import { refetchRuntimeAttestedWriteReady } from "@/lib/runtime-validation"
import { useBridgeProgress } from "./bridge-progress-provider"

const operations = new Map<string, Promise<DepositView>>()
const listeners = new Set<() => void>()
const subscribe = (listener: () => void) => {
  listeners.add(listener)
  return () => {
    listeners.delete(listener)
  }
}
const emit = () => listeners.forEach((listener) => listener())

/** Shared by History and the progress modal. The canister remains the refund authority. */
export function useDepositRefund() {
  const ic = useIcWallet()
  const chainId = useChainId()
  const runtime = useRuntimeValidation(chainId, { enabled: false })
  const heartbeat = useRuntimeHeartbeat(chainId, runtime.data, { enabled: false })
  const queryClient = useQueryClient()
  const bridge = useBridgeProgress()
  const pendingCount = useSyncExternalStore(
    subscribe,
    () => operations.size,
    () => 0,
  )
  const request = useCallback(
    async (record: DepositView) => {
      const key = `0x${Array.from(record.deposit_id, (b) => b.toString(16).padStart(2, "0")).join("")}`
      assertTransferActionAllowed(`deposit:${key}`)
      const existing = operations.get(key)
      if (existing) return existing
      const progress =
        bridge.progress?.deposit?.depositId?.toLowerCase() === key ? bridge.progress : undefined
      const generation = readTransferFacts(`deposit:${key}`)?.generation ?? 0
      const run = Promise.resolve().then(async () => {
        let close: (() => Promise<void>) | undefined
        try {
          const adapter = ic.adapter
          if (!adapter) throw new Error("Connect an IC wallet to check your refund.")
          if (progress) bridge.update(progress.id, { phase: "refund-checking" })
          if (!navigator.locks) throw new Error("Web Locks are required for refunds.")
          const result = await navigator.locks.request(
            `kinic-wallet-prompt:ic:${ic.account?.owner ?? "unknown"}`,
            { ifAvailable: true },
            async (lock) => {
              if (!lock) throw new Error("Another wallet operation is in progress.")
              try {
                close = await adapter.prepare()
                await refetchRuntimeAttestedWriteReady(
                  runtime.data,
                  runtime.refetch,
                  heartbeat.refetch,
                )
                assertTransferActionAllowed(`deposit:${key}`)
                return await adapter.requestDepositRefund(Uint8Array.from(record.deposit_id))
              } finally {
                const finish = close
                close = undefined
                try {
                  await finish?.()
                } catch {
                  toast.error("Refund checked. Close the wallet window manually.")
                }
              }
            },
          )
          const resultPatch = depositRefundProgress(result)
          const facts =
            readTransferFacts(`deposit:${key}`) ??
            initialTransferFacts(`deposit:${key}`, "deposit", "refund-checking")
          if (facts.generation === generation) {
            publishTransferFacts(
              reduceTransfer(facts, {
                identity: facts.identity,
                generation,
                source: "operation",
                revision: (facts.revisions.operation ?? 0) + 1,
                type: "observed",
                ...resultPatch,
              }),
            )
            if (progress) bridge.update(progress.id, resultPatch)
          }
          await queryClient.invalidateQueries({ queryKey: ["deposit-history"] })
          return result
        } catch (error) {
          const message =
            error instanceof Error ? error.message : "Refund check unavailable. Try again."
          if (progress)
            bridge.update(progress.id, {
              phase: "attention",
              issue: "refund-ready",
              attentionMessage: message,
              observationError: message,
            })
          toast.error(message)
          throw error
        } finally {
          try {
            await close?.()
          } finally {
            operations.delete(key)
            emit()
          }
        }
      })
      operations.set(key, run)
      emit()
      return run
    },
    [
      bridge,
      heartbeat.refetch,
      ic.account?.owner,
      ic.adapter,
      queryClient,
      runtime.data,
      runtime.refetch,
    ],
  )
  return { request, pending: pendingCount > 0, connected: Boolean(ic.adapter) }
}

export function depositRefundProgress(record: Pick<DepositView, "state">) {
  if ("Minted" in record.state)
    return { phase: "complete" as const, outcome: "minted" as const, observationError: undefined }
  if ("Refunded" in record.state)
    return { phase: "complete" as const, outcome: "refunded" as const, observationError: undefined }
  if ("Cancelled" in record.state)
    return {
      phase: "complete" as const,
      outcome: "cancelled" as const,
      observationError: undefined,
    }
  if ("RefundProcessing" in record.state)
    return { phase: "refund-processing" as const, observationError: undefined }
  if ("RefundAvailable" in record.state)
    return {
      phase: "attention" as const,
      issue: "refund-available" as const,
      observationError: undefined,
    }
  return {
    phase: "attention" as const,
    issue: "stopped" as const,
    attentionMessage: "Transfer needs reconciliation.",
    observationError: undefined,
  }
}
