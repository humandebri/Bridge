import { createConfig, http } from "wagmi"
import { coinbaseWallet, injected, walletConnect } from "wagmi/connectors"
import { createPublicClient, defineChain } from "viem"
import { base, baseSepolia } from "viem/chains"
import {
  canonicalRpcUrl,
  deploymentProfile,
  resolvedBaseRpcUrl,
  type DeploymentProfile,
} from "@/config/profile"

const baseExplorerByChainId = new Map<number, string>([
  [base.id, base.blockExplorers.default.url],
  [baseSepolia.id, baseSepolia.blockExplorers.default.url],
])

export function baseTransactionExplorerUrl(
  chainId: number,
  transactionHash: `0x${string}`,
): string | undefined {
  const explorer = baseExplorerByChainId.get(chainId)
  if (!explorer || !/^0x[0-9a-fA-F]{64}$/.test(transactionHash)) return undefined
  return `${explorer}/tx/${transactionHash}`
}

export function createProfileChain(profile: DeploymentProfile) {
  const rpcUrl = resolvedBaseRpcUrl(profile)
  return defineChain({
    id: profile.chainId,
    name: profile.label,
    nativeCurrency: { name: "Ether", symbol: "ETH", decimals: 18 },
    rpcUrls: { default: { http: [rpcUrl] } },
  })
}

export const profileChain = createProfileChain(deploymentProfile)

export function createBasePublicClient(profile: DeploymentProfile = deploymentProfile) {
  return createPublicClient({
    chain: createProfileChain(profile),
    transport: http(resolvedBaseRpcUrl(profile)),
  })
}

export const basePublicClient = createBasePublicClient()

export function createBaseHistoryClients(profile: DeploymentProfile = deploymentProfile) {
  const urls = [
    ...new Set((profile.baseHistoryRpcUrls ?? [resolvedBaseRpcUrl(profile)]).map(canonicalRpcUrl)),
  ]
  return urls.map((url) =>
    createPublicClient({
      chain: createProfileChain(profile),
      transport: http(url, { retryCount: 0 }),
    }),
  )
}

export const baseHistoryClients = createBaseHistoryClients()

export async function hasIndependentFinalizedRevertQuorum(
  transactionHash: `0x${string}`,
  clients = baseHistoryClients,
): Promise<boolean> {
  if (clients.length < 2) return false
  const observations = await Promise.allSettled(
    clients.map(async (client) => {
      const receipt = await client.getTransactionReceipt({ hash: transactionHash })
      if (receipt.status !== "reverted" || receipt.blockHash === null) return undefined
      const finalized = await client.getBlock({ blockTag: "finalized" })
      if (finalized.number === null || finalized.number < receipt.blockNumber) return undefined
      const checkpoint = await client.getBlock({ blockNumber: receipt.blockNumber })
      if (
        checkpoint.hash === null ||
        checkpoint.hash.toLowerCase() !== receipt.blockHash.toLowerCase()
      )
        return undefined
      return `${receipt.blockNumber}:${receipt.blockHash.toLowerCase()}`
    }),
  )
  const counts = new Map<string, number>()
  for (const observation of observations) {
    if (observation.status !== "fulfilled" || observation.value === undefined) continue
    const count = (counts.get(observation.value) ?? 0) + 1
    if (count >= 2) return true
    counts.set(observation.value, count)
  }
  return false
}

export async function firstSuccessfulHistoryClient<C, T>(
  clients: readonly C[],
  operation: (client: C) => Promise<T>,
): Promise<T> {
  const errors: unknown[] = []
  for (const client of clients) {
    try {
      return await operation(client)
    } catch (error) {
      errors.push(error)
    }
  }
  throw new AggregateError(errors, "Base history RPCs are unavailable")
}

export async function withHistoryClientFailover<C, T>(
  clients: readonly C[],
  failedClientIndexes: Set<number>,
  operation: (client: C) => Promise<T>,
): Promise<T> {
  let candidates = clients
    .map((client, index) => ({ client, index }))
    .filter(({ index }) => !failedClientIndexes.has(index))
  if (candidates.length === 0) {
    failedClientIndexes.clear()
    candidates = clients.map((client, index) => ({ client, index }))
  }
  return firstSuccessfulHistoryClient(candidates, async ({ client, index }) => {
    try {
      return await operation(client)
    } catch (error) {
      failedClientIndexes.add(index)
      throw error
    }
  })
}

const walletConnectProjectId = import.meta.env.VITE_WALLETCONNECT_PROJECT_ID?.trim()
const walletConnectMetadata =
  typeof window === "undefined"
    ? undefined
    : {
        name: "KINIC Bridge",
        description: "Bridge KINIC between Internet Computer and Base.",
        url: window.location.origin,
        icons: [new URL("/kinic-mark.png", window.location.origin).href],
      }

export const wagmiConfig = createConfig({
  chains: [profileChain],
  connectors: [
    coinbaseWallet({
      appName: "KINIC Bridge",
      appLogoUrl:
        typeof window === "undefined"
          ? null
          : new URL("/kinic-mark.png", window.location.origin).href,
    }),
    injected(),
    ...(walletConnectProjectId
      ? [
          walletConnect({
            projectId: walletConnectProjectId,
            showQrModal: true,
            metadata: walletConnectMetadata,
          }),
        ]
      : []),
  ],
  transports: { [profileChain.id]: http(resolvedBaseRpcUrl(deploymentProfile)) },
})

declare module "wagmi" {
  interface Register {
    config: typeof wagmiConfig
  }
}
