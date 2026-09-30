import { redactRpcUrls } from "@/lib/transfer-error"
import { readBaseBlock } from "@/lib/base-transaction-observation"
import { useQuery, useQueryClient } from "@tanstack/react-query"
import { deploymentProfile } from "@/config/profile"
import { bridgeAbi } from "@/generated/abi/bridge.generated"
import { multiTokenBridgeAbi } from "@/generated/abi/multitokenbridge.generated"
import { createBridgeActor } from "@/lib/ic/bridge"
import {
  RUNTIME_VALIDATION_TTL_MS,
  runtimeProfileFingerprint,
  validateRuntime,
  validateRuntimeHeartbeat,
  type FinalizedRuntimeObservation,
  type RuntimeValidation,
} from "@/lib/runtime-validation"
import { basePublicClient } from "@/lib/evm/client"
import { runtimeBytecodeSha256 } from "@/lib/runtime-bytecode-hash"

interface AutomaticQueryOptions {
  enabled?: boolean
  gcTime?: number
  refetchInterval?: number
  staleTime?: number
}

type RuntimeHeartbeatQueryOptions = Omit<AutomaticQueryOptions, "refetchInterval"> & {
  refetchOnWindowFocus?: boolean | "always"
  refetchOnReconnect?: boolean | "always"
}

export function useRuntimeValidation(chainId?: number, options: AutomaticQueryOptions = {}) {
  const queryClient = useQueryClient()
  const {
    enabled = false,
    gcTime = Number.POSITIVE_INFINITY,
    staleTime = RUNTIME_VALIDATION_TTL_MS,
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
      } catch (error) {
        return {
          ready: false,
          checkedAt: Date.now(),
          blockers: [
            error instanceof Error ? redactRpcUrls(error.message) : "Runtime validation failed",
          ],
        }
      }
    },
    enabled,
    gcTime,
    staleTime,
  })
  return query
}

export function useRuntimeHeartbeat(
  chainId: number | undefined,
  initialValidation: RuntimeValidation | undefined,
  options: RuntimeHeartbeatQueryOptions = {},
) {
  const queryClient = useQueryClient()
  const {
    enabled = false,
    staleTime = RUNTIME_VALIDATION_TTL_MS,
    refetchOnWindowFocus = "always",
    refetchOnReconnect = "always",
  } = options
  const candidate: FinalizedRuntimeObservation | undefined = initialValidation
  const initialData = candidate?.ready && candidate.snapshot ? candidate : undefined
  return useQuery({
    queryKey: ["runtime-heartbeat", runtimeProfileFingerprint(deploymentProfile), chainId],
    queryFn: async () => {
      let validation
      try {
        validation = await validateRuntimeHeartbeat(deploymentProfile, chainId)
      } catch (error) {
        if (deploymentProfile.testOnly)
          console.warn(
            "Runtime heartbeat failed:",
            error instanceof Error ? redactRpcUrls(error.message) : "Request failed",
          )
        throw error
      }
      if (!validation.ready && deploymentProfile.testOnly)
        console.warn("Runtime heartbeat blockers:", validation.blockers.join("; "))
      if (validation.status) {
        queryClient.setQueryData(
          ["bridge-status", deploymentProfile.bridgeCanisterId],
          validation.status,
          { updatedAt: validation.checkedAt },
        )
      }
      return validation
    },
    enabled,
    initialData,
    initialDataUpdatedAt: initialData?.checkedAt,
    staleTime,
    refetchOnMount: initialData ? false : undefined,
    refetchOnWindowFocus,
    refetchOnReconnect,
  })
}

export function useBridgeStatus() {
  return useQuery({
    queryKey: ["bridge-status", deploymentProfile.bridgeCanisterId],
    enabled: false,
    queryFn: async () => {
      const actor = await createBridgeActor(
        deploymentProfile.icHost,
        deploymentProfile.bridgeCanisterId as string,
      )
      return actor.get_bridge_status()
    },
  })
}

export interface QuoteAssetConfig {
  assetId: `0x${string}`
  bridgeAddress: `0x${string}`
  tokenAddress: `0x${string}`
  expectedBridgeRuntimeSha256: `0x${string}`
  expectedTokenRuntimeSha256: `0x${string}`
  shared: boolean
}

