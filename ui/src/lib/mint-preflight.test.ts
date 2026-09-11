import { beforeEach, expect, it, vi } from "vitest"
import type { DepositView } from "@/generated/bridge.did"
import { prepareMint, checkMintDeadline } from "./mint-preflight"
const mocks = vi.hoisted(() => ({
  validate: vi.fn(),
  heartbeat: vi.fn(),
  authorize: vi.fn(),
  simulate: vi.fn(),
  create: vi.fn(),
  connected: vi.fn(),
}))
vi.mock("wagmi/actions", () => ({
  getAccount: () => ({ address: "0x11" }),
  getChainId: () => 8453,
}))
vi.mock("./evm/client", () => ({ wagmiConfig: {}, createBasePublicClient: mocks.create }))
vi.mock("./runtime-validation", () => ({
  runtimeWriteBlocker: (v: unknown) => !v,
  requireRuntimeWriteReady: (v: { ready: boolean }) => {
    if (!v.ready) throw new Error("not ready")
  },
  validateRuntime: mocks.validate,
  validateRuntimeHeartbeat: mocks.heartbeat,
}))
vi.mock("./mint-authorization", () => ({ validateMintAuthorization: mocks.authorize }))
beforeEach(() => {
  vi.clearAllMocks()
  mocks.validate.mockResolvedValue({ ready: true, profileFingerprint: "one" })
  mocks.heartbeat.mockResolvedValue({ ready: true, profileFingerprint: "one" })
  mocks.authorize.mockResolvedValue({
    latestBlockTimestamp: 1000n,
    authorization: { deadline: 1100n },
    signature: "0x11",
  })
  mocks.simulate.mockResolvedValue({})
  mocks.create.mockReturnValue({ simulateContract: mocks.simulate })
})
it("passes_the_trial_abort_signal_to_isolated_ic_and_base_reads", async () => {
  const signal = new AbortController().signal
  const context = { signal, check: vi.fn(), stage: vi.fn() }
  await prepareMint({} as DepositView, "0x11", 8453, undefined, context)
  expect(mocks.validate.mock.calls[0]?.[2]).toBe(signal)
  expect(mocks.heartbeat.mock.calls[0]?.[2]).toBe(signal)
  expect(mocks.create.mock.calls[0]?.[1]).toBe(signal)
  expect(context.stage.mock.calls.map(([v]) => v)).toEqual([
    "checking-ic",
    "checking-base",
    "simulating",
  ])
})
it("rejects_a_different_runtime_profile_before_simulation", async () => {
  mocks.heartbeat.mockResolvedValue({ ready: true, profileFingerprint: "two" })
  await expect(
    prepareMint({} as DepositView, "0x11", 8453, undefined, {
      signal: new AbortController().signal,
      check: () => {},
      stage: () => {},
    }),
  ).rejects.toThrow("profile changed")
  expect(mocks.simulate).not.toHaveBeenCalled()
})
it("checks_elapsed_time_before_wallet_dispatch", async () => {
  const prepared = await prepareMint({} as DepositView, "0x11", 8453, undefined, {
    signal: new AbortController().signal,
    check: () => {},
    stage: () => {},
  })
  expect(() => checkMintDeadline(prepared)).not.toThrow()
  prepared.observedAt = performance.now() - 100000
  expect(() => checkMintDeadline(prepared)).toThrow("expired during preflight")
})
