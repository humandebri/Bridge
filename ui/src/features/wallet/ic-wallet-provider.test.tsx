import { cleanup, fireEvent, render, screen } from "@testing-library/react"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"
import { clearIcHistoryOwner, loadIcHistoryOwner, saveIcHistoryOwner } from "@/lib/ic-history-owner"
import { IcWalletProviderRoot, useIcWallet } from "./ic-wallet-provider"

const mocks = vi.hoisted(() => ({
  connect: vi.fn(),
  disconnect: vi.fn(),
}))

vi.mock("@/config/profile", () => ({
  deploymentProfile: {
    environment: "test",
    chainId: 84_532,
    bridgeCanisterId: "aaaaa-aa",
    ledgerCanisterId: "aaaaa-aa",
    icHost: "https://icp-api.io",
  },
}))

vi.mock("@/lib/ic/wallet", () => ({
  OisyAdapter: class {
    connect = mocks.connect
    disconnect = mocks.disconnect
  },
  PlugAdapter: class {
    connect = mocks.connect
    disconnect = mocks.disconnect
  },
}))

function Probe() {
  const wallet = useIcWallet()
  return <div>
    <span>signed:{wallet.account?.owner ?? "none"}</span>
    <span>history:{wallet.historyAccount?.owner ?? "none"}</span>
    <span>adapter:{wallet.adapter ? "ready" : "none"}</span>
    <button type="button" onClick={() => void wallet.connect("oisy")}>Connect test wallet</button>
    <button type="button" onClick={() => void wallet.disconnect()}>Disconnect test wallet</button>
  </div>
}

describe("IC wallet history restoration", () => {
  afterEach(cleanup)

  beforeEach(() => {
    clearIcHistoryOwner()
    mocks.connect.mockReset().mockResolvedValue({ owner: "2vxsx-fae" })
    mocks.disconnect.mockReset().mockResolvedValue(undefined)
  })

  it("restores the history owner without restoring signing authority", () => {
    saveIcHistoryOwner({ account: { owner: "aaaaa-aa" }, provider: "oisy" })
    render(<IcWalletProviderRoot><Probe /></IcWalletProviderRoot>)

    expect(screen.getByText("signed:none")).toBeVisible()
    expect(screen.getByText("history:aaaaa-aa")).toBeVisible()
    expect(screen.getByText("adapter:none")).toBeVisible()
  })

  it("updates remembered history on connect and clears it on disconnect", async () => {
    render(<IcWalletProviderRoot><Probe /></IcWalletProviderRoot>)

    fireEvent.click(screen.getByRole("button", { name: "Connect test wallet" }))
    expect(await screen.findByText("signed:2vxsx-fae")).toBeVisible()
    expect(screen.getByText("history:2vxsx-fae")).toBeVisible()
    expect(loadIcHistoryOwner()?.account.owner).toBe("2vxsx-fae")

    fireEvent.click(screen.getByRole("button", { name: "Disconnect test wallet" }))
    expect(await screen.findByText("signed:none")).toBeVisible()
    expect(screen.getByText("history:none")).toBeVisible()
    expect(loadIcHistoryOwner()).toBeUndefined()
  })
})
