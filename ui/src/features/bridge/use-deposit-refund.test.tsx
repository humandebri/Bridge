import { clearTransferFacts } from "@/lib/transfer-state"
import { act, cleanup, renderHook, waitFor } from "@testing-library/react"
import { afterEach, beforeEach, expect, it, vi } from "vitest"
import type { DepositView } from "@/generated/bridge.did"
const mocks = vi.hoisted(() => ({
  prepare: vi.fn(),
  request: vi.fn(),
  runtime: vi.fn(),
  close: vi.fn(),
  update: vi.fn(),
  invalidate: vi.fn(),
  error: vi.fn(),
}))
vi.mock("wagmi", () => ({ useChainId: () => 8453 }))
vi.mock("@tanstack/react-query", () => ({
  useQueryClient: () => ({ invalidateQueries: mocks.invalidate }),
}))
vi.mock("@/features/wallet/ic-wallet-provider", () => ({
  useIcWallet: () => ({
    account: { owner: "aaaaa-aa" },
    adapter: { prepare: mocks.prepare, requestDepositRefund: mocks.request },
  }),
}))
vi.mock("@/features/status/use-status", () => ({
  useRuntimeValidation: () => ({ refetch: mocks.runtime }),
  useRuntimeHeartbeat: () => ({ refetch: mocks.runtime }),
}))
vi.mock("@/lib/runtime-validation", () => ({
  refetchRuntimeAttestedWriteReady: (...args: unknown[]) => mocks.runtime(...args),
}))
vi.mock("./bridge-progress-provider", () => ({
  useBridgeProgress: () => ({
    progress: { id: "one", deposit: { depositId: `0x${"11".repeat(32)}` } },
    update: mocks.update,
  }),
}))
vi.mock("sonner", () => ({ toast: { error: mocks.error } }))
import { useDepositRefund } from "./use-deposit-refund"
const record = {
  deposit_id: Array(32).fill(0x11),
  state: { AuthorizationAvailable: null },
} as unknown as DepositView
beforeEach(() => {
  clearTransferFacts()
  vi.clearAllMocks()
  mocks.prepare.mockResolvedValue(mocks.close)
  mocks.close.mockResolvedValue(undefined)
  mocks.runtime.mockResolvedValue({ ready: true })
  mocks.invalidate.mockResolvedValue(undefined)
  Object.defineProperty(navigator, "locks", {
    configurable: true,
    value: { request: vi.fn(async (_key, _options, callback) => callback({ name: "test" })) },
  })
})
afterEach(cleanup)
it("refund_action_coalesces_and_applies_terminal_result", async function refund_action_coalesces_and_applies_terminal_result() {
  let finish!: (value: DepositView) => void
  mocks.request.mockImplementation(
    () =>
      new Promise<DepositView>((resolve) => {
        finish = resolve
      }),
  )
  const a = renderHook(useDepositRefund)
  const b = renderHook(useDepositRefund)
  let first!: Promise<DepositView>, second!: Promise<DepositView>
  act(() => {
    first = a.result.current.request(record)
    second = b.result.current.request(record)
  })
  await waitFor(() => expect(mocks.request).toHaveBeenCalledTimes(1))
  expect(mocks.runtime).toHaveBeenCalledOnce()
  await act(async () => {
    finish({ ...record, state: { Refunded: null } })
    await Promise.all([first, second])
  })
  expect(mocks.update).toHaveBeenLastCalledWith("one", {
    phase: "complete",
    outcome: "refunded",
    observationError: undefined,
  })
  expect(mocks.close).toHaveBeenCalledOnce()
  await expect(a.result.current.request(record)).rejects.toThrow("Transfer already resolved")
  expect(mocks.request).toHaveBeenCalledOnce()
  expect(a.result.current.pending).toBe(false)
})
it("refund_action_respects_lock_and_runtime_failure", async function refund_action_respects_lock_and_runtime_failure() {
  const view = renderHook(useDepositRefund)
  vi.spyOn(navigator.locks, "request").mockImplementation(
    async (_key: string, ...args: unknown[]) =>
      (args.at(-1) as (lock: null) => Promise<unknown>)(null),
  )
  await act(async () => {
    await expect(view.result.current.request(record)).rejects.toThrow("Another wallet operation")
  })
  expect(mocks.prepare).not.toHaveBeenCalled()
  expect(mocks.request).not.toHaveBeenCalled()
  Object.defineProperty(navigator, "locks", {
    configurable: true,
    value: { request: vi.fn(async (_key, _options, callback) => callback({})) },
  })
  mocks.runtime.mockRejectedValueOnce(new Error("Runtime unavailable"))
  await act(async () => {
    await expect(view.result.current.request(record)).rejects.toThrow("Runtime unavailable")
  })
  expect(mocks.request).not.toHaveBeenCalled()
  expect(mocks.close).toHaveBeenCalledOnce()
})
