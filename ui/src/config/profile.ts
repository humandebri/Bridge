import { z } from "zod"

const address = z.custom<`0x${string}`>(
  (value) => typeof value === "string" && /^0x[0-9a-fA-F]{40}$/.test(value),
)
const hash = z
  .custom<`0x${string}`>((value) => typeof value === "string" && /^0x[0-9a-fA-F]{64}$/.test(value))
  .refine((value) => !/^0x0+$/.test(value), "hash must be nonzero")
const releaseSha256 = z
  .string()
  .regex(/^[0-9a-f]{64}$/i, "SHA-256 must be 64 hexadecimal characters")
  .refine((value) => !/^0+$/.test(value), "SHA-256 must be nonzero")
const tokenMetadata = z.object({
  symbol: z.string().min(1),
  decimals: z.literal(8),
})

export function canonicalRpcUrl(value: string): string {
  return new URL(value).href
}

export const deploymentProfileSchema = z
  .object({
    environment: z.string().min(1),
    label: z.string().min(1),
    testOnly: z.boolean(),
    environmentMode: z.enum(["short-delay-test-only"]).nullable(),
    activationTimelockDelaySeconds: z.number().int().positive().nullable(),
    icHost: z.url(),
    baseRpcUrl: z.url().optional(),
    baseHistoryRpcUrls: z
      .array(z.url())
      .min(1)
      .refine(
        (urls) => new Set(urls.map(canonicalRpcUrl)).size === urls.length,
        "Base history RPC URLs must be distinct",
      )
      .optional(),
    chainId: z.number().int().positive(),
    bridgeCanisterId: z.string().min(1).nullable(),
    deploymentInstanceId: hash.nullable(),
    minimumWithdrawalId: hash.nullable(),
    ledgerCanisterId: z.string().min(1).nullable(),
    indexCanisterId: z.string().min(1).nullable(),
    snsRootCanisterId: z.string().min(1).nullable().default(null),
    icToken: tokenMetadata.extend({ name: z.string().min(1) }),
    baseToken: tokenMetadata,
    bridgeAddress: address.nullable(),
    bsnsAddress: address.nullable(),
    timelockAddress: address.nullable(),
    expected_bridge_signer: address.nullable(),
    evmRpcCanisterId: z.string().min(1).nullable(),
    rpcProviderUrlsSha256: hash.nullable(),
    deploymentBlock: z.coerce.bigint().nonnegative().nullable(),
    bridgeRuntimeHash: hash.nullable(),
    bsnsRuntimeHash: hash.nullable(),
  })
  .superRefine((profile, context) => {
    if (!profile.testOnly) return
    try {
      assertEmbeddedTestUiProfile(profile)
    } catch (error) {
      context.addIssue({
        code: "custom",
        message: error instanceof Error ? error.message : "Invalid test deployment profile",
      })
    }
  })

function assertEmbeddedTestUiProfile(profile: {
  environment: string
  environmentMode: "short-delay-test-only" | null
  activationTimelockDelaySeconds: number | null
  baseHistoryRpcUrls?: string[]
  chainId: number
  bridgeCanisterId: string | null
  deploymentInstanceId: `0x${string}` | null
  minimumWithdrawalId: `0x${string}` | null
  ledgerCanisterId: string | null
  indexCanisterId: string | null
  evmRpcCanisterId: string | null
}): void {
  if (profile.chainId === 8453) throw new Error("Test UI deploy rejects Base Mainnet")
  const productionIds = new Set(["73mez-iiaaa-aaaaq-aaasq-cai", "7vojr-tyaaa-aaaaq-aaatq-cai"])
  if (
    [profile.bridgeCanisterId, profile.ledgerCanisterId, profile.indexCanisterId].some(
      (id) => id && productionIds.has(id),
    )
  ) {
    throw new Error("Test UI deploy rejects production canister IDs")
  }
  if (
    profile.environment === "sepolia-staging" &&
    (profile.chainId !== 84532 || profile.evmRpcCanisterId !== "7hfb6-caaaa-aaaar-qadga-cai")
  ) {
    throw new Error("Sepolia staging requires Base Sepolia and the official EVM RPC Canister")
  }
  if (
    profile.environment === "sepolia-staging" &&
    new Set((profile.baseHistoryRpcUrls ?? []).map(canonicalRpcUrl)).size < 2
  ) {
    throw new Error("Sepolia staging requires at least two distinct reviewed Base history RPC URLs")
  }
  if (
    profile.environment === "sepolia-staging" &&
    (!profile.deploymentInstanceId || !profile.minimumWithdrawalId)
  ) {
    throw new Error("Sepolia staging requires deployment instance and minimum withdrawal IDs")
  }
  if (
    profile.environment === "sepolia-staging" &&
    (profile.environmentMode !== "short-delay-test-only" ||
      profile.activationTimelockDelaySeconds !== 300)
  ) {
    throw new Error("Sepolia staging requires the reviewed five-minute test-only Timelock policy")
  }
}

