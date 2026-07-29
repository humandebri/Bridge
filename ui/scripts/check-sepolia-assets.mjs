import { createHash } from "node:crypto"
import { readFile } from "node:fs/promises"
import path from "node:path"
import { fileURLToPath } from "node:url"

const uiRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..")
const profilePath = process.env.BRIDGE_SEPOLIA_PROFILE
  ? path.resolve(process.env.BRIDGE_SEPOLIA_PROFILE)
  : path.resolve(uiRoot, "../deployments/sepolia-staging/frontend-profile.json")
const [rawProfile, builtDigest] = await Promise.all([
  readFile(profilePath, "utf8"),
  readFile(path.join(uiRoot, "dist/.kinic-sepolia-profile-sha256"), "utf8"),
])
const profile = JSON.parse(rawProfile)
const expected = createHash("sha256").update(JSON.stringify(profile)).digest("hex")
if (builtDigest.trim() !== expected) throw new Error("Test deploy rejected: dist was not built from the current completed Sepolia profile")
for (const key of ["environmentMode", "activationTimelockDelaySeconds", "bridgeCanisterId", "ledgerCanisterId", "indexCanisterId", "bridgeAddress", "bsnsAddress", "timelockAddress", "expected_bridge_signer", "bridgeRuntimeHash", "bsnsRuntimeHash", "rpcProviderUrlsSha256", "deploymentBlock"]) {
  if (profile[key] === null || profile[key] === undefined || profile[key] === "" || String(profile[key]).startsWith("REPLACE_")) {
    throw new Error(`Test deploy rejected incomplete profile field: ${key}`)
  }
}
if (profile.environmentMode !== "short-delay-test-only" || profile.activationTimelockDelaySeconds !== 300) {
  throw new Error("Test deploy rejected non-staging Timelock policy")
}
