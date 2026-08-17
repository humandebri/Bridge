#!/usr/bin/env node
import { createHash } from "node:crypto"
import { readFile } from "node:fs/promises"
import path from "node:path"
import { fileURLToPath } from "node:url"

const WITHDRAWALS_PAUSED_SELECTOR = "0xe9f2838e"
const NEXT_WITHDRAWAL_ID_SELECTOR = "0x4a9122e3"

function fail(message) { throw new Error(message) }
function quantity(value, context) {
  if (typeof value !== "string" || !/^0x[0-9a-fA-F]+$/.test(value)) fail(`${context} is not a hex quantity`)
  return BigInt(value)
}
function word(value, context) {
  if (typeof value !== "string" || !/^0x[0-9a-fA-F]{64}$/.test(value)) fail(`${context} is not one ABI word`)
  return value.toLowerCase()
}
function sha256(value) { return createHash("sha256").update(value).digest("hex") }
function delay(milliseconds) { return new Promise((resolve) => setTimeout(resolve, milliseconds)) }

async function rpc(url, method, params, fetchImpl) {
  for (let attempt = 0; attempt < 4; attempt += 1) {
    const response = await fetchImpl(url, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ jsonrpc: "2.0", id: 1, method, params }),
    })
    if (response.ok) {
      const payload = await response.json()
      if (payload?.error || payload?.result === undefined || payload?.result === null) fail(`${method} returned no usable result`)
      return payload.result
    }
    if (response.status !== 429 || attempt === 3) fail(`${method} returned HTTP ${response.status}`)
    await delay(1_000 * (attempt + 1))
  }
  fail(`${method} exhausted its retry budget`)
}

function validateUrl(value, index) {
  const parsed = new URL(value)
  if (parsed.protocol !== "https:" || parsed.username || parsed.password || parsed.search || parsed.hash) {
    fail(`RPC provider ${index + 1} must be a credential-free HTTPS URL`)
  }
  return parsed.hostname.toLowerCase()
}

export async function collectWithdrawalBoundary(profile, config, fetchImpl = fetch) {
  const expectedUrls = [profile?.baseRpcUrl, ...(profile?.baseHistoryRpcUrls ?? [])]
  if (expectedUrls.length !== 3 || expectedUrls.some((url) => typeof url !== "string")) {
    fail("profile requires one Base RPC URL and two Base history RPC URLs")
  }
  if (config?.schema_version !== 1 || !Array.isArray(config.rpc_urls) || config.rpc_urls.length !== 3
    || new Set(config.rpc_urls).size !== 3) fail("capture config requires exactly three distinct RPC URLs")
  if (!config.rpc_urls.every((url, index) => url === expectedUrls[index])) {
    fail("capture RPC URLs must exactly match the ordered deployment profile RPC URLs")
  }
  const providerHosts = config.rpc_urls.map(validateUrl)
  if (new Set(providerHosts).size !== providerHosts.length) {
    fail("capture RPC URLs must use three distinct provider hosts")
  }
  const bridge = String(profile?.bridgeAddress ?? "").toLowerCase()
  if (!/^0x[0-9a-f]{40}$/.test(bridge)) fail("profile bridgeAddress is invalid")
  const expectedChain = BigInt(profile?.chainId)
  const heads = []
  for (const url of config.rpc_urls) {
    const chainId = quantity(await rpc(url, "eth_chainId", [], fetchImpl), "eth_chainId")
    if (chainId !== expectedChain) fail("RPC provider returned the wrong chain ID")
    const block = await rpc(url, "eth_getBlockByNumber", ["finalized", false], fetchImpl)
    heads.push({ number: quantity(block.number, "finalized block number"), hash: String(block.hash).toLowerCase() })
  }
  const sorted = heads.map((head) => head.number).sort((a, b) => a < b ? -1 : a > b ? 1 : 0)
  const checkpoint = sorted[1]
  const observations = []
  for (const [index, url] of config.rpc_urls.entries()) {
    if (heads[index].number < checkpoint) continue
    const block = await rpc(url, "eth_getBlockByNumber", [`0x${checkpoint.toString(16)}`, false], fetchImpl)
    if (quantity(block.number, "checkpoint block number") !== checkpoint) fail("provider returned a different checkpoint block number")
    const blockHash = String(block.hash).toLowerCase()
    if (!/^0x[0-9a-f]{64}$/.test(blockHash)) fail("checkpoint block hash is invalid")
    const selector = { blockHash, requireCanonical: true }
    const pausedWord = await rpc(
      url,
      "eth_call",
      [{ to: bridge, data: WITHDRAWALS_PAUSED_SELECTOR }, selector],
      fetchImpl,
    )
    const nextWord = await rpc(
      url,
      "eth_call",
      [{ to: bridge, data: NEXT_WITHDRAWAL_ID_SELECTOR }, selector],
      fetchImpl,
    )
    const paused = quantity(word(pausedWord, "withdrawalsPaused result"), "withdrawalsPaused result")
    const minimumWithdrawalId = word(nextWord, "nextWithdrawalId result")
    if (paused !== 1n || BigInt(minimumWithdrawalId) === 0n) fail("Bridge must be withdrawal-paused with a nonzero nextWithdrawalId")
    observations.push({
      provider_url_sha256: sha256(url),
      finalized_head_block_number: Number(heads[index].number),
      checkpoint_block_number: Number(checkpoint),
      checkpoint_block_hash: blockHash,
      withdrawals_paused: true,
      minimum_withdrawal_id: minimumWithdrawalId,
    })
  }
  const groups = new Map()
  for (const observation of observations) {
    const key = `${observation.checkpoint_block_hash}:${observation.minimum_withdrawal_id}`
    groups.set(key, [...(groups.get(key) ?? []), observation])
  }
  const quorum = [...groups.values()].find((items) => items.length >= 2)
  if (!quorum) fail("fewer than two providers agreed on the canonical withdrawal boundary")
  return {
    schema_version: 1,
    kind: "withdrawal-admission-boundary",
    observed_at: new Date().toISOString(),
    chain_id: Number(expectedChain),
    bridge_address: bridge,
    finalized_checkpoint_block_number: quorum[0].checkpoint_block_number,
    finalized_checkpoint_block_hash: quorum[0].checkpoint_block_hash,
    withdrawals_paused: true,
    minimum_withdrawal_id: quorum[0].minimum_withdrawal_id,
    providers: observations,
  }
}

async function main() {
  const [, , profilePath, configPath] = process.argv
  if (!profilePath || !configPath) fail("usage: capture-withdrawal-boundary.mjs <frontend-profile.json> <capture-config.json>")
  const [profile, config] = await Promise.all([
    readFile(path.resolve(profilePath), "utf8").then(JSON.parse),
    readFile(path.resolve(configPath), "utf8").then(JSON.parse),
  ])
  process.stdout.write(`${JSON.stringify(await collectWithdrawalBoundary(profile, config))}\n`)
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) await main()
