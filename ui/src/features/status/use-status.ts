import { useQuery } from "@tanstack/react-query"
import { useEffect, useReducer } from "react"
import { createPublicClient, defineChain, http } from "viem"
import { deploymentProfile } from "@/config/profile"
import { bridgeAbi } from "@/generated/abi/bridge.generated"
import { createBridgeActor } from "@/lib/ic/bridge"
import { RUNTIME_VALIDATION_TTL_MS, runtimeWriteBlocker, validateRuntime, type RuntimeValidation } from "@/lib/runtime-validation"

export function useRuntimeValidation(chainId?: number) {
  return useQuery({
    queryKey: ["runtime-validation", chainId],
    queryFn: async () => {
      try { return await validateRuntime(deploymentProfile, chainId) }
      catch (error) { return { ready: false, checkedAt: Date.now(), blockers: [error instanceof Error ? error.message : "Runtime validation failed"] } }
    },
    enabled: false,
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

export function useCurrentBaseQuote() {
  return useQuery({
    queryKey: ["base-quote", deploymentProfile.bridgeAddress],
    enabled: false,
    queryFn: async () => {
      const client = baseClient()
      const address = deploymentProfile.bridgeAddress as `0x${string}`
      const snapshot = await client.readContract({ address, abi: bridgeAbi, functionName: "bridgeSnapshot" })
      return bridgeSnapshotView(snapshot)
    },
  })
}

export function useConfirmedBaseStatus() {
  return useQuery({
    queryKey: ["base-status-safe", deploymentProfile.bridgeAddress],
    enabled: false,
    queryFn: async () => {
      const client = baseClient()
      const address = deploymentProfile.bridgeAddress as `0x${string}`
      const safe = await client.getBlock({ blockTag: "safe" })
      if (safe.number === null) throw new Error("Safe Base block number is unavailable")
      const snapshot = await client.readContract({ address, abi: bridgeAbi, functionName: "bridgeSnapshot", blockNumber: safe.number })
      return { ...bridgeSnapshotView(snapshot), observedBlock: snapshot.blockNumber, observedTimestamp: snapshot.blockTimestamp }
    },
  })
}

function baseClient() {
  return createPublicClient({ chain: defineChain({ id: deploymentProfile.chainId, name: deploymentProfile.label, nativeCurrency: { name: "Ether", symbol: "ETH", decimals: 18 }, rpcUrls: { default: { http: [deploymentProfile.baseRpcUrl] } } }), transport: http(deploymentProfile.baseRpcUrl) })
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