export function useCurrentBaseQuote(options: AutomaticQueryOptions = {}, asset?: QuoteAssetConfig) {
  const { enabled = false, refetchInterval, staleTime } = options
  return useQuery({
    queryKey: [
      "base-quote",
      asset?.bridgeAddress ?? deploymentProfile.bridgeAddress,
      asset?.assetId,
    ],
    enabled: enabled && Boolean(asset ?? deploymentProfile.bridgeAddress),
    staleTime,
    refetchInterval,
    refetchIntervalInBackground: true,
    refetchOnWindowFocus: refetchInterval !== undefined,
    refetchOnReconnect: refetchInterval !== undefined,
    queryFn: async () => {
      const client = basePublicClient
      if (asset?.shared) {
        const block = await client.getBlock({ blockTag: "finalized" })
        if (block.number === null) throw new Error("Finalized Base block is unavailable")
        const [
          snapshot,
          globalDepositsPaused,
          globalWithdrawalsPaused,
          bridgeSigner,
          globalEpoch,
          token,
          bridgeCode,
          tokenCode,
        ] = await Promise.all([
          client.readContract({
            address: asset.bridgeAddress,
            abi: multiTokenBridgeAbi,
            functionName: "assetSnapshot",
            args: [asset.assetId],
            blockNumber: block.number,
          }),
          client.readContract({
            address: asset.bridgeAddress,
            abi: multiTokenBridgeAbi,
            functionName: "globalDepositMintsPaused",
            blockNumber: block.number,
          }),
          client.readContract({
            address: asset.bridgeAddress,
            abi: multiTokenBridgeAbi,
            functionName: "globalWithdrawalsPaused",
            blockNumber: block.number,
          }),
          client.readContract({
            address: asset.bridgeAddress,
            abi: multiTokenBridgeAbi,
            functionName: "bridgeSigner",
            blockNumber: block.number,
          }),
          client.readContract({
            address: asset.bridgeAddress,
            abi: multiTokenBridgeAbi,
            functionName: "globalEpoch",
            blockNumber: block.number,
          }),
          client.readContract({
            address: asset.bridgeAddress,
            abi: multiTokenBridgeAbi,
            functionName: "tokenForAsset",
            args: [asset.assetId],
            blockNumber: block.number,
          }),
          client.getBytecode({ address: asset.bridgeAddress, blockNumber: block.number }),
          client.getBytecode({ address: asset.tokenAddress, blockNumber: block.number }),
        ])
        if (
          !bridgeCode ||
          !tokenCode ||
          runtimeBytecodeSha256(bridgeCode).toLowerCase() !==
            asset.expectedBridgeRuntimeSha256.toLowerCase() ||
          runtimeBytecodeSha256(tokenCode).toLowerCase() !==
            asset.expectedTokenRuntimeSha256.toLowerCase() ||
          token.toLowerCase() !== asset.tokenAddress.toLowerCase() ||
          snapshot.minServiceFee === 0n ||
          snapshot.serviceFee < snapshot.minServiceFee ||
          snapshot.maxServiceFee < snapshot.minServiceFee
        ) {
          throw new Error(
            "Selected asset runtime does not match the registered Bridge configuration",
          )
        }
        return {
          serviceFee: snapshot.serviceFee,
          maxServiceFee: snapshot.maxServiceFee,
          perDepositLimit: snapshot.perDepositLimit,
          minted: snapshot.mintedInWindow,
          limit: snapshot.mintWindowLimit,
          startedAt: snapshot.mintWindowStartedAt,
          duration: snapshot.mintWindowDuration,
          depositsPaused: globalDepositsPaused || snapshot.depositMintsPaused,
          withdrawalsPaused: globalWithdrawalsPaused || snapshot.withdrawalsPaused,
          bridgeSigner,
          mintAuthorizationEpoch: globalEpoch,
          assetEpoch: snapshot.assetEpoch,
          blockTimestamp: block.timestamp,
        }
      }
      const address = deploymentProfile.bridgeAddress as `0x${string}`
      const snapshot = await client.readContract({
        address,
        abi: bridgeAbi,
        functionName: "bridgeSnapshot",
      })
      return bridgeSnapshotView(snapshot)
    },
  })
}

export function useFinalizedBaseClock(options: AutomaticQueryOptions = {}) {
  const { enabled = false, refetchInterval, staleTime } = options
  return useQuery({
    queryKey: ["finalized-base-clock", deploymentProfile.bridgeAddress],
    enabled,
    staleTime,
    refetchInterval,
    refetchIntervalInBackground: true,
    refetchOnWindowFocus: refetchInterval !== undefined,
    refetchOnReconnect: refetchInterval !== undefined,
    queryFn: async () => {
      const block = await readBaseBlock("finalized")
      if (block.timestamp === undefined) throw new Error("Finalized Base time is unavailable")
      return { timestamp: block.timestamp }
    },
  })
}

export function useLatestBaseClock(options: AutomaticQueryOptions = {}) {
  const { enabled = false, refetchInterval, staleTime } = options
  return useQuery({
    queryKey: ["latest-base-clock", deploymentProfile.bridgeAddress],
    enabled,
    staleTime,
    refetchInterval,
    refetchIntervalInBackground: true,
    refetchOnWindowFocus: refetchInterval !== undefined,
    refetchOnReconnect: refetchInterval !== undefined,
    queryFn: async () => {
      const block = await readBaseBlock("latest")
      if (block.timestamp === undefined) throw new Error("Latest Base time is unavailable")
      return { timestamp: block.timestamp }
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
  mintAuthorizationEpoch: bigint
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
    mintAuthorizationEpoch: snapshot.mintAuthorizationEpoch,
    blockTimestamp: snapshot.blockTimestamp,
  }
}
