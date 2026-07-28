import { useQuery } from "@tanstack/react-query"
import { useEffect, useReducer } from "react"
import { deploymentProfile } from "@/config/profile"
import { bridgeAbi } from "@/generated/abi/bridge.generated"
import { createBridgeActor } from "@/lib/ic/bridge"
import { finalizedHeadTimestampBlocker, RUNTIME_VALIDATION_TTL_MS, runtimeWriteBlocker, validateRuntime, validateRuntimeHeartbeat, type RuntimeValidation } from "@/lib/runtime-validation"
import { basePublicClient } from "@/lib/evm/client"

interface AutomaticQueryOptions {
  enabled?: boolean
  gcTime?: number
  refetchInterval?: number
  staleTime?: number
}

export function useRuntimeValidation(chainId?: number, options: AutomaticQueryOptions = {}) {
  const { enabled = false, gcTime, staleTime } = options
  return useQuery({
    queryKey: ["runtime-validation", chainId],
    queryFn: async () => {
      try { return await validateRuntime(deploymentProfile, chainId) }
      catch (error) { return { ready: false, checkedAt: Date.now(), blockers: [error instanceof Error ? error.message : "Runtime validation failed"] } }
    },
    enabled,
    gcTime,
    staleTime,
  })
}

export function useRuntimeHeartbeat(chainId: number | undefined, initialValidation: RuntimeValidation | undefined, options: AutomaticQueryOptions = {}) {
  const { enabled = false, refetchInterval } = options
  const initialData = initialValidation?.ready ? initialValidation : undefined
  return useQuery({
    queryKey: ["runtime-heartbeat", chainId, initialData?.checkedAt ?? 0],
    queryFn: async () => {
      try { return await validateRuntimeHeartbeat(deploymentProfile, chainId) }
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
