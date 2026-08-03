import { StrictMode, type ReactNode } from "react"
import { focusManager, onlineManager, QueryClient, QueryClientProvider } from "@tanstack/react-query"
import { act, cleanup, renderHook, waitFor } from "@testing-library/react"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"
import type * as RuntimeValidationModule from "@/lib/runtime-validation"
import { useCurrentBaseQuote, useRuntimeHeartbeat, useRuntimeValidation } from "./use-status"

const mocks = vi.hoisted(() => ({
  validateRuntime: vi.fn(),
  validateRuntimeHeartbeat: vi.fn(),
  readContract: vi.fn(),
}))

vi.mock("@/lib/runtime-validation", async (importOriginal) => {
  const actual = await importOriginal<typeof RuntimeValidationModule>()
  return {
    ...actual,
    validateRuntime: mocks.validateRuntime,
    validateRuntimeHeartbeat: mocks.validateRuntimeHeartbeat,
  }
})

vi.mock("@/lib/evm/client", () => ({
  basePublicClient: { readContract: mocks.readContract },
}))

function wrapper() {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false, staleTime: 0 } } })
  return function Wrapper({ children }: { children: ReactNode }) {
    return <StrictMode><QueryClientProvider client={client}>{children}</QueryClientProvider></StrictMode>
  }
}

