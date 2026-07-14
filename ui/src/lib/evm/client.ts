import { createConfig, http } from "wagmi"
import { injected } from "wagmi/connectors"
import { defineChain } from "viem"
import { deploymentProfile } from "@/config/profile"

const profileChain = defineChain({
  id: deploymentProfile.chainId,
  name: deploymentProfile.label,
  nativeCurrency: { name: "Ether", symbol: "ETH", decimals: 18 },
  rpcUrls: { default: { http: [deploymentProfile.baseRpcUrl] } },
})

export const wagmiConfig = createConfig({
  chains: [profileChain],
  connectors: [injected()],
  transports: { [profileChain.id]: http(deploymentProfile.baseRpcUrl) },
})

declare module "wagmi" {
  interface Register { config: typeof wagmiConfig }
}
