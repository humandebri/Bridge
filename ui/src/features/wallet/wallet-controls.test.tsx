import { cleanup, fireEvent, render, screen, within } from "@testing-library/react"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"
import type { Connector } from "wagmi"
import { deploymentProfile } from "@/config/profile"
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
const coinbase = connector({ id: "coinbaseWalletSDK", uid: "coinbase", name: "Coinbase Wallet", type: "coinbaseWallet" })
const rabby = connector({ id: "io.rabby", uid: "rabby", name: "Rabby Wallet", type: "injected", icon: "data:image/png;base64,AA==" })
const metamask = connector({ id: "io.metamask", uid: "metamask", name: "MetaMask", type: "injected" })
const metamaskSdk = connector({ id: "metaMaskSDK", uid: "metamask-sdk", name: "MetaMask", type: "metaMask" })
const plugEvm = connector({ id: "com.plugwallet", uid: "plug-evm", name: "Plug", type: "injected" })
const walletConnect = connector({ id: "walletConnect", uid: "wallet-connect", name: "WalletConnect", type: "walletConnect" })

describe("wallet controls", () => {
  afterEach(cleanup)

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

  it("never exposes Plug or a generic connector that may resolve to Plug on Base", () => {
    expect(visibleEvmConnectors([plugEvm, injected, metamaskSdk, walletConnect]).map((item) => item.name)).toEqual([
      "MetaMask",
      "WalletConnect",
    ])
  })

  it("shows chain logos in disconnected wallet controls", () => {
    const { unmount } = render(<WalletDialogProvider><WalletCenter /></WalletDialogProvider>)

    const icSummary = screen.getByRole("button", { name: "Connect IC wallet" })
    const baseSummary = screen.getByRole("button", { name: "Connect EVM wallet" })
    expect(within(icSummary).getByRole("img", { name: "Internet Computer logo" })).toHaveAttribute("data-network-logo", "ic")
    expect(within(baseSummary).getByRole("img", { name: "Base logo" })).toHaveAttribute("data-network-logo", "base")
    unmount()
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
    mocks.useConnectors.mockReturnValue([walletConnect, plugEvm, rabby, injected, metamask, coinbase])
    render(<WalletDialogProvider><WalletCenter /></WalletDialogProvider>)
    fireEvent.click(screen.getByRole("button", { name: "Connect EVM wallet" }))

    const section = screen.getByRole("region", { name: "EVM wallet" })
    expect(within(section).queryByRole("button", { name: "Connect Plug" })).not.toBeInTheDocument()
    expect(within(section).queryByRole("button", { name: "Connect Browser wallet" })).not.toBeInTheDocument()
    expect(within(section).getByRole("img", { name: "Coinbase Wallet logo" })).toBeVisible()
    expect(within(section).getByRole("button", { name: "Connect Coinbase Wallet" })).toBeVisible()
    expect(within(section).getByRole("img", { name: "MetaMask logo" })).toBeVisible()
    expect(within(section).getByRole("img", { name: "Rabby Wallet logo" })).toHaveAttribute("src", rabby.icon)
    expect(within(section).getByRole("button", { name: "Connect WalletConnect" })).toBeVisible()
    fireEvent.click(within(section).getByRole("button", { name: "Connect Rabby Wallet" }))

    expect(mocks.connectAsync).toHaveBeenCalledWith({
      connector: rabby,
      chainId: deploymentProfile.chainId,
    })
  })

  it("uses the connected provider logo and only the address in the EVM wallet summary", () => {
    mocks.useAccount.mockReturnValue({ address: "0x1234567890abcdef1234567890abcdef12345678", connector: rabby, isConnected: true })
    render(<WalletDialogProvider><WalletCenter /></WalletDialogProvider>)

    const summary = screen.getByRole("button", { name: /EVM wallet connected as/ })
    expect(within(summary).getByRole("img", { name: "Rabby Wallet logo" })).toHaveAttribute("src", rabby.icon)
    expect(summary).toHaveTextContent("0x1234…5678")
    expect(summary).not.toHaveTextContent("Rabby Wallet")
    fireEvent.click(summary)
    expect(screen.getByText("Review or disconnect the EVM wallet connected to Base.")).toBeVisible()
    expect(screen.getByRole("dialog").querySelector('[data-dialog-network-logo="base"]')).toBeVisible()
  })

  it("uses the connected provider logo and only the principal in the IC wallet summary", () => {
    mocks.useIcWallet.mockReturnValue({
      account: { owner: "aaaaa-aa" },
      provider: "oisy",
      adapter: {},
      connecting: undefined,
      connect: mocks.connectIc,
      disconnect: mocks.disconnectIc,
    })
    render(<WalletDialogProvider><WalletCenter /></WalletDialogProvider>)

    const summary = screen.getByRole("button", { name: /IC wallet connected as/ })
    expect(within(summary).getByRole("img", { name: "OISY Wallet logo" })).toBeVisible()
    expect(summary).toHaveTextContent("aaaaa-…a-aa")
    expect(summary).not.toHaveTextContent("OISY Wallet")
    fireEvent.click(summary)
    expect(screen.getByText("Review or disconnect the Internet Computer wallet connected to this bridge.")).toBeVisible()
    expect(screen.getByRole("dialog").querySelector('[data-dialog-network-logo="ic"]')).toBeVisible()
  })
})
