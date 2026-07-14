import { z } from "zod"

const address = z.custom<`0x${string}`>((value) => typeof value === "string" && /^0x[0-9a-fA-F]{40}$/.test(value))
const hash = z.custom<`0x${string}`>((value) => typeof value === "string" && /^0x[0-9a-fA-F]{64}$/.test(value))

const deploymentProfileSchema = z.object({
  environment: z.string().min(1),
  label: z.string().min(1),
  testOnly: z.boolean(),
  writeEnabled: z.boolean(),
  allowedOrigins: z.array(z.url()).min(1),
  icHost: z.url(),
  baseRpcUrl: z.url(),
  chainId: z.number().int().positive(),
  bridgeCanisterId: z.string().min(1).nullable(),
  ledgerCanisterId: z.string().min(1).nullable(),
  bridgeAddress: address.nullable(),
  bsnsAddress: address.nullable(),
  deploymentBlock: z.bigint().nonnegative().nullable(),
  bridgeRuntimeHash: hash.nullable(),
  bsnsRuntimeHash: hash.nullable(),
})

export type DeploymentProfile = z.infer<typeof deploymentProfileSchema>

// Derived only from the checked-in Base Sepolia preflight manifest. Null deployment values keep
// every asset-moving control fail-closed until a reviewed, complete manifest is checked in.
export const deploymentProfile: DeploymentProfile = deploymentProfileSchema.parse({
  environment: "base-sepolia-preflight",
  label: "Base Sepolia preflight",
  testOnly: true,
  writeEnabled: false,
  allowedOrigins: ["https://bridge.kinic.xyz", "http://localhost:5173", "http://localhost:4173", "http://127.0.0.1:5173", "http://127.0.0.1:4173"],
  icHost: "https://icp-api.io",
  baseRpcUrl: "https://base-sepolia-rpc.publicnode.com",
  chainId: 84532,
  bridgeCanisterId: null,
  ledgerCanisterId: null,
  bridgeAddress: null,
  bsnsAddress: null,
  deploymentBlock: null,
  bridgeRuntimeHash: null,
  bsnsRuntimeHash: null,
})

export function profileCompleteness(profile: DeploymentProfile): string[] {
  const missing: string[] = []
  if (!profile.writeEnabled) missing.push("Deployment profile is not approved for writes")
  if (typeof window !== "undefined" && !profile.allowedOrigins.includes(window.location.origin)) missing.push("This origin is not approved for Bridge writes")
  if (!profile.bridgeCanisterId) missing.push("Bridge canister ID is missing")
  if (!profile.ledgerCanisterId) missing.push("KINIC ledger ID is missing")
  if (!profile.bridgeAddress) missing.push("Bridge contract address is missing")
  if (!profile.bsnsAddress) missing.push("bSNS contract address is missing")
  if (profile.deploymentBlock === null) missing.push("Deployment block is missing")
  if (!profile.bridgeRuntimeHash) missing.push("Bridge runtime bytecode hash is missing")
  if (!profile.bsnsRuntimeHash) missing.push("bSNS runtime bytecode hash is missing")
  return missing
}
