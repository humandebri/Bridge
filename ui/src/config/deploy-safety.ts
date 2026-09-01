export interface UiDeploymentMode {
  environment?: string
  testOnly?: boolean
  environmentMode?: string | null
  activationTimelockDelaySeconds?: number | null
  chainId?: number
  bridgeCanisterId?: string | null
  ledgerCanisterId?: string | null
  indexCanisterId?: string | null
  evmRpcCanisterId?: string | null
  gateBManifestSha256?: string | null
  profileFileSha256?: string | null
  profileCanonicalSha256?: string | null
  timelockAddress?: string | null
}

export const BASE_MAINNET_CHAIN_ID = 8453
export const BASE_SEPOLIA_CHAIN_ID = 84532
export const OFFICIAL_EVM_RPC_CANISTER_ID = "7hfb6-caaaa-aaaar-qadga-cai"
export const MINIMUM_PRODUCTION_TIMELOCK_DELAY_SECONDS = 24 * 60 * 60
export const PRODUCTION_CANISTER_IDS = new Set([
  "73mez-iiaaa-aaaaq-aaasq-cai",
  "7vojr-tyaaa-aaaaq-aaatq-cai",
])

export function assertTestUiProfile(profile: UiDeploymentMode): void {
  if (profile.testOnly !== true) throw new Error("Test UI deploy requires testOnly: true")
  if (profile.chainId === BASE_MAINNET_CHAIN_ID)
    throw new Error("Test UI deploy rejects Base Mainnet")
  for (const canisterId of [
    profile.bridgeCanisterId,
    profile.ledgerCanisterId,
    profile.indexCanisterId,
  ]) {
    if (canisterId && PRODUCTION_CANISTER_IDS.has(canisterId)) {
      throw new Error("Test UI deploy rejects production canister IDs")
    }
  }
  if (profile.environment === "sepolia-staging") {
    if (profile.chainId !== BASE_SEPOLIA_CHAIN_ID)
      throw new Error("Sepolia staging requires Base Sepolia chain ID 84532")
    if (profile.evmRpcCanisterId !== OFFICIAL_EVM_RPC_CANISTER_ID) {
      throw new Error("Sepolia staging requires the official EVM RPC Canister")
    }
    if (
      profile.environmentMode !== "short-delay-test-only" ||
      profile.activationTimelockDelaySeconds !== 300
    ) {
      throw new Error("Sepolia staging requires the five-minute test-only Timelock policy")
    }
  }
}

export function assertProductionUiProfile(
  profile: UiDeploymentMode,
  verifiedManifestSha256?: string,
): void {
  if (profile.testOnly !== false)
    throw new Error("Production UI deploy rejects test-only or unspecified deployment profiles")
  if (profile.environmentMode !== null)
    throw new Error("Production UI deploy rejects test-only or unspecified environment modes")
  const timelockDelay = profile.activationTimelockDelaySeconds
  if (
    typeof timelockDelay !== "number" ||
    !Number.isSafeInteger(timelockDelay) ||
    timelockDelay < MINIMUM_PRODUCTION_TIMELOCK_DELAY_SECONDS
  ) {
    throw new Error("Production UI deploy requires a Timelock delay of at least 24 hours")
  }
  if (!/^0x[0-9a-fA-F]{40}$/.test(profile.timelockAddress ?? "")) {
    throw new Error("Production UI deploy requires a Timelock contract address")
  }
  if (
    !/^[0-9a-f]{64}$/i.test(verifiedManifestSha256 ?? "") ||
    /^0+$/.test(verifiedManifestSha256 ?? "")
  ) {
    throw new Error("Production UI deploy requires a verified Gate B manifest hash")
  }
  if (profile.gateBManifestSha256?.toLowerCase() !== verifiedManifestSha256?.toLowerCase()) {
    throw new Error("Production UI profile does not match the verified Gate B manifest")
  }
  if (
    ![profile.profileFileSha256, profile.profileCanonicalSha256].every(
      (value) => /^[0-9a-f]{64}$/i.test(value ?? "") && !/^0+$/.test(value ?? ""),
    )
  ) {
    throw new Error("Production UI profile requires nonzero source profile hashes")
  }
}
