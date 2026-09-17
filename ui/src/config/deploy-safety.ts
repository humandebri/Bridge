export interface UiDeploymentMode {
  mintRecoveryUrl?: string
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
  deploymentBlock?: bigint | number | string | null
  profileFileSha256?: string | null
  profileCanonicalSha256?: string | null
  canisterSchemaVersion?: number
  canisterModuleSha256?: string
  postActivationUpgradeSha256?: string
  uiRpcConfigSha256?: string
  timelockAddress?: string | null
}

export const MINIMUM_PRODUCTION_TIMELOCK_DELAY_SECONDS = 24 * 60 * 60

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
  if (profile.mintRecoveryUrl !== "https://recovery.bridge.kinic.xyz/v1/mint-recovery")
    throw new Error("Production UI requires the reviewed mint recovery URL")
  let deploymentBlock: bigint
  try {
    deploymentBlock = BigInt(profile.deploymentBlock ?? 0)
  } catch {
    throw new Error("Production UI deploy requires a positive deployment block")
  }
  if (deploymentBlock <= 0n) {
    throw new Error("Production UI deploy requires a positive deployment block")
  }
  if (
    ![profile.profileFileSha256, profile.profileCanonicalSha256].every(
      (value) => /^[0-9a-f]{64}$/i.test(value ?? "") && !/^0+$/.test(value ?? ""),
    )
  ) {
    throw new Error("Production UI profile requires nonzero source profile hashes")
  }
  if (
    profile.canisterSchemaVersion !== 36 ||
    ![
      profile.canisterModuleSha256,
      profile.postActivationUpgradeSha256,
      profile.uiRpcConfigSha256,
    ].every((value) => /^[0-9a-f]{64}$/i.test(value ?? "") && !/^0+$/.test(value ?? ""))
  ) {
    throw new Error(
      "Production UI requires the v36 module, upgrade chain, and reviewed RPC bindings",
    )
  }
}
