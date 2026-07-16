import { createConfig, http } from "wagmi"
import { injected } from "wagmi/connectors"
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

export const wagmiConfig = createConfig({
  chains: [profileChain],
  connectors: [injected()],
  transports: { [profileChain.id]: http(deploymentProfile.baseRpcUrl) },
})

declare module "wagmi" {
  interface Register { config: typeof wagmiConfig }
}
