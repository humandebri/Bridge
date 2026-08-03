#!/usr/bin/env node
import { execFileSync } from "node:child_process"
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
      fail(`${kind} is not a canonical successful pause on ${url}`)
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
  if (!/\bdeposits_paused\s*=\s*true\s*:\s*bool\b/s.test(status)) fail("get_bridge_status did not confirm deposits_paused")
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

export async function collectEvidence(profile, config, live, canisterStatus, dependencies = {}) {
  const fetchImpl = dependencies.fetchImpl ?? fetch
  const runIcpImpl = dependencies.runIcpImpl ?? runIcp
  const now = dependencies.now ?? (() => new Date())
  exactKeys(config, [
    "schema_version", "rpc_urls", "pause_deposit_mints_transaction_hash",
    "pause_withdrawals_transaction_hash", "ic_environment", "audit_cursor",
  ], "capture config")
  if (config.schema_version !== 1 || !Array.isArray(config.rpc_urls) || config.rpc_urls.length !== 3 || new Set(config.rpc_urls).size !== 3) {
    fail("capture config must contain exactly three distinct RPC URLs")
  }
  if (config.rpc_urls.some((url) => typeof url !== "string" || !url.startsWith("https://"))) {
    fail("capture config RPC URLs must use HTTPS")
  }
  for (const expected of Object.values(actions)) {
    if (typeof config[expected.configField] !== "string" || !/^0x[0-9a-fA-F]{64}$/.test(config[expected.configField])) {
      fail(`${expected.configField} must be a transaction hash`)
    }
  }
  if (config.ic_environment !== "sepolia-staging" || !Number.isSafeInteger(config.audit_cursor) || config.audit_cursor < 0) {
    fail("capture config must bind sepolia-staging and a nonnegative audit cursor")
  }
  const bridgeAddress = String(profile.bridgeAddress).toLowerCase()
  const providers = await Promise.all(config.rpc_urls.map((url) => observeProvider(url, bridgeAddress, config, fetchImpl)))
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
  const icPause = observeIcPause(profile.bridgeCanisterId, config.ic_environment, config.audit_cursor, runIcpImpl)
  return {
    schema_version: 1,
    environment: "sepolia-staging",
    observed_at: now().toISOString(),
    bridge_canister_id: profile.bridgeCanisterId,
    chain_id: profile.chainId,
    bridge_address: profile.bridgeAddress,
    live_schema_version: Number(live.schema_version),
    previous_deployment_instance_id: live.deployment_instance_id,
    live_module_hash: canisterStatus.module_hash,
    providers,
    ic_pause: icPause,
    complete: true,
  }
}

async function main() {
  const [, , profileArg, configArg, liveArg, statusArg] = process.argv
  if (!profileArg || !configArg || !liveArg || !statusArg) {
    fail("usage: capture-obsolete-pause-evidence.mjs <frontend-profile.json> <capture-config.json> <live-public-config.json> <live-canister-status.json>")
  }
  const [profile, config, live, canisterStatus] = await Promise.all(
    [profileArg, configArg, liveArg, statusArg].map(async (file) => JSON.parse(await readFile(path.resolve(file), "utf8"))),
  )
  process.stdout.write(`${JSON.stringify(await collectEvidence(profile, config, live, canisterStatus))}\n`)
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  await main()
}
