export interface UiDeploymentMode {
  testOnly?: boolean
  gateBManifestSha256?: string | null
  profileFileSha256?: string | null
  profileCanonicalSha256?: string | null
}

export function assertProductionUiProfile(profile: UiDeploymentMode, verifiedManifestSha256?: string): void {
  if (profile.testOnly !== false) throw new Error("Production UI deploy rejects test-only or unspecified deployment profiles")
  if (!/^[0-9a-f]{64}$/i.test(verifiedManifestSha256 ?? "") || /^0+$/.test(verifiedManifestSha256 ?? "")) {
    throw new Error("Production UI deploy requires a verified Gate B manifest hash")
  }
  if (profile.gateBManifestSha256?.toLowerCase() !== verifiedManifestSha256?.toLowerCase()) {
    throw new Error("Production UI profile does not match the verified Gate B manifest")
  }
  if (![profile.profileFileSha256, profile.profileCanonicalSha256].every((value) => /^[0-9a-f]{64}$/i.test(value ?? "") && !/^0+$/.test(value ?? ""))) {
    throw new Error("Production UI profile requires nonzero source profile hashes")
  }
}
