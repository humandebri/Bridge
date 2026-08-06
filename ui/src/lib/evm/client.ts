import { createConfig, http } from "wagmi"
import { coinbaseWallet, injected, metaMask, walletConnect } from "wagmi/connectors"
import { createPublicClient, defineChain } from "viem"
import { base, baseSepolia } from "viem/chains"
import { deploymentProfile, type DeploymentProfile } from "@/config/profile"

const baseExplorerByChainId = new Map<number, string>([
  [base.id, base.blockExplorers.default.url],
  [baseSepolia.id, baseSepolia.blockExplorers.default.url],
])

export function baseTransactionExplorerUrl(chainId: number, transactionHash: `0x${string}`): string | undefined {
  const explorer = baseExplorerByChainId.get(chainId)
  if (!explorer || !/^0x[0-9a-fA-F]{64}$/.test(transactionHash)) return undefined
  return `${explorer}/tx/${transactionHash}`
}

export function createProfileChain(profile: DeploymentProfile) {
  return defineChain({
  id: profile.chainId,
  name: profile.label,
  nativeCurrency: { name: "Ether", symbol: "ETH", decimals: 18 },
  rpcUrls: { default: { http: [profile.baseRpcUrl] } },
})
}

export const profileChain = createProfileChain(deploymentProfile)

export function createBasePublicClient(profile: DeploymentProfile = deploymentProfile) {
  return createPublicClient({
    chain: createProfileChain(profile),
    transport: http(profile.baseRpcUrl),
  })
}

export const basePublicClient = createBasePublicClient()

export function createBaseHistoryClients(profile: DeploymentProfile = deploymentProfile) {
  const urls = profile.baseHistoryRpcUrls ?? [profile.baseRpcUrl]
  return urls.map((url) => createPublicClient({
    chain: createProfileChain(profile),
    transport: http(url, { retryCount: 0 }),
  }))
}

export const baseHistoryClients = createBaseHistoryClients()

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

export function withBaseHistoryClient<T>(
  operation: (client: (typeof baseHistoryClients)[number]) => Promise<T>,
): Promise<T> {
  return firstSuccessfulHistoryClient(baseHistoryClients, operation)
}

const walletConnectProjectId = import.meta.env.VITE_WALLETCONNECT_PROJECT_ID?.trim()
const walletConnectMetadata = typeof window === "undefined" ? undefined : {
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
      appLogoUrl: typeof window === "undefined" ? null : new URL("/kinic-mark.png", window.location.origin).href,
    }),
    metaMask(),
    injected(),
    ...(walletConnectProjectId ? [walletConnect({
      projectId: walletConnectProjectId,
      showQrModal: true,
      metadata: walletConnectMetadata,
    })] : []),
  ],
  transports: { [profileChain.id]: http(deploymentProfile.baseRpcUrl) },
})

declare module "wagmi" {
  interface Register { config: typeof wagmiConfig }
}
