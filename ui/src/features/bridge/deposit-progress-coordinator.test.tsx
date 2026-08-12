import { act, cleanup, render, waitFor } from "@testing-library/react"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"

const mocks = vi.hoisted(() => ({
  getDeposit: vi.fn(),
  mintAuthorizationAction: vi.fn<(props: unknown) => null>(() => null),
  update: vi.fn(),
  setAction: vi.fn(),
  progress: {
    id: "deposit:1",
    direction: "deposit" as const,
    phase: "authorization-generating" as const,
    source: "aaaaa-aa",
    destination: "0x0000000000000000000000000000000000000002",
    sendAmount: "2",
    receiveAmount: "1.5",
    sendSymbol: "TICRC1",
    receiveSymbol: "KINIC",
    deposit: { owner: "aaaaa-aa", ownerSequence: "3", depositId: `0x${"07".repeat(32)}` },
  },
}))

vi.mock("@/features/bridge/bridge-progress-provider", () => ({
  useBridgeProgress: () => ({ progress: mocks.progress, update: mocks.update, setAction: mocks.setAction }),
}))
vi.mock("@/features/bridge/mint-authorization-action", () => ({
  MintAuthorizationAction: mocks.mintAuthorizationAction,
}))
vi.mock("@/lib/ic/bridge", () => ({
  createBridgeActor: vi.fn().mockResolvedValue({ get_deposit_by_owner_sequence: mocks.getDeposit }),
}))
vi.mock("@/config/profile", () => ({
  deploymentProfile: { icHost: "https://ic.example", bridgeCanisterId: "aaaaa-aa" },
}))

import { DepositProgressCoordinator } from "./deposit-progress-coordinator"

beforeEach(() => {
  vi.clearAllMocks()
  mocks.getDeposit.mockResolvedValue([{ state: { Minted: null } }])
})

afterEach(cleanup)

describe("DepositProgressCoordinator", () => {
  it("completes a restored deposit when the canonical record is Minted", async () => {
    render(<DepositProgressCoordinator />)

    await waitFor(() => expect(mocks.update).toHaveBeenCalledWith("deposit:1", {
      phase: "complete",
      completionMessage: "1.5 KINIC was minted on Base.",
    }))
  })

  it("automatically opens the Base wallet when mint authorization becomes available", async () => {
    mocks.getDeposit.mockResolvedValue([{
      state: { AuthorizationAvailable: null },
      mint_authorization: [{}],
    }])

    render(<DepositProgressCoordinator />)

    await waitFor(() => expect(mocks.mintAuthorizationAction).toHaveBeenCalled())
    expect(mocks.mintAuthorizationAction.mock.calls.at(-1)?.[0]).toEqual(expect.objectContaining({
      autoPromptOwner: "aaaaa-aa",
      headless: true,
    }))
  })

  it("keeps_a_restored_deposit_pending_during_an_IC_query_failure_then_resumes_after_remount", async () => {
    mocks.getDeposit.mockRejectedValueOnce(new Error("IC query unavailable"))
    const first = render(<DepositProgressCoordinator />)

    await waitFor(() => expect(mocks.getDeposit).toHaveBeenCalledOnce())
    expect(mocks.update).not.toHaveBeenCalled()
    expect(mocks.mintAuthorizationAction).not.toHaveBeenCalled()

    first.unmount()
    mocks.getDeposit.mockResolvedValue([{ state: { AuthorizationPending: null } }])
    render(<DepositProgressCoordinator />)

    await waitFor(() => expect(mocks.update).toHaveBeenCalledWith("deposit:1", {
      phase: "authorization-generating",
    }))
    expect(mocks.mintAuthorizationAction).not.toHaveBeenCalled()
  })

  it("records successful and reverted Base receipts as distinct presentation facts", async () => {
    mocks.getDeposit.mockResolvedValue([{
      state: { AuthorizationAvailable: null },
      mint_authorization: [{}],
    }])
    render(<DepositProgressCoordinator />)
    await waitFor(() => expect(mocks.mintAuthorizationAction).toHaveBeenCalled())
    const props = mocks.mintAuthorizationAction.mock.calls.at(-1)?.[0] as {
      onProgress: (event: unknown) => void
    }

    act(() => props.onProgress({ phase: "included", transactionHash: `0x${"22".repeat(32)}`, blockNumber: 123n, outcome: "success" }))
    expect(mocks.update).toHaveBeenLastCalledWith("deposit:1", expect.objectContaining({
      phase: "base-mint-included",
      baseTransactionOutcome: "success",
    }))

    act(() => props.onProgress({ phase: "included", transactionHash: `0x${"22".repeat(32)}`, blockNumber: 123n, outcome: "reverted" }))
    expect(mocks.update).toHaveBeenLastCalledWith("deposit:1", expect.objectContaining({
      phase: "base-mint-included",
      baseTransactionOutcome: "reverted",
    }))
  })
})
