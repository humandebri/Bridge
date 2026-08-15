import assert from "node:assert/strict"
import { collectWithdrawalBoundary } from "./capture-withdrawal-boundary.mjs"

const urls = ["https://one.example", "https://two.example", "https://three.example"]
const blockHash = `0x${"22".repeat(32)}`
const word = (value) => `0x${value.toString(16).padStart(64, "0")}`
const profile = {
  chainId: 84532,
  bridgeAddress: `0x${"44".repeat(20)}`,
  baseRpcUrl: urls[0],
  baseHistoryRpcUrls: urls.slice(1),
}

async function fetchImpl(url, request) {
  const { method, params } = JSON.parse(request.body)
  let result
  if (method === "eth_chainId") result = "0x14a34"
  else if (method === "eth_getBlockByNumber" && params[0] === "finalized") {
    result = { number: ["0x5a", "0x64", "0x6e"][urls.indexOf(url)], hash: blockHash }
  } else if (method === "eth_getBlockByNumber") result = { number: params[0], hash: blockHash }
  else if (method === "eth_call" && params[0].data === "0xe9f2838e") result = word(1)
  else if (method === "eth_call" && params[0].data === "0x4a9122e3") result = word(3)
  else throw new Error(`unexpected ${method}`)
  return { ok: true, json: async () => ({ jsonrpc: "2.0", id: 1, result }) }
}

const evidence = await collectWithdrawalBoundary(
  profile,
  { schema_version: 1, rpc_urls: urls },
  fetchImpl,
)
assert.equal(evidence.finalized_checkpoint_block_number, 100)
assert.equal(evidence.finalized_checkpoint_block_hash, blockHash)
assert.equal(evidence.minimum_withdrawal_id, word(3))
assert.equal(evidence.providers.length, 2)
assert.deepEqual(
  evidence.providers.map(({ finalized_head_block_number: head }) => head),
  [100, 110],
)

await assert.rejects(
  collectWithdrawalBoundary(
    profile,
    { schema_version: 1, rpc_urls: urls },
    async (url, request) => {
      const response = await fetchImpl(url, request)
      const body = JSON.parse(request.body)
      if (body.method !== "eth_getBlockByNumber" || body.params[0] === "finalized") return response
      return { ok: true, json: async () => ({ jsonrpc: "2.0", id: 1, result: { number: "0x65", hash: blockHash } }) }
    },
  ),
  /different checkpoint block number/,
)

for (const [name, rpcUrls, expected] of [
  ["arbitrary provider", [urls[0], urls[1], "https://other.example"], /ordered deployment profile/],
  ["reordered providers", [urls[1], urls[0], urls[2]], /ordered deployment profile/],
  ["same provider host", ["https://one.example", "https://one.example/", urls[2]], /distinct provider hosts/],
]) {
  const matchingProfile = name === "same provider host"
    ? { ...profile, baseHistoryRpcUrls: rpcUrls.slice(1) }
    : profile
  await assert.rejects(
    collectWithdrawalBoundary(
      matchingProfile,
      { schema_version: 1, rpc_urls: rpcUrls },
      fetchImpl,
    ),
    expected,
  )
}

await assert.rejects(
  collectWithdrawalBoundary(
    profile,
    { schema_version: 1, rpc_urls: urls },
    async (url, request) => {
      const response = await fetchImpl(url, request)
      const body = JSON.parse(request.body)
      if (body.method !== "eth_chainId") return response
      return { ok: true, json: async () => ({ jsonrpc: "2.0", id: 1, result: "0x1" }) }
    },
  ),
  /wrong chain ID/,
)

process.stdout.write("withdrawal admission boundary capture tests passed\n")
