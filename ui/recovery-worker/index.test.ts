// @vitest-environment node
import { beforeEach, afterEach, expect, it, vi } from "vitest"
import worker from "./index"
const mocks = vi.hoisted(() => ({ deposit: vi.fn(), runtime: vi.fn() }))
vi.mock("@icp-sdk/core/agent", () => ({
  HttpAgent: { createSync: () => ({}) },
  Actor: {
    createActor: () => ({ get_deposit: mocks.deposit, get_runtime_binding: mocks.runtime }),
  },
}))
const id = `0x${"11".repeat(32)}`
const instance = `0x${"33".repeat(32)}`
const hash = `0x${"44".repeat(32)}`
const bridge = `0x${"55".repeat(20)}`
const token = `0x${"66".repeat(20)}`
const profile = {
  chainId: 8453,
  testOnly: false,
  icHost: "https://icp-api.io",
  canisterSchemaVersion: 36,
  bridgeCanisterId: "aaaaa-aa",
  bridgeAddress: bridge,
  bsnsAddress: token,
  deploymentInstanceId: instance,
  deploymentBlock: "10",
}
const env: Env = {
  RECOVERY_ENABLED: "true",
  ALCHEMY_API_KEY: "server-secret",
  CURSOR_KEY: "s".repeat(32),
  BRIDGE_PROFILE_JSON: JSON.stringify(profile),
  IP_LIMIT: { limit: async () => ({ success: true }) },
  DEPOSIT_LIMIT: { limit: async () => ({ success: true }) },
}
function request(body: unknown = { depositId: id }, origin = "https://bridge.kinic.xyz") {
  return new Request("https://recovery.bridge.kinic.xyz/v1/mint-recovery", {
    method: "POST",
    headers: {
      Origin: origin,
      "CF-Connecting-IP": "192.0.2.1",
      "Content-Type": "application/json",
    },
    body: JSON.stringify(body),
  })
}
const rpc = vi.fn()
beforeEach(() => {
  vi.clearAllMocks()
  mocks.deposit.mockResolvedValue([
    {
      deposit_id: new Uint8Array(32).fill(0x11),
      mint_authorization: [
        {
          signature: [new Uint8Array(65)],
          digest: new Uint8Array(32).fill(0x22),
          recipient: new Uint8Array(20).fill(0x77),
          finalized_block_number: 20n,
          chain_id: 8453n,
          verifying_contract: new Uint8Array(20).fill(0x55),
        },
      ],
    },
  ])
  mocks.runtime.mockResolvedValue({
    schema_version: 36,
    base_chain_id: 8453n,
    deployment_instance_id: new Uint8Array(32).fill(0x33),
    bridge_contract: new Uint8Array(20).fill(0x55),
  })
  rpc.mockImplementation(async (_url, options) => {
    const call = JSON.parse(options.body)
    return Response.json({
      result:
        call.method === "eth_blockNumber" ? "0x100" : { transfers: [{ hash }], pageKey: "next" },
    })
  })
  vi.stubGlobal("fetch", rpc)
})
afterEach(() => {
  vi.unstubAllGlobals()
  vi.useRealTimers()
})
it("recovery_worker_binds_search_and_signed_pagination", async () => {
  const first = await worker.fetch(request(), env)
  expect(first.status).toBe(200)
  const page = (await first.json()) as { cursor: string; hashes: string[] }
  expect(page.hashes).toEqual([hash])
  const call = JSON.parse(rpc.mock.calls[1]![1].body)
  expect(call.params[0]).toMatchObject({
    fromBlock: "0x14",
    toBlock: "0x100",
    toAddress: `0x${"77".repeat(20)}`,
    contractAddresses: [token],
    category: ["erc20"],
    maxCount: "0x64",
  })
  const next = await worker.fetch(request({ depositId: id, cursor: page.cursor }), env)
  expect(next.status).toBe(200)
  expect(rpc).toHaveBeenCalledTimes(3)
  expect(JSON.parse(rpc.mock.calls[2]![1].body).params[0]).toMatchObject({
    pageKey: "next",
    toBlock: "0x100",
  })
  expect(JSON.stringify(page)).not.toContain(env.ALCHEMY_API_KEY)
})
it("recovery_worker_rejects_invalid_expired_and_foreign_cursors", async () => {
  const page = (await (await worker.fetch(request(), env)).json()) as { cursor: string }
  expect(
    (await worker.fetch(request({ depositId: id, cursor: `${page.cursor}x` }), env)).status,
  ).toBe(400)
  const other = { ...env, BRIDGE_PROFILE_JSON: JSON.stringify({ ...profile, bsnsAddress: bridge }) }
  expect((await worker.fetch(request({ depositId: id, cursor: page.cursor }), other)).status).toBe(
    400,
  )
  vi.useFakeTimers()
  vi.setSystemTime(Date.now() + 900001)
  expect((await worker.fetch(request({ depositId: id, cursor: page.cursor }), env)).status).toBe(
    410,
  )
})
it("recovery_worker_rejects_foreign_runtime_and_input_overrides", async () => {
  expect((await worker.fetch(request({ depositId: id, toAddress: bridge }), env)).status).toBe(400)
  mocks.runtime.mockResolvedValue({ schema_version: 35 })
  expect((await worker.fetch(request(), env)).status).toBe(409)
  expect(rpc).not.toHaveBeenCalled()
})
it("recovery_worker_limits_origin_body_and_request_rate", async () => {
  expect((await worker.fetch(request({}, "https://other.example"), env)).status).toBe(403)
  expect((await worker.fetch(request({ depositId: "x".repeat(5000) }), env)).status).toBe(413)
  expect(
    (
      await worker.fetch(request(), {
        ...env,
        IP_LIMIT: { limit: async () => ({ success: false }) },
      })
    ).status,
  ).toBe(429)
  expect((await worker.fetch(request(), { ...env, RECOVERY_ENABLED: "false" })).status).toBe(503)
  expect(rpc).not.toHaveBeenCalled()
})
it("recovery_worker_preserves_upstream_failures_and_timeout", async () => {
  rpc.mockResolvedValueOnce(new Response("rate limited", { status: 429 }))
  expect((await worker.fetch(request(), env)).status).toBe(429)
  rpc.mockResolvedValueOnce(Response.json({ error: { message: env.ALCHEMY_API_KEY } }))
  const invalid = await worker.fetch(request(), env)
  expect(invalid.status).toBe(502)
  expect(await invalid.text()).not.toContain(env.ALCHEMY_API_KEY)
  vi.useFakeTimers()
  mocks.deposit.mockReturnValue(new Promise(() => {}))
  const pending = worker.fetch(request(), env)
  await vi.advanceTimersByTimeAsync(15001)
  expect((await pending).status).toBe(504)
})