export type DeploymentProfile = z.infer<typeof deploymentProfileSchema>

// Release inputs retain the evidence needed by deploy-time verification. This schema is
// intentionally separate from the browser contract so release metadata cannot enter runtime.
export const releaseProfileSchema = deploymentProfileSchema.extend({
  gateBManifestSha256: releaseSha256.nullable(),
  profileFileSha256: releaseSha256,
  profileCanonicalSha256: releaseSha256,
})

export type ReleaseDeploymentProfile = z.infer<typeof releaseProfileSchema>

export const DEFAULT_BASE_MAINNET_RPC_URL = "https://mainnet.base.org"

export function resolvedBaseRpcUrl(
  profile: Pick<DeploymentProfile, "baseRpcUrl" | "chainId">,
): string {
  if (profile.chainId === 8453) return DEFAULT_BASE_MAINNET_RPC_URL
  if (profile.baseRpcUrl) return profile.baseRpcUrl
  throw new Error(`Deployment profile has no default RPC URL for chain ${profile.chainId}`)
}

// Local and test builds fail closed on this incomplete profile. Managed deployments inject a
// complete runtime profile; release evidence is intentionally not part of the browser contract.
const preflightProfile = {
  environment: "base-sepolia-preflight",
  label: "Base Sepolia preflight",
  testOnly: true,
  environmentMode: null,
  activationTimelockDelaySeconds: null,
  icHost: "https://icp-api.io",
  baseRpcUrl: "https://base-sepolia-rpc.publicnode.com",
  baseHistoryRpcUrls: ["https://sepolia.base.org", "https://base-sepolia.api.onfinality.io/public"],
  chainId: 84532,
  bridgeCanisterId: null,
  deploymentInstanceId: null,
  minimumWithdrawalId: null,
  ledgerCanisterId: null,
  indexCanisterId: null,
  icToken: { name: "TEST ICRC1", symbol: "TICRC1", decimals: 8 },
  baseToken: { symbol: "KINIC", decimals: 8 },
  bridgeAddress: null,
  bsnsAddress: null,
  timelockAddress: null,
  expected_bridge_signer: null,
  evmRpcCanisterId: null,
  rpcProviderUrlsSha256: null,
  deploymentBlock: null,
  bridgeRuntimeHash: null,
  bsnsRuntimeHash: null,
}

const viteProfileJson: unknown = import.meta.env?.VITE_DEPLOYMENT_PROFILE_JSON
const globalProfileJson = (
  globalThis as typeof globalThis & { __KINIC_DEPLOYMENT_PROFILE_JSON__?: string }
).__KINIC_DEPLOYMENT_PROFILE_JSON__
const injectedProfileJson =
  typeof globalProfileJson === "string"
    ? globalProfileJson
    : typeof viteProfileJson === "string"
      ? viteProfileJson
      : undefined
const deploymentProfileInput: unknown = injectedProfileJson
  ? (JSON.parse(injectedProfileJson) as unknown)
  : preflightProfile

export const deploymentProfile: DeploymentProfile =
  deploymentProfileSchema.parse(deploymentProfileInput)

export function profileCompleteness(profile: DeploymentProfile): string[] {
  const missing: string[] = []
  if (!profile.bridgeCanisterId) missing.push("Bridge canister ID is missing")
  if (!profile.deploymentInstanceId) missing.push("Deployment instance ID is missing")
  if (!profile.minimumWithdrawalId) missing.push("Minimum withdrawal ID is missing")
  if (!profile.ledgerCanisterId) missing.push("IC token ledger ID is missing")
  if (!profile.indexCanisterId) missing.push("IC token index ID is missing")
  if (!profile.testOnly && !profile.snsRootCanisterId) missing.push("KINIC SNS Root ID is missing")
  if (!profile.bridgeAddress) missing.push("Bridge contract address is missing")
  if (!profile.bsnsAddress) missing.push("bSNS contract address is missing")
  if (!profile.timelockAddress) missing.push("Timelock contract address is missing")
  if (profile.activationTimelockDelaySeconds === null) missing.push("Timelock delay is missing")
  if (!profile.expected_bridge_signer) missing.push("Expected Bridge signer is missing")
  if (!profile.evmRpcCanisterId) missing.push("EVM RPC Canister ID is missing")
  if (!profile.rpcProviderUrlsSha256) missing.push("RPC provider URL digest is missing")
  if (profile.deploymentBlock === null || (!profile.testOnly && profile.deploymentBlock === 0n)) {
    missing.push("Deployment history start block is missing")
  }
  if (!profile.bridgeRuntimeHash) missing.push("Bridge runtime bytecode hash is missing")
  if (!profile.bsnsRuntimeHash) missing.push("bSNS runtime bytecode hash is missing")
  return missing
}
