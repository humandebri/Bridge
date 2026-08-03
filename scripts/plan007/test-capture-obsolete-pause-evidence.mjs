import assert from "node:assert/strict"
import { collectEvidence, observeIcPause } from "./capture-obsolete-pause-evidence.mjs"

const hash = (character) => `0x${character.repeat(64)}`
const address = `0x${"4".repeat(40)}`
const depositTransaction = hash("1")
const withdrawalTransaction = hash("2")
const depositTopic = "0x7a8cbbf7de1b70cf6a63059c06484e4a6ca4b28f18ced89f03ea751608fc29a1"
const withdrawalTopic = "0x7c82b8b6bc44325506945ff406eeb0f2add5b91cfdd2265e80994967d30a787d"
const config = {
  schema_version: 1,
  rpc_urls: ["https://one.example", "https://two.example", "https://three.example"],
  pause_deposit_mints_transaction_hash: depositTransaction,
  pause_withdrawals_transaction_hash: withdrawalTransaction,
  ic_environment: "sepolia-staging",
  audit_cursor: 7,
}

function rpcResult(method, params) {
  if (method === "eth_chainId") return "0x14a34"
  if (method === "eth_getBlockByNumber" && params[0] === "finalized") {
    return { number: "0x65", hash: hash("3") }
  }
  if (method === "eth_getBlockByNumber") return { hash: hash("4") }
  const transactionHash = params[0]
  const deposit = transactionHash === depositTransaction
  if (method === "eth_getTransactionByHash") {
    return { hash: transactionHash, to: address, input: deposit ? "0x15415f22" : "0x56bb54a7" }
  }
  if (method === "eth_getTransactionReceipt") {
    return {
      blockNumber: "0x64",
      blockHash: hash("4"),
      transactionHash,
      status: "0x1",
      logs: [{ address, topics: [deposit ? depositTopic : withdrawalTopic] }],
    }
  }
  throw new Error(`unexpected RPC method ${method}`)
}

const fetchImpl = async (_url, request) => {
  const { method, params } = JSON.parse(request.body)
  return { ok: true, json: async () => ({ jsonrpc: "2.0", id: 1, result: rpcResult(method, params) }) }
}
const runIcpImpl = (_canister, _environment, method) => {
  if (method === "pause_new_deposits") return "variant { Ok }"
  if (method === "get_bridge_status") {
    return "record { deposits_paused = true : bool; last_audit_sequence = opt (7 : nat64) }"
  }
  return 'record { kind = variant { DepositsPaused }; caller = principal "aaaaa-aa"; sequence = 7 : nat64 }'
}

const evidence = await collectEvidence(
  {
    bridgeAddress: address,
    bridgeCanisterId: "rlhjx-iyaaa-aaaaf-qcnyq-cai",
    chainId: 84532,
  },
  config,
  { schema_version: 30, deployment_instance_id: hash("5") },
  { module_hash: hash("6") },
  { fetchImpl, runIcpImpl, now: () => new Date("2026-08-03T00:00:00Z") },
)
assert.equal(evidence.providers.length, 3)
assert.ok(evidence.providers.every((provider) => provider.actions.every((action) => (
  action.receipt_status === 1 && action.event_observed && action.canonical_probe_succeeded
))))
assert.deepEqual(evidence.ic_pause, {
  method: "pause_new_deposits",
  response: "Ok",
  audit_sequence: 7,
  audit_kind: "DepositsPaused",
  audit_caller: "aaaaa-aa",
  status_deposits_paused: true,
  status_last_audit_sequence: 7,
})

let icCalled = false
await assert.rejects(
  collectEvidence(
    { bridgeAddress: address, bridgeCanisterId: "rlhjx-iyaaa-aaaaf-qcnyq-cai", chainId: 84532 },
    config,
    { schema_version: 30, deployment_instance_id: hash("5") },
    { module_hash: hash("6") },
    {
      fetchImpl: async (url, request) => {
        const response = await fetchImpl(url, request)
        const payload = await response.json()
        if (JSON.parse(request.body).method === "eth_getTransactionReceipt") payload.result.status = "0x0"
        return { ok: true, json: async () => payload }
      },
      runIcpImpl: () => { icCalled = true; return "" },
    },
  ),
  /not a canonical successful pause/,
)
assert.equal(icCalled, false)

assert.throws(
  () => observeIcPause("aaaaa-aa", "sepolia-staging", 7, (_canister, _environment, method) => (
    method === "pause_new_deposits"
      ? "variant { Ok }"
      : "record { deposits_paused = false : bool }"
  )),
  /did not confirm deposits_paused/,
)

for (const status of [
  "record { deposits_paused = true : bool; last_audit_sequence = null }",
  "record { deposits_paused = true : bool; last_audit_sequence = opt (7 : nat32) }",
  "record { deposits_paused = true : bool; last_audit_sequence = opt 7 : nat64 }",
  "record { deposits_paused = true : bool; last_audit_sequence = opt (7 : nat64); last_audit_sequence = opt (8 : nat64) }",
]) {
  assert.throws(
    () => observeIcPause("aaaaa-aa", "sepolia-staging", 7, (_canister, _environment, method) => (
      method === "pause_new_deposits"
        ? "variant { Ok }"
        : method === "get_bridge_status"
          ? status
          : 'record { kind = variant { DepositsPaused }; caller = principal "aaaaa-aa"; sequence = 7 : nat64 }'
    )),
    /did not expose exactly one nat64 last_audit_sequence/,
  )
}

assert.throws(
  () => observeIcPause("aaaaa-aa", "sepolia-staging", 7, (_canister, _environment, method) => (
    method === "pause_new_deposits"
      ? "variant { Ok }"
      : method === "get_bridge_status"
        ? "record { deposits_paused = true : bool; last_audit_sequence = opt (7 : nat64) }"
        : 'record { kind = variant { DepositsPaused }; caller = principal "aaaaa-aa"; sequence = 8 : nat64 }'
  )),
  /did not contain the pause event referenced by BridgeStatus/,
)

process.stdout.write("obsolete pause evidence capture tests passed\n")
