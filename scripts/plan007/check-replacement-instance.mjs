import { readFile } from "node:fs"
import { promisify } from "node:util"
import path from "node:path"
import { fileURLToPath } from "node:url"

const readFileAsync = promisify(readFile)

export function deploymentInstanceHex(value, context) {
  if (typeof value === "string" && /^0x[0-9a-fA-F]{64}$/.test(value) && !/^0x0+$/.test(value)) {
    return value.toLowerCase()
  }
  if (Array.isArray(value) && value.length === 32 && value.some((byte) => byte !== 0)
    && value.every((byte) => Number.isInteger(byte) && byte >= 0 && byte <= 255)) {
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

const reviewedLegacyInstance = "0x2c344ca868267bc19791a2e1a966c210eef545945b3879223d3cb6c1255d789b"
const reviewedLegacyModule = "0xedecad666c6777f7d0d6d5f47851d71c4df77633826b7b3906578a6950632697"

export function verifyReplacementInstance(profile, liveRuntimeBinding, liveCanisterStatus) {
  const next = deploymentInstanceHex(profile?.deploymentInstanceId, "frontend profile deploymentInstanceId")
  const schemaVersion = Number(liveRuntimeBinding?.schema_version)
  if (schemaVersion !== 34) {
    throw new Error("staging replacement requires the reviewed legacy schema v34")
  }
  const previous = deploymentInstanceHex(
    liveRuntimeBinding?.deployment_instance_id,
    "live RuntimeBinding deployment_instance_id",
  )
  if (previous !== reviewedLegacyInstance) {
    throw new Error("live deployment instance differs from reviewed abandonment evidence")
  }
  if (next === previous) {
    throw new Error("destructive replacement requires a fresh deployment instance ID")
  }
  const liveModuleHash = moduleHash(liveCanisterStatus?.module_hash, "live canister status module_hash")
  if (liveModuleHash !== reviewedLegacyModule) {
    throw new Error("live module hash differs from reviewed abandonment evidence")
  }
  return {
    replacement_mode: "destructive-reinstall",
    live_schema_version: schemaVersion,
    previous_deployment_instance_id: previous,
    live_module_hash: liveModuleHash,
    next,
  }
}

async function main() {
  const [, , profileArg, liveArg, statusArg] = process.argv
  if (!profileArg || !liveArg || !statusArg) {
    throw new Error("usage: check-replacement-instance.mjs <frontend-profile.json> <live-public-config.json> <live-canister-status.json>")
  }
  const [profile, live, status] = await Promise.all([
    readFileAsync(path.resolve(profileArg), "utf8").then(JSON.parse),
    readFileAsync(path.resolve(liveArg), "utf8").then(JSON.parse),
    readFileAsync(path.resolve(statusArg), "utf8").then(JSON.parse),
  ])
  process.stdout.write(`${JSON.stringify(verifyReplacementInstance(profile, live, status))}\n`)
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) await main()
