import { useQuery, useQueryClient } from "@tanstack/react-query"
import { useEffect, useReducer, useRef, useState } from "react"
import { deploymentProfile } from "@/config/profile"
import { bridgeAbi } from "@/generated/abi/bridge.generated"
import { createBridgeActor } from "@/lib/ic/bridge"
import { finalizedHeadTimestampBlocker, RUNTIME_VALIDATION_TTL_MS, runtimeProfileFingerprint, runtimeWriteBlocker, validateRuntime, validateRuntimeHeartbeat, type FinalizedRuntimeObservation, type RuntimeValidation } from "@/lib/runtime-validation"
import { basePublicClient } from "@/lib/evm/client"

interface AutomaticQueryOptions {
  enabled?: boolean
  gcTime?: number
  refetchInterval?: number
  staleTime?: number
}

interface RuntimeValidationQueryOptions extends AutomaticQueryOptions {
  retryNotReadyAfterMs?: number
}

export function useRuntimeValidation(chainId?: number, options: RuntimeValidationQueryOptions = {}) {
  const queryClient = useQueryClient()
  const {
    enabled = false,
    gcTime = Number.POSITIVE_INFINITY,
    retryNotReadyAfterMs,
    staleTime = Number.POSITIVE_INFINITY,
  } = options
  const query = useQuery({
    queryKey: ["runtime-validation", runtimeProfileFingerprint(deploymentProfile), chainId],
    queryFn: async () => {
      try {
        const validation = await validateRuntime(deploymentProfile, chainId)
        if (validation.snapshot) {
          queryClient.setQueryData(
            ["runtime-heartbeat", runtimeProfileFingerprint(deploymentProfile), chainId],
            validation,
            { updatedAt: validation.checkedAt },
          )
        }
        if (validation.status) {
          queryClient.setQueryData(
            ["bridge-status", deploymentProfile.bridgeCanisterId],
            validation.status,
            { updatedAt: validation.checkedAt },
          )
        }
        return validation
      }
      catch (error) { return { ready: false, checkedAt: Date.now(), blockers: [error instanceof Error ? error.message : "Runtime validation failed"] } }
    },
    enabled,
    gcTime,
    staleTime,
  })
  const retryKey = chainId?.toString() ?? "no-chain"
  const attemptedRetryKey = useRef<string | undefined>(undefined)
  const [pendingRetryKey, setPendingRetryKey] = useState<string>()
  const { data, isFetching, refetch } = query

  useEffect(() => {
    if (!enabled
      || retryNotReadyAfterMs === undefined
      || data?.ready !== false
      || isFetching
      || attemptedRetryKey.current === retryKey) return

    setPendingRetryKey(retryKey)
    const timeout = window.setTimeout(() => {
      attemptedRetryKey.current = retryKey
      void refetch().finally(() => {
        setPendingRetryKey((current) => current === retryKey ? undefined : current)
      })
    }, retryNotReadyAfterMs)
    return () => window.clearTimeout(timeout)
  }, [data?.ready, enabled, isFetching, refetch, retryKey, retryNotReadyAfterMs])

  return {
    ...query,
    isAutoRetryPending: pendingRetryKey === retryKey && data?.ready === false,
  }
}

export function useRuntimeHeartbeat(chainId: number | undefined, initialValidation: RuntimeValidation | undefined, options: AutomaticQueryOptions = {}) {
  const queryClient = useQueryClient()
  const { enabled = false, refetchInterval } = options
  const candidate: FinalizedRuntimeObservation | undefined = initialValidation
  const initialData = candidate?.ready && candidate.snapshot ? candidate : undefined
  return useQuery({
    queryKey: ["runtime-heartbeat", runtimeProfileFingerprint(deploymentProfile), chainId],
    queryFn: async () => {
      try {
        const validation = await validateRuntimeHeartbeat(deploymentProfile, chainId)
        if (validation.status) {
          queryClient.setQueryData(
            ["bridge-status", deploymentProfile.bridgeCanisterId],
            validation.status,
            { updatedAt: validation.checkedAt },
          )
        }
        return validation
      }
      catch (error) { return { ready: false, checkedAt: Date.now(), blockers: [error instanceof Error ? error.message : "Runtime heartbeat failed"] } }
    },
    enabled,
    initialData,
    initialDataUpdatedAt: initialData?.checkedAt,
    staleTime: refetchInterval,
    refetchOnMount: initialData ? false : undefined,
    refetchInterval,
    refetchIntervalInBackground: false,
    refetchOnWindowFocus: refetchInterval !== undefined,
    refetchOnReconnect: refetchInterval !== undefined,
  })
}

