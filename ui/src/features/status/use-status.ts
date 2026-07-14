import { useQuery } from "@tanstack/react-query"
import { createPublicClient, defineChain, http } from "viem"
import { deploymentProfile } from "@/config/profile"
import { bridgeAbi } from "@/generated/abi/bridge.generated"
import { createBridgeActor } from "@/lib/ic/bridge"
import { validateRuntime } from "@/lib/runtime-validation"

export function useRuntimeValidation(chainId?: number) {
  return useQuery({
    queryKey: ["runtime-validation", chainId],
    queryFn: async () => {
      try { return await validateRuntime(deploymentProfile, chainId) }
      catch (error) { return { ready: false, checkedAt: Date.now(), blockers: [error instanceof Error ? error.message : "Runtime validation failed"] } }
    },
    refetchInterval: 30_000,
  })
}

export function useBridgeStatus() {
  return useQuery({
    queryKey: ["bridge-status", deploymentProfile.bridgeCanisterId],
    enabled: Boolean(deploymentProfile.bridgeCanisterId),
    queryFn: async () => {
      const actor = await createBridgeActor(deploymentProfile.icHost, deploymentProfile.bridgeCanisterId as string)
      return actor.get_bridge_status()
    },
    refetchInterval: 15_000,
  })
}

export function useBaseStatus() {
  return useQuery({
    queryKey: ["base-status", deploymentProfile.bridgeAddress],
    enabled: Boolean(deploymentProfile.bridgeAddress),
    queryFn: async () => {
      const client = createPublicClient({ chain: defineChain({ id: deploymentProfile.chainId, name: deploymentProfile.label, nativeCurrency: { name: "Ether", symbol: "ETH", decimals: 18 }, rpcUrls: { default: { http: [deploymentProfile.baseRpcUrl] } } }), transport: http(deploymentProfile.baseRpcUrl) })
      const address = deploymentProfile.bridgeAddress as `0x${string}`
      const snapshot = await client.readContract({ address, abi: bridgeAbi, functionName: "bridgeSnapshot" })
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
        finalizedBlock: snapshot.blockNumber,
        finalizedTimestamp: snapshot.blockTimestamp,
        bridgeSigner: snapshot.bridgeSigner,
      }
    },
    refetchInterval: 15_000,
  })
}
