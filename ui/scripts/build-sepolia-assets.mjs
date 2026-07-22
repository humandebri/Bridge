import { readFile, writeFile } from "node:fs/promises"
import { createHash } from "node:crypto"
import { spawnSync } from "node:child_process"
import path from "node:path"
import { fileURLToPath } from "node:url"

const uiRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..")
const profilePath = process.env.BRIDGE_SEPOLIA_PROFILE
  ? path.resolve(process.env.BRIDGE_SEPOLIA_PROFILE)
  : path.resolve(uiRoot, "../deployments/sepolia-staging/frontend-profile.json")
const profile = JSON.parse(await readFile(profilePath, "utf8"))

const productionIds = new Set([
  "73mez-iiaaa-aaaaq-aaasq-cai",
  "7vojr-tyaaa-aaaaq-aaatq-cai",
])
if (profile.environment !== "sepolia-staging" || profile.testOnly !== true || profile.chainId !== 84532) {
  throw new Error("Asset build requires the sepolia-staging test-only Base Sepolia profile")
}
if (profile.evmRpcCanisterId !== "7hfb6-caaaa-aaaar-qadga-cai") {
  throw new Error("Asset build requires the official EVM RPC Canister")
}
for (const key of ["bridgeCanisterId", "ledgerCanisterId", "indexCanisterId"]) {
  if (typeof profile[key] !== "string" || profile[key].startsWith("REPLACE_") || productionIds.has(profile[key])) {
    throw new Error(`Asset build rejects missing or production ${key}`)
  }
}
for (const key of ["bridgeAddress", "bsnsAddress", "expected_bridge_signer"]) {
  if (!/^0x[0-9a-fA-F]{40}$/.test(profile[key] ?? "")) throw new Error(`Asset build requires ${key}`)
}
for (const key of ["bridgeRuntimeHash", "bsnsRuntimeHash", "rpcProviderUrlsSha256"]) {
  if (!/^0x[0-9a-fA-F]{64}$/.test(profile[key] ?? "") || /^0x0+$/.test(profile[key])) {
    throw new Error(`Asset build requires nonzero ${key}`)
  }
}
if (!/^\d+$/.test(String(profile.deploymentBlock)) || BigInt(profile.deploymentBlock) <= 0n) {
  throw new Error("Asset build requires a positive deploymentBlock")
}

const result = spawnSync("pnpm", ["run", "build"], {
  cwd: uiRoot,
  stdio: "inherit",
  env: { ...process.env, VITE_DEPLOYMENT_PROFILE_JSON: JSON.stringify(profile) },
})
if (result.error) throw result.error
if (result.status !== 0) process.exit(result.status ?? 1)
const profileSha256 = createHash("sha256").update(JSON.stringify(profile)).digest("hex")
await writeFile(path.join(uiRoot, "dist/.kinic-sepolia-profile-sha256"), `${profileSha256}\n`, { flag: "w" })
