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

export function verifyUpgradeInstance(profile, liveRuntimeBinding, liveCanisterStatus) {
  const next = deploymentInstanceHex(profile?.deploymentInstanceId, "frontend profile deploymentInstanceId")
  const schemaVersion = Number(liveRuntimeBinding?.schema_version)
  if (![33, 35].includes(schemaVersion)) throw new Error("staging upgrade requires reviewed source schema v33 or target schema v35")
  const previous = deploymentInstanceHex(
    liveRuntimeBinding?.deployment_instance_id,
    "live RuntimeBinding deployment_instance_id",
  )
  if (next !== previous) {
    throw new Error("reinstall is prohibited: staging upgrade must preserve the deployment instance ID")
  }
  return {
    replacement_mode: schemaVersion === 33 ? "schema-migration-upgrade" : "current-schema-upgrade",
    live_schema_version: schemaVersion,
    previous_deployment_instance_id: previous,
    live_module_hash: moduleHash(liveCanisterStatus?.module_hash, "live canister status module_hash"),
    next,
  }
}

async function main() {
  const [, , profileArg, liveArg, statusArg] = process.argv
  if (!profileArg || !liveArg || !statusArg) {
    throw new Error("usage: check-upgrade-instance.mjs <frontend-profile.json> <live-public-config.json> <live-canister-status.json>")
  }
  const [profile, live, status] = await Promise.all([
    readFileAsync(path.resolve(profileArg), "utf8").then(JSON.parse),
    readFileAsync(path.resolve(liveArg), "utf8").then(JSON.parse),
    readFileAsync(path.resolve(statusArg), "utf8").then(JSON.parse),
  ])
  process.stdout.write(`${JSON.stringify(verifyUpgradeInstance(profile, live, status))}\n`)
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) await main()
