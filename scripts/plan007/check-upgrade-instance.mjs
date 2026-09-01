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

function digestHex(value, context) {
  if (typeof value === "string" && /^(?:0x)?[0-9a-fA-F]{64}$/.test(value)) {
    return `0x${value.replace(/^0x/i, "").toLowerCase()}`
  }
  if (Array.isArray(value) && value.length === 32
    && value.every((byte) => Number.isInteger(byte) && byte >= 0 && byte <= 255)) {
    return `0x${value.map((byte) => byte.toString(16).padStart(2, "0")).join("")}`
  }
  throw new Error(`${context} must be a 32-byte SHA-256 digest`)
}

function requireExactKeys(value, expected, context) {
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    throw new Error(`${context} must be an object`)
  }
  const actual = Object.keys(value).sort()
  const required = [...expected].sort()
  if (actual.length !== required.length || actual.some((key, index) => key !== required[index])) {
    throw new Error(`${context} fields differ from the required status contract`)
  }
}

export function verifyUpgradeInstance(profile, liveRuntimeBinding, liveCanisterStatus) {
  requireExactKeys(
    liveCanisterStatus,
    ["canister_id", "module_hash", "controller_principals", "cycles_balance"],
    "live canister status",
  )
  if (typeof profile?.bridgeCanisterId !== "string"
    || liveCanisterStatus.canister_id !== profile.bridgeCanisterId) {
    throw new Error("live canister status is not bound to the reviewed Bridge canister")
  }
  const next = deploymentInstanceHex(profile?.deploymentInstanceId, "frontend profile deploymentInstanceId")
  const schemaVersion = Number(liveRuntimeBinding?.schema_version)
  if (schemaVersion !== 35) {
    throw new Error("staging upgrade requires current schema v35; old and unknown schemas are unsupported")
  }
  const previous = deploymentInstanceHex(
    liveRuntimeBinding?.deployment_instance_id,
    "live RuntimeBinding deployment_instance_id",
  )
  if (next !== previous) {
    throw new Error("reinstall is prohibited: staging upgrade must preserve the deployment instance ID")
  }
  if (digestHex(liveRuntimeBinding?.rpc_provider_urls_sha256, "live RuntimeBinding rpc_provider_urls_sha256")
    !== digestHex(profile?.rpcProviderUrlsSha256, "frontend profile rpcProviderUrlsSha256")) {
    throw new Error("live RuntimeBinding RPC provider digest differs from the reviewed profile")
  }
  return {
    replacement_mode: "current-schema-upgrade",
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
