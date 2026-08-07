import { type ReactNode } from "react"
import { cleanup, render, waitFor } from "@testing-library/react"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"
import { Route } from "./status"

const mocks = vi.hoisted(() => {
  const validationData: {
    value: {
      ready: boolean
      blockers: string[]
      checkedAt: number
      status?: { source: string }
    } | undefined
  } = { value: { ready: true, blockers: [], checkedAt: 1 } }
  return {
    chainId: { value: 84_532 },
    validationData,
    validationRefetch: vi.fn(),
    heartbeatRefetch: vi.fn(),
    canisterRefetch: vi.fn(),
  }
})

vi.mock("@tanstack/react-router", () => ({
  createFileRoute: () => (options: { component: () => ReactNode }) => ({ options }),
}))

vi.mock("wagmi", () => ({ useChainId: () => mocks.chainId.value }))

vi.mock("@/features/status/use-status", () => ({
  useRuntimeValidation: () => ({
    data: mocks.validationData.value,
    dataUpdatedAt: 1,
    isFetching: false,
    refetch: mocks.validationRefetch,
  }),
  useRuntimeWriteReadiness: (value?: { ready: boolean; checkedAt: number }) => ({
    ready: value?.ready === true && Date.now() - value.checkedAt <= 60_000,
  }),
  useRuntimeHeartbeat: () => ({
    data: undefined,
    dataUpdatedAt: 1,
    isError: false,
    isFetching: false,
    refetch: mocks.heartbeatRefetch,
  }),
  useBridgeStatus: () => ({
    data: undefined,
    dataUpdatedAt: 1,
    isError: false,
    isFetching: false,
    refetch: mocks.canisterRefetch,
  }),
}))

const StatusPage = Route.options.component!

describe("StatusPage refresh", () => {
  afterEach(cleanup)

  beforeEach(() => {
    mocks.chainId.value = 84_532
    mocks.validationData.value = { ready: true, blockers: [], checkedAt: Date.now() }
    mocks.validationRefetch.mockReset().mockResolvedValue({ data: mocks.validationData.value })
    mocks.heartbeatRefetch.mockReset().mockResolvedValue({
      data: { ready: true, blockers: [], checkedAt: Date.now(), status: { source: "heartbeat" } },
    })
    mocks.canisterRefetch.mockReset().mockResolvedValue({ data: { source: "fallback" } })
  })

  it("does not duplicate Canister status when heartbeat returns it", async () => {
    render(<StatusPage />)

    await waitFor(() => expect(mocks.heartbeatRefetch).toHaveBeenCalledOnce())
    expect(mocks.canisterRefetch).not.toHaveBeenCalled()
    expect(mocks.validationRefetch).not.toHaveBeenCalled()
  })

  it("uses the Canister status fallback when heartbeat returns no status", async () => {
    mocks.heartbeatRefetch.mockResolvedValue({
      data: { ready: false, blockers: ["Runtime heartbeat failed"], checkedAt: Date.now() },
    })
    render(<StatusPage />)

    await waitFor(() => expect(mocks.canisterRefetch).toHaveBeenCalledOnce())
    expect(mocks.heartbeatRefetch).toHaveBeenCalledOnce()
  })

  it("retries full validation without heartbeat when validation has not succeeded", async () => {
    mocks.validationData.value = { ready: false, blockers: ["Runtime validation failed"], checkedAt: Date.now() }
    mocks.validationRefetch.mockResolvedValue({ data: mocks.validationData.value })
    render(<StatusPage />)

    await waitFor(() => expect(mocks.validationRefetch).toHaveBeenCalledOnce())
    expect(mocks.heartbeatRefetch).not.toHaveBeenCalled()
    expect(mocks.canisterRefetch).toHaveBeenCalledOnce()
  })

  it("retries full validation when the successful result has expired", async () => {
    mocks.validationData.value = { ready: true, blockers: [], checkedAt: Date.now() - 60_001 }
    mocks.validationRefetch.mockResolvedValue({
      data: { ready: true, blockers: [], checkedAt: Date.now(), status: { source: "refreshed-validation" } },
    })

    render(<StatusPage />)

    await waitFor(() => expect(mocks.validationRefetch).toHaveBeenCalledOnce())
    expect(mocks.heartbeatRefetch).not.toHaveBeenCalled()
    expect(mocks.canisterRefetch).not.toHaveBeenCalled()
  })

  it("does not follow a successful full validation with an immediate heartbeat", async () => {
    mocks.validationData.value = { ready: false, blockers: ["Runtime validation pending"], checkedAt: Date.now() }
    const completed = { ready: true, blockers: [], checkedAt: Date.now(), status: { source: "full-validation" } }
    mocks.validationRefetch.mockResolvedValue({ data: completed })
    const view = render(<StatusPage />)

    await waitFor(() => expect(mocks.validationRefetch).toHaveBeenCalledOnce())
    mocks.validationData.value = completed
    view.rerender(<StatusPage />)
    expect(mocks.heartbeatRefetch).not.toHaveBeenCalled()
    expect(mocks.canisterRefetch).not.toHaveBeenCalled()
  })

  it("starts full validation when the connected chain changes", async () => {
    const view = render(<StatusPage />)

    await waitFor(() => expect(mocks.heartbeatRefetch).toHaveBeenCalledOnce())

    mocks.chainId.value = 8_453
    mocks.validationData.value = undefined
    mocks.validationRefetch.mockResolvedValue({
      data: { ready: true, blockers: [], checkedAt: Date.now(), status: { source: "new-chain-validation" } },
    })
    view.rerender(<StatusPage />)

    await waitFor(() => expect(mocks.validationRefetch).toHaveBeenCalledOnce())
    expect(mocks.heartbeatRefetch).toHaveBeenCalledOnce()
    expect(mocks.canisterRefetch).not.toHaveBeenCalled()
  })
})
