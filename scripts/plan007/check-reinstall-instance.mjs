import { readFile } from "node:fs/promises"
import path from "node:path"
import { fileURLToPath } from "node:url"

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

export function verifyReinstallInstance(profile, livePublicConfig) {
  const next = deploymentInstanceHex(profile?.deploymentInstanceId, "frontend profile deploymentInstanceId")
  const schemaVersion = Number(livePublicConfig?.schema_version)
  if (!Number.isInteger(schemaVersion)) {
    throw new Error("live PublicConfig schema_version must be an integer")
  }
  if (schemaVersion === 29) {
    if (livePublicConfig?.deployment_instance_id != null) {
      throw new Error("v29 live PublicConfig must not contain a deployment instance ID")
    }
    return { live_schema_version: schemaVersion, previous_deployment_instance_id: null, next }
  }
  if (schemaVersion !== 30) {
    throw new Error("staging reinstall only accepts live stable schema v29 or v30")
  }
  const previous = deploymentInstanceHex(
    livePublicConfig?.deployment_instance_id,
    "live PublicConfig deployment_instance_id",
  )
  if (next === previous) {
    throw new Error("staging reinstall rejected reuse of the live deployment instance ID")
  }
  return { live_schema_version: schemaVersion, previous_deployment_instance_id: previous, next }
}

async function main() {
  const [, , profileArg, liveArg] = process.argv
  if (!profileArg || !liveArg) {
    throw new Error("usage: check-reinstall-instance.mjs <frontend-profile.json> <live-public-config.json>")
  }
  const [profile, live] = await Promise.all([
    readFile(path.resolve(profileArg), "utf8").then(JSON.parse),
    readFile(path.resolve(liveArg), "utf8").then(JSON.parse),
  ])
  process.stdout.write(`${JSON.stringify(verifyReinstallInstance(profile, live))}\n`)
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  await main()
}