export function finalizedObservationQuote(observation?: FinalizedRuntimeObservation) {
  return observation?.ready ? observation.snapshot : undefined
}

export function useRuntimeWriteReadiness(validation?: RuntimeValidation) {
  const [, expire] = useReducer((value: number) => value + 1, 0)
  useEffect(() => {
    if (!validation?.ready) return
    const remaining = validation.checkedAt + RUNTIME_VALIDATION_TTL_MS - Date.now()
    if (remaining <= 0) return
    const timeout = window.setTimeout(expire, remaining + 1)
    return () => window.clearTimeout(timeout)
  }, [validation?.checkedAt, validation?.ready])
  const reason = runtimeWriteBlocker(validation)
  return { ready: reason === undefined, reason }
}

export function useBridgeStatus() {
  return useQuery({
    queryKey: ["bridge-status", deploymentProfile.bridgeCanisterId],
    enabled: false,
    queryFn: async () => {
      const actor = await createBridgeActor(deploymentProfile.icHost, deploymentProfile.bridgeCanisterId as string)
      return actor.get_bridge_status()
    },
  })
}

export function useCurrentBaseQuote(options: AutomaticQueryOptions = {}) {
  const { enabled = false, refetchInterval, staleTime } = options
  return useQuery({
    queryKey: ["base-quote", deploymentProfile.bridgeAddress],
    enabled,
    staleTime,
    refetchInterval,
    refetchIntervalInBackground: false,
    refetchOnWindowFocus: refetchInterval !== undefined,
    refetchOnReconnect: refetchInterval !== undefined,
    queryFn: async () => {
      const client = basePublicClient
      const address = deploymentProfile.bridgeAddress as `0x${string}`
      const snapshot = await client.readContract({ address, abi: bridgeAbi, functionName: "bridgeSnapshot" })
      return bridgeSnapshotView(snapshot)
    },
  })
}

export function useConfirmedBaseStatus() {
  return useQuery({
    queryKey: ["base-status-finalized", deploymentProfile.bridgeAddress],
    enabled: false,
    queryFn: async () => {
      const client = basePublicClient
      const address = deploymentProfile.bridgeAddress as `0x${string}`
      const finalized = await client.getBlock({ blockTag: "finalized" })
      if (finalized.number === null || finalized.hash === null) throw new Error("Finalized Base block number or hash is unavailable")
      const timestampBlocker = finalizedHeadTimestampBlocker(finalized.timestamp)
      if (timestampBlocker) throw new Error(timestampBlocker)
      const snapshot = await client.readContract({
        address,
        abi: bridgeAbi,
        functionName: "bridgeSnapshot",
        blockHash: finalized.hash,
        requireCanonical: true,
      })
      return {
        ...bridgeSnapshotView(snapshot),
        observedBlock: finalized.number,
        observedBlockHash: finalized.hash,
        observedTimestamp: snapshot.blockTimestamp,
      }
    },
  })
}

function bridgeSnapshotView(snapshot: {
  serviceFee: bigint
  maxServiceFee: bigint
  perDepositLimit: bigint
  mintedInWindow: bigint
  mintWindowLimit: bigint
  mintWindowStartedAt: bigint
  mintWindowDuration: bigint
  depositMintsPaused: boolean
  withdrawalsPaused: boolean
  bridgeSigner: `0x${string}`
  blockTimestamp: bigint
}) {
  return {
    serviceFee: snapshot.serviceFee,
    maxServiceFee: snapshot.maxServiceFee,
    perDepositLimit: snapshot.perDepositLimit,
    minted: snapshot.mintedInWindow,
    limit: snapshot.mintWindowLimit,
    startedAt: snapshot.mintWindowStartedAt,
    duration: snapshot.mintWindowDuration,
    depositsPaused: snapshot.depositMintsPaused,
    withdrawalsPaused: snapshot.withdrawalsPaused,
    bridgeSigner: snapshot.bridgeSigner,
  }
}
