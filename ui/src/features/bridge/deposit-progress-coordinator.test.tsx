import { cleanup, render, waitFor } from "@testing-library/react"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"

const mocks = vi.hoisted(() => ({
  getDeposit: vi.fn(),
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
  MintAuthorizationAction: () => null,
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
})
