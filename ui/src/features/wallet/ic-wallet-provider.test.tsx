import { cleanup, fireEvent, render, screen } from "@testing-library/react"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"
import { clearIcHistoryOwner, loadIcHistoryOwner, saveIcHistoryOwner } from "@/lib/ic-history-owner"
import { IcWalletProviderRoot, useIcWallet } from "./ic-wallet-provider"

const mocks = vi.hoisted(() => ({
  connect: vi.fn(),
  disconnect: vi.fn(),
  oisyConstructed: vi.fn(),
  plugConstructed: vi.fn(),
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
    constructor(...args: unknown[]) { mocks.oisyConstructed(...args) }
    connect = mocks.connect
    disconnect = mocks.disconnect
  },
  PlugAdapter: class {
    constructor(...args: unknown[]) { mocks.plugConstructed(...args) }
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
    mocks.oisyConstructed.mockReset()
    mocks.plugConstructed.mockReset()
  })

  it("restores the remembered OISY account without opening the signer", () => {
    saveIcHistoryOwner({ account: { owner: "aaaaa-aa" }, provider: "oisy" })
    render(<IcWalletProviderRoot><Probe /></IcWalletProviderRoot>)

    expect(screen.getByText("signed:aaaaa-aa")).toBeVisible()
    expect(screen.getByText("history:aaaaa-aa")).toBeVisible()
    expect(screen.getByText("adapter:ready")).toBeVisible()
    expect(mocks.oisyConstructed).toHaveBeenCalledOnce()
    expect(mocks.oisyConstructed.mock.calls[0]?.[4]).toEqual({ owner: "aaaaa-aa", subaccount: undefined })
    expect(mocks.connect).not.toHaveBeenCalled()
  })

  it("restores the remembered Plug account without requesting a new connection", () => {
    saveIcHistoryOwner({ account: { owner: "aaaaa-aa" }, provider: "plug" })
    render(<IcWalletProviderRoot><Probe /></IcWalletProviderRoot>)

    expect(screen.getByText("signed:aaaaa-aa")).toBeVisible()
    expect(screen.getByText("adapter:ready")).toBeVisible()
    expect(mocks.plugConstructed).toHaveBeenCalledOnce()
    expect(mocks.plugConstructed.mock.calls[0]?.[3]).toEqual({ owner: "aaaaa-aa", subaccount: undefined })
    expect(mocks.connect).not.toHaveBeenCalled()
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
