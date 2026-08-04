#!/usr/bin/env node
import { execFileSync } from "node:child_process"
import { randomUUID } from "node:crypto"
import { readFile } from "node:fs/promises"
import path from "node:path"
import { fileURLToPath } from "node:url"

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..")
const didPath = path.join(root, "canister/bridge-canister/bridge.did")
const actions = {
  PauseDepositMints: {
    configField: "pause_deposit_mints_transaction_hash",
    calldata: "0x15415f22",
    eventTopic: "0x7a8cbbf7de1b70cf6a63059c06484e4a6ca4b28f18ced89f03ea751608fc29a1",
  },
  PauseWithdrawals: {
    configField: "pause_withdrawals_transaction_hash",
    calldata: "0x56bb54a7",
    eventTopic: "0x7c82b8b6bc44325506945ff406eeb0f2add5b91cfdd2265e80994967d30a787d",
  },
}

function fail(message) {
  throw new Error(message)
}

function exactKeys(value, expected, context) {
  if (!value || typeof value !== "object" || Array.isArray(value)) fail(`${context} must be an object`)
  const actual = Object.keys(value).sort()
  const wanted = [...expected].sort()
  if (JSON.stringify(actual) !== JSON.stringify(wanted)) fail(`${context} fields differ`)
}

function hexQuantity(value, context) {
  if (typeof value !== "string" || !/^0x[0-9a-fA-F]+$/.test(value)) fail(`${context} is not a hex quantity`)
  return Number.parseInt(value, 16)
}

async function rpc(url, method, params, fetchImpl) {
  const response = await fetchImpl(url, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ jsonrpc: "2.0", id: 1, method, params }),
  })
  if (!response.ok) fail(`${method} returned HTTP ${response.status}`)
  const payload = await response.json()
  if (payload?.error || payload?.result === undefined || payload?.result === null) {
    fail(`${method} returned no usable result`)
  }
  return payload.result
}

async function sha256(value) {
  const bytes = new TextEncoder().encode(value)
  return Buffer.from(await crypto.subtle.digest("SHA-256", bytes)).toString("hex")
}

function validateCredentialFreeRpcUrl(value, index) {
  let parsed
  try {
    parsed = new URL(value)
  } catch {
    fail(`capture config RPC provider ${index + 1} is not a valid URL`)
  }
  if (
    parsed.protocol !== "https:"
    || !parsed.hostname
    || parsed.username
    || parsed.password
    || parsed.search
    || parsed.hash
  ) {
    fail(`capture config RPC provider ${index + 1} must be a credential-free HTTPS origin or path`)
  }
}

export async function observeProvider(url, bridgeAddress, config, fetchImpl = fetch) {
  const chainId = hexQuantity(await rpc(url, "eth_chainId", [], fetchImpl), "eth_chainId")
  const finalized = await rpc(url, "eth_getBlockByNumber", ["finalized", false], fetchImpl)
  const finalizedNumber = hexQuantity(finalized.number, "finalized block number")
  const observedActions = []
  for (const [kind, expected] of Object.entries(actions)) {
    const transactionHash = config[expected.configField]
    const [transaction, receipt] = await Promise.all([
      rpc(url, "eth_getTransactionByHash", [transactionHash], fetchImpl),
      rpc(url, "eth_getTransactionReceipt", [transactionHash], fetchImpl),
    ])
    const block = await rpc(url, "eth_getBlockByNumber", [receipt.blockNumber, false], fetchImpl)
    const target = String(transaction.to).toLowerCase()
    const calldata = String(transaction.input).toLowerCase()
    const blockHash = String(receipt.blockHash).toLowerCase()
    const canonical = String(block.hash).toLowerCase() === blockHash
    const eventObserved = Array.isArray(receipt.logs) && receipt.logs.some((log) => (
      String(log.address).toLowerCase() === bridgeAddress
      && String(log.topics?.[0]).toLowerCase() === expected.eventTopic
    ))
    const receiptStatus = hexQuantity(receipt.status, `${kind} receipt status`)
    const blockNumber = hexQuantity(receipt.blockNumber, `${kind} block number`)
    if (
      String(transaction.hash).toLowerCase() !== transactionHash.toLowerCase()
      || String(receipt.transactionHash).toLowerCase() !== transactionHash.toLowerCase()
      || receiptStatus !== 1
      || blockNumber > finalizedNumber
      || target !== bridgeAddress
      || calldata !== expected.calldata
      || !eventObserved
      || !canonical
    ) {
      fail(`${kind} is not a canonical successful pause on provider ${await sha256(url)}`)
    }
    observedActions.push({
      kind,
      transaction_hash: String(transactionHash).toLowerCase(),
      block_number: blockNumber,
      block_hash: blockHash,
      receipt_status: receiptStatus,
      target,
      calldata_hex: calldata,
      event_topic: expected.eventTopic,
      event_observed: eventObserved,
      canonical_probe_succeeded: canonical,
    })
  }
  return {
    provider_url_sha256: await sha256(url),
    chain_id: chainId,
    finalized_block_number: finalizedNumber,
    finalized_block_hash: String(finalized.hash).toLowerCase(),
    actions: observedActions,
  }
}

