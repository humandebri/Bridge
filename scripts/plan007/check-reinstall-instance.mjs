import { readFile, readFileSync } from "node:fs"
import { promisify } from "node:util"
import path from "node:path"
import { fileURLToPath } from "node:url"

const readFileAsync = promisify(readFile)
const policyPath = fileURLToPath(new URL("../../deployments/sepolia-staging/obsolete-replacement-policy.json", import.meta.url))
export const obsoleteReplacementPolicy = JSON.parse(readFileSync(policyPath, "utf8"))

export function deploymentInstanceHex(value, context) {
  if (typeof value === "string" && /^0x[0-9a-fA-F]{64}$/.test(value) && !/^0x0+$/.test(value)) {
    return value.toLowerCase()
  }
  if (
    Array.isArray(value)
    && value.length === 32
    && value.some((byte) => byte !== 0)
    && value.every((byte) => Number.isInteger(byte) && byte >= 0 && byte <= 255)
  ) {
    return `0x${value.map((byte) => byte.toString(16).padStart(2, "0")).join("")}`
  }
  throw new Error(`${context} must be a nonzero 32-byte deployment instance ID`)
}

function moduleHash(value, context) {
  if (typeof value !== "string" || !/^0x[0-9a-fA-F]{64}$/.test(value) || /^0x0+$/.test(value)) {
    throw new Error(`${context} must be a nonzero 32-byte module hash`)
  }
  return value.toLowerCase()
}

export function verifyReinstallInstance(profile, livePublicConfig, liveCanisterStatus) {
  const next = deploymentInstanceHex(profile?.deploymentInstanceId, "frontend profile deploymentInstanceId")
  const schemaVersion = Number(livePublicConfig?.schema_version)
  if (!Number.isInteger(schemaVersion)) {
    throw new Error("live PublicConfig schema_version must be an integer")
  }
  if (schemaVersion !== 31 && schemaVersion !== 30) {
    throw new Error("staging install check only accepts current schema v31 or audited obsolete schema v30")
  }
  const previous = deploymentInstanceHex(
    livePublicConfig?.deployment_instance_id,
    "live PublicConfig deployment_instance_id",
  )
  if (schemaVersion === 31 && next === previous) {
    throw new Error("staging reinstall rejected reuse of the live deployment instance ID")
  }
  if (schemaVersion === 30 && next !== previous) {
    throw new Error("staging upgrade must preserve the live deployment instance ID")
  }
  const liveModuleHash = moduleHash(liveCanisterStatus?.module_hash, "live canister status module_hash")
  const replacementMode = schemaVersion === 31
    ? "current-schema-reinstall"
    : "obsolete-schema-upgrade"
  if (replacementMode === "obsolete-schema-upgrade") {
    if (
      profile?.bridgeCanisterId !== obsoleteReplacementPolicy.bridge_canister_id
      || schemaVersion !== obsoleteReplacementPolicy.live_schema_version
      || previous !== obsoleteReplacementPolicy.previous_deployment_instance_id
      || liveModuleHash !== obsoleteReplacementPolicy.module_hash
    ) {
      throw new Error("obsolete staging upgrade does not match the reviewed replacement policy")
    }
  }
  return {
    replacement_mode: replacementMode,
    live_schema_version: schemaVersion,
    previous_deployment_instance_id: previous,
    live_module_hash: liveModuleHash,
    next,
  }
}

async function main() {
  const [, , profileArg, liveArg, statusArg] = process.argv
  if (!profileArg || !liveArg || !statusArg) {
    throw new Error("usage: check-reinstall-instance.mjs <frontend-profile.json> <live-public-config.json> <live-canister-status.json>")
  }
  const [profile, live, status] = await Promise.all([
    readFileAsync(path.resolve(profileArg), "utf8").then(JSON.parse),
    readFileAsync(path.resolve(liveArg), "utf8").then(JSON.parse),
    readFileAsync(path.resolve(statusArg), "utf8").then(JSON.parse),
  ])
  process.stdout.write(`${JSON.stringify(verifyReinstallInstance(profile, live, status))}\n`)
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  await main()
}
