import { createConfig, http } from "wagmi"
import { coinbaseWallet, injected, metaMask, walletConnect } from "wagmi/connectors"
import { createPublicClient, defineChain } from "viem"
import { deploymentProfile, type DeploymentProfile } from "@/config/profile"

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