describe("automatic status queries", () => {
  afterEach(cleanup)

  beforeEach(() => {
    focusManager.setFocused(true)
    onlineManager.setOnline(true)
    mocks.validateRuntime.mockReset().mockResolvedValue({ ready: true, blockers: [], checkedAt: Date.now() })
    mocks.validateRuntimeHeartbeat.mockReset().mockResolvedValue({ ready: true, blockers: [], checkedAt: Date.now() })
    mocks.readContract.mockReset().mockResolvedValue({
      serviceFee: 50_000_000n,
      maxServiceFee: 50_000_000n,
      perDepositLimit: 15_000_000_000_000n,
      mintedInWindow: 0n,
      mintWindowLimit: 15_000_000_000_000n,
      mintWindowStartedAt: 0n,
      mintWindowDuration: 86_400n,
      depositMintsPaused: false,
      withdrawalsPaused: false,
      bridgeSigner: "0x0000000000000000000000000000000000000001",
      blockTimestamp: 0n,
    })
  })

  it("starts validation on mount and deduplicates Strict Mode", async () => {
    const view = renderHook(
      () => useRuntimeValidation(undefined, { enabled: true }),
      { wrapper: wrapper() },
    )

    await waitFor(() => expect(view.result.current.isSuccess).toBe(true))
    expect(mocks.validateRuntime).toHaveBeenCalledOnce()
  })

  it("retries one not-ready initial validation and stops after success", async () => {
    mocks.validateRuntime
      .mockResolvedValueOnce({ ready: false, blockers: ["Base RPC unavailable"], checkedAt: Date.now() })
      .mockResolvedValueOnce({ ready: true, blockers: [], checkedAt: Date.now() })
    const view = renderHook(
      () => useRuntimeValidation(undefined, { enabled: true, retryNotReadyAfterMs: 100 }),
      { wrapper: wrapper() },
    )

    await waitFor(() => expect(view.result.current.isAutoRetryPending).toBe(true))
    await waitFor(() => expect(mocks.validateRuntime).toHaveBeenCalledTimes(2))
    await waitFor(() => expect(view.result.current.data?.ready).toBe(true))
    expect(view.result.current.isAutoRetryPending).toBe(false)
  })

  it("retries a persistent not-ready result only once", async () => {
    mocks.validateRuntime.mockResolvedValue({ ready: false, blockers: ["Bridge signer differs"], checkedAt: Date.now() })
    const view = renderHook(
      () => useRuntimeValidation(undefined, { enabled: true, retryNotReadyAfterMs: 10 }),
      { wrapper: wrapper() },
    )

    await waitFor(() => expect(mocks.validateRuntime).toHaveBeenCalledTimes(2))
    await act(async () => new Promise((resolve) => setTimeout(resolve, 30)))
    expect(mocks.validateRuntime).toHaveBeenCalledTimes(2)
    expect(view.result.current.data?.ready).toBe(false)
    expect(view.result.current.isAutoRetryPending).toBe(false)
  })

  it("allows one new not-ready retry after the chain changes", async () => {
    mocks.validateRuntime.mockResolvedValue({ ready: false, blockers: ["RPC unavailable"], checkedAt: Date.now() })
    const view = renderHook(
      ({ chainId }) => useRuntimeValidation(chainId, { enabled: true, retryNotReadyAfterMs: 10 }),
      { wrapper: wrapper(), initialProps: { chainId: 1 } },
    )

    await waitFor(() => expect(mocks.validateRuntime).toHaveBeenCalledTimes(2))
    view.rerender({ chainId: 2 })
    await waitFor(() => expect(mocks.validateRuntime).toHaveBeenCalledTimes(4))
    await act(async () => new Promise((resolve) => setTimeout(resolve, 30)))
    expect(mocks.validateRuntime).toHaveBeenCalledTimes(4)
  })

  it("does not treat validation without an observation as heartbeat data", async () => {
    const initialValidation = { ready: true, blockers: [], checkedAt: Date.now() }
    const view = renderHook(
      () => useRuntimeHeartbeat(undefined, initialValidation, { enabled: true, refetchInterval: 20 }),
      { wrapper: wrapper() },
    )

    await waitFor(() => expect(mocks.validateRuntimeHeartbeat).toHaveBeenCalled())
    await waitFor(() => expect(view.result.current.data).toBeDefined())
    expect(mocks.validateRuntime).not.toHaveBeenCalled()
    view.unmount()
  })

  it("seeds a completed validation into heartbeat without an immediate duplicate read", async () => {
    const checkedAt = Date.now()
    const validation = {
      ready: true,
      blockers: [],
      checkedAt,
      profileFingerprint: "profile",
      finalizedBlock: 12n,
      finalizedBlockHash: `0x${"44".repeat(32)}` as const,
      snapshot: {
        serviceFee: 1n,
        maxServiceFee: 1n,
        perDepositLimit: 10n,
        minted: 0n,
        limit: 10n,
        startedAt: 0n,
        duration: 60n,
        depositsPaused: false,
        withdrawalsPaused: false,
        bridgeSigner: `0x${"11".repeat(20)}` as const,
        mintAuthorizationEpoch: 1n,
        blockTimestamp: 1n,
      },
    }
    mocks.validateRuntime.mockResolvedValue(validation)
    const view = renderHook(() => {
      const full = useRuntimeValidation(undefined, { enabled: true })
      const heartbeat = useRuntimeHeartbeat(undefined, full.data, { enabled: full.data?.ready === true, refetchInterval: 45_000 })
      return { full, heartbeat }
    }, { wrapper: wrapper() })

    await waitFor(() => expect(view.result.current.heartbeat.data?.snapshot).toBeDefined())
    expect(mocks.validateRuntimeHeartbeat).not.toHaveBeenCalled()
    expect(view.result.current.heartbeat.data?.checkedAt).toBe(checkedAt)
  })

  it("pauses while hidden and refreshes on focus and reconnect", async () => {
    focusManager.setFocused(false)
    const initialValidation = { ready: true, blockers: [], checkedAt: Date.now() - 1_000 }
    const view = renderHook(
      () => useRuntimeHeartbeat(undefined, initialValidation, { enabled: true, refetchInterval: 20 }),
      { wrapper: wrapper() },
    )

    await act(async () => new Promise((resolve) => setTimeout(resolve, 30)))
    expect(mocks.validateRuntimeHeartbeat).toHaveBeenCalledOnce()

    const callsBeforeFocus = mocks.validateRuntimeHeartbeat.mock.calls.length
    focusManager.setFocused(true)
    await waitFor(() => expect(mocks.validateRuntimeHeartbeat.mock.calls.length).toBeGreaterThan(callsBeforeFocus))

    onlineManager.setOnline(false)
    await act(async () => new Promise((resolve) => setTimeout(resolve, 30)))
    const callsBeforeReconnect = mocks.validateRuntimeHeartbeat.mock.calls.length
    onlineManager.setOnline(true)
    await waitFor(() => expect(mocks.validateRuntimeHeartbeat.mock.calls.length).toBeGreaterThan(callsBeforeReconnect))
    view.unmount()
  })

  it("does not validate automatically when disabled", async () => {
    const view = renderHook(
      () => useRuntimeValidation(undefined, { retryNotReadyAfterMs: 10 }),
      { wrapper: wrapper() },
    )
    await act(async () => Promise.resolve())
    expect(mocks.validateRuntime).not.toHaveBeenCalled()
    expect(view.result.current.isAutoRetryPending).toBe(false)
  })

  it("loads the Base quote when runtime readiness enables it", async () => {
    const view = renderHook(
      ({ enabled }) => useCurrentBaseQuote({ enabled }),
      { wrapper: wrapper(), initialProps: { enabled: false } },
    )
    expect(mocks.readContract).not.toHaveBeenCalled()

    view.rerender({ enabled: true })
    await waitFor(() => expect(view.result.current.isSuccess).toBe(true))
    expect(mocks.readContract).toHaveBeenCalledOnce()
  })

  it("honors the requested stale window for the Base quote", async () => {
    const view = renderHook(
      () => useCurrentBaseQuote({ enabled: true, staleTime: 60_000 }),
      { wrapper: wrapper() },
    )

    await waitFor(() => expect(view.result.current.data).toBeDefined())
    expect(view.result.current.isStale).toBe(false)
  })
})