function runCanisterStatus(canisterId, environment) {
  const output = execFileSync(
    "icp",
    ["canister", "status", canisterId, "-e", environment, "--json", "--project-root-override", root],
    { encoding: "utf8", stdio: ["ignore", "pipe", "inherit"] },
  )
  return JSON.parse(output)
}

function candidNat(candid, field, context) {
  const matches = [...candid.matchAll(new RegExp(`\\b${field}\\s*=\\s*(\\d+)\\s*:\\s*nat(?:16|32|64)?\\b`, "g"))]
  if (matches.length !== 1) fail(`${context} did not expose exactly one ${field}`)
  return Number(matches[0][1])
}

function candidBlob32(candid, field, context) {
  const vector = candid.match(new RegExp(`\\b${field}\\s*=\\s*vec\\s*\\{([^}]*)\\}`, "s"))
  const blob = candid.match(new RegExp(`\\b${field}\\s*=\\s*blob\\s*"([^"]*)"`, "s"))
  const bytes = vector
    ? [...vector[1].matchAll(/(\d+)\s*:\s*nat8/g)].map((item) => Number(item[1]))
    : blob
      ? [...blob[1].matchAll(/\\([0-9a-fA-F]{2})/g)].map((item) => Number.parseInt(item[1], 16))
      : []
  if (bytes.length !== 32 || bytes.every((byte) => byte === 0) || bytes.some((byte) => byte < 0 || byte > 255)) {
    fail(`${context} ${field} is not a nonzero 32-byte value`)
  }
  return `0x${Buffer.from(bytes).toString("hex")}`
}

function moduleHash(value) {
  const raw = value?.module_hash ?? value?.moduleHash
  const lowered = typeof raw === "string" ? raw.toLowerCase() : ""
  const normalized = /^[0-9a-f]{64}$/.test(lowered) ? `0x${lowered}` : lowered
  if (!/^0x[0-9a-f]{64}$/.test(normalized)) fail("canister status did not expose a module hash")
  return normalized
}

export function observeIcLive(canisterId, environment, runIcpImpl = runIcp, runStatusImpl = runCanisterStatus) {
  const publicConfig = runIcpImpl(canisterId, environment, "get_public_config", "()", true)
  const bridgeStatus = runIcpImpl(canisterId, environment, "get_bridge_status", "()", true)
  if (!/\bdeposits_paused\s*=\s*true(?:\s*:\s*bool)?\s*;/s.test(bridgeStatus)) {
    fail("post-pause BridgeStatus did not confirm deposits_paused")
  }
  return {
    live_schema_version: candidNat(publicConfig, "schema_version", "get_public_config"),
    previous_deployment_instance_id: candidBlob32(publicConfig, "deployment_instance_id", "get_public_config"),
    live_module_hash: moduleHash(runStatusImpl(canisterId, environment)),
    status_deposits_paused: true,
  }
}

function runIcp(canisterId, environment, method, args, query = false) {
  const command = [
    "canister", "call", canisterId, method, args,
    "-e", environment, "--json", "--candid", didPath,
    "--project-root-override", root,
  ]
  if (query) command.push("--query")
  const output = execFileSync("icp", command, { encoding: "utf8", stdio: ["ignore", "pipe", "inherit"] })
  const parsed = JSON.parse(output)
  if (typeof parsed.response_candid !== "string") fail(`${method} did not return response_candid`)
  return parsed.response_candid
}

export function observeIcPause(canisterId, environment, auditCursor, runIcpImpl = runIcp) {
  const pause = runIcpImpl(canisterId, environment, "pause_new_deposits", "()")
  if (!/variant\s*\{\s*Ok\s*\}/s.test(pause)) fail("pause_new_deposits did not return Ok")
  const status = runIcpImpl(canisterId, environment, "get_bridge_status", "()", true)
  if (!/\bdeposits_paused\s*=\s*true(?:\s*:\s*bool)?\s*;/s.test(status)) fail("get_bridge_status did not confirm deposits_paused")
  const statusSequences = [...status.matchAll(/\blast_audit_sequence\s*=\s*opt\s*\(\s*(\d+)\s*:\s*nat64\s*\)/g)]
  if (statusSequences.length !== 1) fail("get_bridge_status did not expose exactly one nat64 last_audit_sequence")
  const statusSequence = statusSequences[0][1]
  const audit = runIcpImpl(canisterId, environment, "get_audit_events", `(${auditCursor} : nat64, 100 : nat16)`, true)
  const matches = [...audit.matchAll(/kind\s*=\s*variant\s*\{\s*(DepositsPaused|DepositsPauseRepeated)\s*\}[\s\S]*?caller\s*=\s*principal\s*"([^"]+)"[\s\S]*?sequence\s*=\s*(\d+)\s*:\s*nat64/g)]
  const selected = matches.map((match) => ({ kind: match[1], caller: match[2], sequence: Number(match[3]) }))
    .findLast((event) => event.sequence === Number(statusSequence))
  if (!selected) fail("get_audit_events did not contain the pause event referenced by BridgeStatus")
  return {
    method: "pause_new_deposits",
    response: "Ok",
    audit_sequence: selected.sequence,
    audit_kind: selected.kind,
    audit_caller: selected.caller,
    status_deposits_paused: true,
    status_last_audit_sequence: Number(statusSequence),
  }
}

