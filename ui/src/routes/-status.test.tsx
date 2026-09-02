import { type ReactNode } from "react"
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"
import { Route } from "./status"

const mocks = vi.hoisted(() => ({
  heartbeat: {
    data: undefined as undefined | { ready: boolean; blockers: string[]; checkedAt: number; status?: { source: string } },
    dataUpdatedAt: 0,
    isError: false,
    error: undefined,
    isFetched: false,
    isFetching: false,
  },
  canister: {
    data: undefined,
    dataUpdatedAt: 0,
    isError: false,
    error: undefined,
    isFetching: false,
  },
  heartbeatRefetch: vi.fn(),
  canisterRefetch: vi.fn(),
}))

vi.mock("@tanstack/react-router", () => ({
  createFileRoute: () => (options: { component: () => ReactNode }) => ({ options }),
}))

vi.mock("wagmi", () => ({ useChainId: () => 84_532 }))

vi.mock("@/features/status/use-status", () => ({
  useRuntimeHeartbeat: () => ({ ...mocks.heartbeat, refetch: mocks.heartbeatRefetch }),
  useBridgeStatus: () => ({ ...mocks.canister, refetch: mocks.canisterRefetch }),
}))

const StatusPage = Route.options.component!

describe("StatusPage refresh", () => {
  afterEach(cleanup)

  beforeEach(() => {
    Object.assign(mocks.heartbeat, { data: undefined, dataUpdatedAt: 0, isError: false, error: undefined, isFetched: false, isFetching: false })
    Object.assign(mocks.canister, { data: undefined, dataUpdatedAt: 0, isError: false, error: undefined, isFetching: false })
    mocks.heartbeatRefetch.mockReset().mockResolvedValue({
      data: { ready: true, blockers: [], checkedAt: Date.now(), status: { source: "heartbeat" } },
      isError: false,
    })
    mocks.canisterRefetch.mockReset().mockResolvedValue({ data: { source: "fallback" } })
  })

  it("reports unknown rather than unavailable before live observations exist", () => {
    render(<StatusPage />)

    expect(screen.getAllByText("Unknown").length).toBeGreaterThanOrEqual(3)
    expect(screen.queryByText("Unavailable")).not.toBeInTheDocument()
    expect(screen.getByText("Live availability is unknown until current status checks succeed.")).toBeVisible()
    expect(mocks.canisterRefetch).not.toHaveBeenCalled()
  })

  it("reports unknown when fresh heartbeat observations fail signer verification", async () => {
    const now = Date.now()
    Object.assign(mocks.heartbeat, {
      data: {
        ready: false,
        blockers: ["Bridge signer differs from the reviewed profile"],
        checkedAt: now,
        finalizedBlock: 100n,
        finalizedBlockHash: "0x1234",
        snapshot: {
          blockTimestamp: 1n,
          serviceFee: 1n,
          perDepositLimit: 100n,
          minted: 0n,
          limit: 1_000n,
          depositsPaused: false,
          withdrawalsPaused: false,
        },
        status: { source: "heartbeat" },
      },
      dataUpdatedAt: now,
      isFetched: true,
    })
    Object.assign(mocks.canister, {
      data: {
        deposits_paused: false,
        counts: { deposits: 0n, withdrawals: 0n },
        unpaid_withdrawal_count: 0n,
        unpaid_withdrawal_amount_out: 0n,
        reserve: {
          cycles_balance: 100n,
          required_cycles: 10n,
        },
      },
      dataUpdatedAt: now,
    })

    render(<StatusPage />)
    await act(async () => {
      await new Promise((resolve) => window.setTimeout(resolve, 0))
    })

    expect(screen.getAllByText("Unknown").length).toBeGreaterThanOrEqual(3)
    expect(screen.queryByText("Available")).not.toBeInTheDocument()
    expect(screen.getByText("Current bridge status could not be confirmed. Please try again shortly.")).toBeVisible()
    expect(screen.queryByText("Bridge signer differs from the reviewed profile")).not.toBeInTheDocument()
  })

  it("loads Canister status automatically when the initial heartbeat fails", async () => {
    Object.assign(mocks.heartbeat, {
      isFetched: true,
      isError: true,
      error: new Error("Base RPC unavailable"),
    })

    render(<StatusPage />)

    await waitFor(() => expect(mocks.canisterRefetch).toHaveBeenCalledOnce())
    expect(mocks.heartbeatRefetch).not.toHaveBeenCalled()
  })

  it("does not duplicate Canister status included by the initial heartbeat", async () => {
    Object.assign(mocks.heartbeat, {
      data: { ready: true, blockers: [], checkedAt: Date.now(), status: { source: "heartbeat" } },
      dataUpdatedAt: Date.now(),
      isFetched: true,
    })

    render(<StatusPage />)

    await waitFor(() => expect(mocks.canisterRefetch).not.toHaveBeenCalled())
  })

  it("loads Canister status when the initial heartbeat has no embedded status", async () => {
    Object.assign(mocks.heartbeat, {
      data: { ready: false, blockers: ["Base RPC unavailable"], checkedAt: Date.now() },
      dataUpdatedAt: Date.now(),
      isFetched: true,
    })

    render(<StatusPage />)

    await waitFor(() => expect(mocks.canisterRefetch).toHaveBeenCalledOnce())
  })

  it("does not repeat the automatic fallback after Canister failure rerenders", async () => {
    Object.assign(mocks.heartbeat, {
      isFetched: true,
      isError: true,
      error: new Error("Base RPC unavailable"),
    })
    const view = render(<StatusPage />)
    await waitFor(() => expect(mocks.canisterRefetch).toHaveBeenCalledOnce())

    Object.assign(mocks.canister, { isError: true, error: new Error("IC unavailable"), isFetching: false })
    view.rerender(<StatusPage />)

    await waitFor(() => expect(mocks.canisterRefetch).toHaveBeenCalledOnce())
  })

  it("refreshes only the lightweight heartbeat when it returns Canister status", async () => {
    render(<StatusPage />)
    fireEvent.click(screen.getByRole("button", { name: "Refresh" }))

    await waitFor(() => expect(mocks.heartbeatRefetch).toHaveBeenCalledOnce())
    expect(mocks.canisterRefetch).not.toHaveBeenCalled()
  })

  it("uses the Canister fallback when the heartbeat refresh fails", async () => {
    mocks.heartbeatRefetch.mockResolvedValue({ data: undefined, isError: true, error: new Error("Base RPC unavailable") })
    render(<StatusPage />)
    fireEvent.click(screen.getByRole("button", { name: "Refresh" }))

    await waitFor(() => expect(mocks.canisterRefetch).toHaveBeenCalledOnce())
    expect(mocks.heartbeatRefetch).toHaveBeenCalledOnce()
  })
})
