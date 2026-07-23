import { fireEvent, render, screen, within } from "@testing-library/react"
import { beforeEach, describe, expect, it, vi } from "vitest"
import type { Connector } from "wagmi"
import { WalletCenter, WalletDialogProvider, visibleEvmConnectors } from "./wallet-controls"

const mocks = vi.hoisted(() => ({
  useAccount: vi.fn(),
  useConnect: vi.fn(),
  useConnectors: vi.fn(),
  useDisconnect: vi.fn(),
  useIcWallet: vi.fn(),
  connectAsync: vi.fn(),
  disconnectBase: vi.fn(),
  connectIc: vi.fn(),
  disconnectIc: vi.fn(),
}))

vi.mock("wagmi", () => ({
  useAccount: mocks.useAccount,
  useConnect: mocks.useConnect,
  useConnectors: mocks.useConnectors,
  useDisconnect: mocks.useDisconnect,
}))

vi.mock("@/features/wallet/ic-wallet-provider", () => ({
  useIcWallet: mocks.useIcWallet,
}))

function connector(input: Partial<Connector> & Pick<Connector, "id" | "name" | "type" | "uid">): Connector {
  return input as Connector
}

const injected = connector({ id: "injected", uid: "generic", name: "Injected", type: "injected" })
const rabby = connector({ id: "io.rabby", uid: "rabby", name: "Rabby Wallet", type: "injected", icon: "data:image/png;base64,AA==" })
const metamask = connector({ id: "io.metamask", uid: "metamask", name: "MetaMask", type: "injected" })
const walletConnect = connector({ id: "walletConnect", uid: "wallet-connect", name: "WalletConnect", type: "walletConnect" })

describe("wallet controls", () => {
  beforeEach(() => {
    mocks.connectAsync.mockReset().mockResolvedValue(undefined)
    mocks.disconnectBase.mockReset()
    mocks.connectIc.mockReset().mockResolvedValue(undefined)
    mocks.disconnectIc.mockReset().mockResolvedValue(undefined)
    mocks.useAccount.mockReturnValue({ address: undefined, connector: undefined, isConnected: false })
    mocks.useConnectors.mockReturnValue([])
    mocks.useConnect.mockReturnValue({ connectAsync: mocks.connectAsync, isPending: false, variables: undefined })
    mocks.useDisconnect.mockReturnValue({ disconnect: mocks.disconnectBase })
    mocks.useIcWallet.mockReturnValue({
      account: undefined,
      provider: undefined,
      adapter: undefined,
      connecting: undefined,
      connect: mocks.connectIc,
      disconnect: mocks.disconnectIc,
    })
  })

  it("orders named browser wallets before WalletConnect and removes the generic duplicate", () => {
    expect(visibleEvmConnectors([walletConnect, rabby, injected, metamask]).map((item) => item.name)).toEqual([
      "MetaMask",
      "Rabby Wallet",
      "WalletConnect",
    ])
    expect(visibleEvmConnectors([injected, walletConnect]).map((item) => item.name)).toEqual(["Injected", "WalletConnect"])
  })

  it("shows the official IC wallet logos and connects the selected provider", () => {
    render(<WalletDialogProvider><WalletCenter /></WalletDialogProvider>)
    fireEvent.click(screen.getByRole("button", { name: "Connect IC wallet" }))

    expect(screen.getByRole("img", { name: "OISY Wallet logo" })).toBeVisible()
    expect(screen.getByRole("img", { name: "Plug logo" })).toBeVisible()
    fireEvent.click(screen.getByRole("button", { name: "Connect OISY Wallet" }))

    expect(mocks.connectIc).toHaveBeenCalledWith("oisy")
  })

  it("renders detected EVM wallets as choices and connects the selected connector", () => {
    mocks.useConnectors.mockReturnValue([walletConnect, rabby, injected, metamask])
    render(<WalletDialogProvider><WalletCenter /></WalletDialogProvider>)
    fireEvent.click(screen.getByRole("button", { name: "Connect Base wallet" }))

    const section = screen.getByRole("region", { name: "Base wallet" })
    expect(within(section).queryByRole("button", { name: "Connect Browser wallet" })).not.toBeInTheDocument()
    expect(within(section).getByRole("img", { name: "Rabby Wallet logo" })).toHaveAttribute("src", rabby.icon)
    expect(within(section).getByRole("button", { name: "Connect WalletConnect" })).toBeVisible()
    fireEvent.click(within(section).getByRole("button", { name: "Connect Rabby Wallet" }))

    expect(mocks.connectAsync).toHaveBeenCalledWith({ connector: rabby })
  })

  it("uses the connected provider logo and name in the Base wallet summary", () => {
    mocks.useAccount.mockReturnValue({ address: "0x1234567890abcdef1234567890abcdef12345678", connector: rabby, isConnected: true })
    render(<WalletDialogProvider><WalletCenter /></WalletDialogProvider>)

    const summary = screen.getByRole("button", { name: /Base wallet connected as/ })
    expect(within(summary).getByRole("img", { name: "Rabby Wallet logo" })).toHaveAttribute("src", rabby.icon)
    expect(summary).toHaveTextContent("Rabby Wallet")
  })

  it("uses the connected IC provider logo and name in the IC wallet summary", () => {
    mocks.useIcWallet.mockReturnValue({
      account: { owner: "aaaaa-aa" },
      provider: "plug",
      adapter: {},
      connecting: undefined,
      connect: mocks.connectIc,
      disconnect: mocks.disconnectIc,
    })
    render(<WalletDialogProvider><WalletCenter /></WalletDialogProvider>)

    const summary = screen.getByRole("button", { name: /IC wallet connected as/ })
    expect(within(summary).getByRole("img", { name: "Plug logo" })).toBeVisible()
    expect(summary).toHaveTextContent("Plug")
  })
})
