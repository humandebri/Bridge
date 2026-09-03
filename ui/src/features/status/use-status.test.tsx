import { StrictMode, type ReactNode } from "react"
import {
  focusManager,
  onlineManager,
  QueryClient,
  QueryClientProvider,
} from "@tanstack/react-query"
import { act, cleanup, renderHook, waitFor } from "@testing-library/react"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"
import { deploymentProfile } from "@/config/profile"
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
    return (
      <StrictMode>
        <QueryClientProvider client={client}>{children}</QueryClientProvider>
      </StrictMode>
    )
  }
}

describe("automatic status queries", () => {
  afterEach(cleanup)

  beforeEach(() => {
    focusManager.setFocused(true)
    onlineManager.setOnline(true)
    mocks.validateRuntime
      .mockReset()
      .mockResolvedValue({ ready: true, blockers: [], checkedAt: Date.now() })
    mocks.validateRuntimeHeartbeat
      .mockReset()
      .mockResolvedValue({ ready: true, blockers: [], checkedAt: Date.now() })
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
    const view = renderHook(() => useRuntimeValidation(undefined, { enabled: true }), {
      wrapper: wrapper(),
    })

    await waitFor(() => expect(view.result.current.isSuccess).toBe(true))
    expect(mocks.validateRuntime).toHaveBeenCalledOnce()
  })

  it("does not treat validation without an observation as heartbeat data", async () => {
    const initialValidation = { ready: true, blockers: [], checkedAt: Date.now() }
    const view = renderHook(
      () => useRuntimeHeartbeat(undefined, initialValidation, { enabled: true }),
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
    const view = renderHook(
      () => {
        const full = useRuntimeValidation(undefined, { enabled: true })
        const heartbeat = useRuntimeHeartbeat(undefined, full.data, {
          enabled: full.data?.ready === true,
        })
        return { full, heartbeat }
      },
      { wrapper: wrapper() },
    )

    await waitFor(() => expect(view.result.current.heartbeat.data?.snapshot).toBeDefined())
    expect(mocks.validateRuntimeHeartbeat).not.toHaveBeenCalled()
    expect(view.result.current.heartbeat.data?.checkedAt).toBe(checkedAt)
  })

  it("publishes heartbeat Canister status to the shared status cache", async () => {
    const status = { marker: "heartbeat-status" }
    mocks.validateRuntimeHeartbeat.mockResolvedValue({
      ready: true,
      blockers: [],
      checkedAt: Date.now(),
      status,
    })
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    const TestWrapper = ({ children }: { children: ReactNode }) => (
      <QueryClientProvider client={client}>{children}</QueryClientProvider>
    )
    const view = renderHook(() => useRuntimeHeartbeat(undefined, undefined, { enabled: true }), {
      wrapper: TestWrapper,
    })

    await waitFor(() => expect(view.result.current.isSuccess).toBe(true))
    expect(client.getQueryData(["bridge-status", deploymentProfile.bridgeCanisterId])).toEqual(
      status,
    )
  })

  it("keeps the last successful heartbeat when a refresh raises a communication error", async () => {
    const validation = {
      ready: true,
      blockers: [],
      checkedAt: Date.now(),
      snapshot: {
        serviceFee: 50_000_000n,
        maxServiceFee: 100_000_000n,
        perDepositLimit: 10_000_000_000n,
        minted: 0n,
        limit: 10_000_000_000n,
        startedAt: 0n,
        duration: 60n,
        depositsPaused: false,
        withdrawalsPaused: false,
        bridgeSigner: `0x${"11".repeat(20)}` as const,
        mintAuthorizationEpoch: 1n,
        blockTimestamp: 1n,
      },
    }
    mocks.validateRuntimeHeartbeat
      .mockResolvedValueOnce(validation)
      .mockRejectedValue(new Error("Base RPC unavailable"))
    const view = renderHook(
      () => {
        const heartbeat = useRuntimeHeartbeat(undefined, undefined, { enabled: true })
        return {
          data: heartbeat.data,
          error: heartbeat.error,
          isError: heartbeat.isError,
          refetch: heartbeat.refetch,
        }
      },
      { wrapper: wrapper() },
    )

    await waitFor(() => expect(view.result.current.data).toEqual(validation))
    await act(async () => {
      await view.result.current.refetch()
    })

    await waitFor(() => expect(view.result.current.isError).toBe(true))
    expect(view.result.current.error).toEqual(new Error("Base RPC unavailable"))
    expect(view.result.current.data).toEqual(validation)
  })

  it("keeps confirmed safety blockers as successful heartbeat data", async () => {
    const blocker = { ready: false, blockers: ["Deposit minting is paused"], checkedAt: Date.now() }
    mocks.validateRuntimeHeartbeat.mockResolvedValue(blocker)
    const view = renderHook(() => useRuntimeHeartbeat(undefined, undefined, { enabled: true }), {
      wrapper: wrapper(),
    })

    await waitFor(() => expect(view.result.current.isSuccess).toBe(true))
    expect(view.result.current.isError).toBe(false)
    expect(view.result.current.data).toEqual(blocker)
  })

  it("does not poll and refreshes only after focus and reconnect events", async () => {
    focusManager.setFocused(false)
    const view = renderHook(() => useRuntimeHeartbeat(undefined, undefined, { enabled: true }), {
      wrapper: wrapper(),
    })

    await waitFor(() => expect(view.result.current.isSuccess).toBe(true))
    await act(async () => new Promise((resolve) => setTimeout(resolve, 60)))
    expect(mocks.validateRuntimeHeartbeat).toHaveBeenCalledOnce()

    const callsBeforeFocus = mocks.validateRuntimeHeartbeat.mock.calls.length
    focusManager.setFocused(true)
    await waitFor(() =>
      expect(mocks.validateRuntimeHeartbeat.mock.calls.length).toBeGreaterThan(callsBeforeFocus),
    )

    onlineManager.setOnline(false)
    await act(async () => new Promise((resolve) => setTimeout(resolve, 30)))
    const callsBeforeReconnect = mocks.validateRuntimeHeartbeat.mock.calls.length
    onlineManager.setOnline(true)
    await waitFor(() =>
      expect(mocks.validateRuntimeHeartbeat.mock.calls.length).toBeGreaterThan(
        callsBeforeReconnect,
      ),
    )
    view.unmount()
  })

  it("can disable focus and reconnect refreshes", async () => {
    focusManager.setFocused(false)
    const view = renderHook(
      () =>
        useRuntimeHeartbeat(undefined, undefined, {
          enabled: true,
          refetchOnWindowFocus: false,
          refetchOnReconnect: false,
        }),
      { wrapper: wrapper() },
    )

    await waitFor(() => expect(view.result.current.isSuccess).toBe(true))
    expect(mocks.validateRuntimeHeartbeat).toHaveBeenCalledOnce()

    focusManager.setFocused(true)
    onlineManager.setOnline(false)
    await act(async () => new Promise((resolve) => setTimeout(resolve, 30)))
    onlineManager.setOnline(true)
    await act(async () => new Promise((resolve) => setTimeout(resolve, 30)))

    expect(mocks.validateRuntimeHeartbeat).toHaveBeenCalledOnce()
    view.unmount()
  })

  it("does not validate automatically when disabled", async () => {
    renderHook(() => useRuntimeValidation(undefined), { wrapper: wrapper() })
    await act(async () => Promise.resolve())
    expect(mocks.validateRuntime).not.toHaveBeenCalled()
  })

  it("loads the Base quote when runtime readiness enables it", async () => {
    const view = renderHook(({ enabled }) => useCurrentBaseQuote({ enabled }), {
      wrapper: wrapper(),
      initialProps: { enabled: false },
    })
    expect(mocks.readContract).not.toHaveBeenCalled()

    view.rerender({ enabled: true })
    await waitFor(() => expect(view.result.current.isSuccess).toBe(true))
    expect(mocks.readContract).toHaveBeenCalledOnce()
  })

  it("honors the requested stale window for the Base quote", async () => {
    const view = renderHook(() => useCurrentBaseQuote({ enabled: true, staleTime: 60_000 }), {
      wrapper: wrapper(),
    })

    await waitFor(() => expect(view.result.current.data).toBeDefined())
    expect(view.result.current.isStale).toBe(false)
  })
})
