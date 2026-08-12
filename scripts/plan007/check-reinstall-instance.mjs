import { readFile } from "node:fs"
import { promisify } from "node:util"
import path from "node:path"
import { fileURLToPath } from "node:url"

const readFileAsync = promisify(readFile)
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
  if (![32, 33].includes(schemaVersion)) {
    throw new Error("staging install check only accepts current schema v33 or explicitly discarded schema v32")
  }
  const previous = deploymentInstanceHex(
    livePublicConfig?.deployment_instance_id,
    "live PublicConfig deployment_instance_id",
  )
  const liveModuleHash = moduleHash(liveCanisterStatus?.module_hash, "live canister status module_hash")
  if (schemaVersion === 32 && next === previous) {
    throw new Error("obsolete schema v32 reinstall requires a distinct deployment instance ID")
  }
  const replacementMode = schemaVersion === 32
    ? "obsolete-schema-reinstall"
    : next === previous
      ? "current-schema-upgrade"
      : "current-schema-reinstall"
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