export async function collectEvidence(profile, config, dependencies = {}) {
  const fetchImpl = dependencies.fetchImpl ?? fetch
  const runIcpImpl = dependencies.runIcpImpl ?? runIcp
  const runStatusImpl = dependencies.runStatusImpl ?? runCanisterStatus
  const now = dependencies.now ?? (() => new Date())
  const captureId = dependencies.captureId ?? randomUUID()
  const captureStartedAt = now().toISOString()
  exactKeys(config, [
    "schema_version", "rpc_urls", "pause_deposit_mints_transaction_hash",
    "pause_withdrawals_transaction_hash", "ic_environment", "audit_cursor",
  ], "capture config")
  if (config.schema_version !== 1 || !Array.isArray(config.rpc_urls) || config.rpc_urls.length !== 3 || new Set(config.rpc_urls).size !== 3) {
    fail("capture config must contain exactly three distinct RPC URLs")
  }
  config.rpc_urls.forEach(validateCredentialFreeRpcUrl)
  for (const expected of Object.values(actions)) {
    if (typeof config[expected.configField] !== "string" || !/^0x[0-9a-fA-F]{64}$/.test(config[expected.configField])) {
      fail(`${expected.configField} must be a transaction hash`)
    }
  }
  if (config.ic_environment !== "sepolia-staging" || !Number.isSafeInteger(config.audit_cursor) || config.audit_cursor < 0) {
    fail("capture config must bind sepolia-staging and a nonnegative audit cursor")
  }
  const bridgeAddress = String(profile.bridgeAddress).toLowerCase()
  const providers = await Promise.all(config.rpc_urls.map(async (url) => ({
    ...(await observeProvider(url, bridgeAddress, config, fetchImpl)),
    observed_at: now().toISOString(),
  })))
  if (providers.some((provider) => provider.chain_id !== profile.chainId)) {
    fail("a Base RPC provider observed the wrong chain")
  }
  const heads = providers.map((provider) => `${provider.finalized_block_number}:${provider.finalized_block_hash}`)
  const agreedHead = heads.find((head) => heads.filter((candidate) => candidate === head).length >= 2)
  if (!agreedHead) fail("Finalized head has no 2-of-3 agreement")
  const eligible = providers.filter((_provider, index) => heads[index] === agreedHead)
  for (const kind of Object.keys(actions)) {
    const observations = eligible.map((provider) => JSON.stringify(provider.actions.find((action) => action.kind === kind)))
    if (!observations.some((observation) => observations.filter((candidate) => candidate === observation).length >= 2)) {
      fail(`${kind} has no 2-of-3 canonical agreement`)
    }
  }
  const icPause = {
    ...observeIcPause(profile.bridgeCanisterId, config.ic_environment, config.audit_cursor, runIcpImpl),
    observed_at: now().toISOString(),
  }
  const icLive = {
    ...observeIcLive(profile.bridgeCanisterId, config.ic_environment, runIcpImpl, runStatusImpl),
    observed_at: now().toISOString(),
  }
  return {
    schema_version: 2,
    environment: "sepolia-staging",
    capture_id: captureId,
    capture_started_at: captureStartedAt,
    observed_at: now().toISOString(),
    bridge_canister_id: profile.bridgeCanisterId,
    chain_id: profile.chainId,
    bridge_address: profile.bridgeAddress,
    providers,
    ic_pause: icPause,
    ic_live: icLive,
    complete: true,
  }
}

async function main() {
  const [, , profileArg, configArg] = process.argv
  if (!profileArg || !configArg) {
    fail("usage: capture-obsolete-pause-evidence.mjs <frontend-profile.json> <capture-config.json>")
  }
  const [profile, config] = await Promise.all(
    [profileArg, configArg].map(async (file) => JSON.parse(await readFile(path.resolve(file), "utf8"))),
  )
  process.stdout.write(`${JSON.stringify(await collectEvidence(profile, config))}\n`)
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  await main()
}
