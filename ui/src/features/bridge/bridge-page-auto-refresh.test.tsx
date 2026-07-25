import { StrictMode, useState, type ReactNode } from "react"
import { QueryClient, QueryClientProvider } from "@tanstack/react-query"
import { cleanup, render, screen, waitFor } from "@testing-library/react"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"
import { BridgePage } from "./bridge-page"

const mocks = vi.hoisted(() => ({
  useAccount: vi.fn(),
  useIcWallet: vi.fn(),
  getNextDepositSequence: vi.fn(),
  getPublicConfig: vi.fn(),
  ledgerBalance: vi.fn(),
  ledgerAllowance: vi.fn(),
  bsnsBalance: vi.fn(),
  readDepositIntent: vi.fn(),
  runtimeWriteReadiness: vi.fn(),
}))

vi.mock("wagmi", () => ({
  useAccount: mocks.useAccount,
  useChainId: () => 84_532,
  useConnectorClient: () => ({ data: undefined }),
  useWriteContract: () => ({ isPending: false, writeContractAsync: vi.fn() }),
}))

vi.mock("@tanstack/react-router", () => ({
  Link: ({ children }: { children: ReactNode }) => <a>{children}</a>,
}))

vi.mock("@/features/wallet/ic-wallet-provider", () => ({ useIcWallet: mocks.useIcWallet }))
vi.mock("@/features/wallet/wallet-controls", () => ({ useWalletDialog: () => ({ openFor: vi.fn() }) }))
vi.mock("@/features/status/use-status", () => ({
  useRuntimeValidation: () => ({ data: { ready: true, blockers: [], checkedAt: Date.now() }, isFetching: false, refetch: vi.fn() }),
  useRuntimeHeartbeat: (_chainId: number | undefined, validation: unknown) => ({ data: validation, isFetching: false, refetch: vi.fn() }),
  useRuntimeWriteReadiness: mocks.runtimeWriteReadiness,
  useCurrentBaseQuote: () => ({
    data: {
      serviceFee: 50_000_000n,
      maxServiceFee: 50_000_000n,
      perDepositLimit: 15_000_000_000_000n,
      minted: 0n,
      limit: 15_000_000_000_000n,
      startedAt: 0n,
      duration: 86_400n,
      depositsPaused: false,
      withdrawalsPaused: false,
      bridgeSigner: "0x0000000000000000000000000000000000000001",
    },
    isError: false,
    isStale: false,
    isFetching: false,
    refetch: vi.fn(),
  }),
}))

vi.mock("@/lib/ic/bridge", () => ({
  createBridgeActor: () => Promise.resolve({
    get_next_deposit_sequence: mocks.getNextDepositSequence,
    get_public_config: mocks.getPublicConfig,
  }),
}))

vi.mock("@/lib/ic/ledger", () => ({
  createLedgerActor: () => Promise.resolve({
    icrc1_balance_of: mocks.ledgerBalance,
    icrc2_allowance: mocks.ledgerAllowance,
  }),
  ledgerAccount: vi.fn(() => ({})),
}))

vi.mock("@/lib/evm/client", () => ({
  basePublicClient: { readContract: mocks.bsnsBalance },
}))

vi.mock("@/lib/deposit-intents", () => ({
  readDepositIntent: mocks.readDepositIntent,
  removeDepositIntent: vi.fn(),
  saveDepositIntent: vi.fn(),
}))

function Wrapper({ children }: { children: ReactNode }) {
  const [client] = useState(() => new QueryClient({ defaultOptions: { queries: { retry: false } } }))
  return <StrictMode><QueryClientProvider client={client}>{children}</QueryClientProvider></StrictMode>
}

describe("BridgePage automatic wallet refresh", () => {
  afterEach(cleanup)

  beforeEach(() => {
    mocks.useAccount.mockReset().mockReturnValue({ address: undefined, isConnected: false })
    mocks.useIcWallet.mockReset().mockReturnValue({
      account: undefined,
      provider: undefined,
      adapter: undefined,
      connecting: undefined,
      connect: vi.fn(),
      disconnect: vi.fn(),
    })
    mocks.getNextDepositSequence.mockReset().mockResolvedValue(3n)
    mocks.getPublicConfig.mockReset().mockResolvedValue({ ledger_fee: 10_000n })
    mocks.ledgerBalance.mockReset().mockResolvedValue(1_000_000_000n)
    mocks.ledgerAllowance.mockReset().mockResolvedValue({ allowance: 0n })
    mocks.bsnsBalance.mockReset().mockResolvedValue(1_000_000_000n)
    mocks.readDepositIntent.mockReset().mockReturnValue(undefined)
    mocks.runtimeWriteReadiness.mockReset().mockReturnValue({ ready: true, reason: undefined })
  })

  it("loads IC balance, allowance, and sequence when an IC wallet appears later", async () => {
    const view = render(<BridgePage direction="deposit" onDirectionChange={vi.fn()} />, { wrapper: Wrapper })
    expect(mocks.ledgerBalance).not.toHaveBeenCalled()
    expect(mocks.getNextDepositSequence).not.toHaveBeenCalled()

    mocks.useIcWallet.mockReturnValue({
      account: { owner: "aaaaa-aa" },
      provider: "plug",
      adapter: {},
      connecting: undefined,
      connect: vi.fn(),
      disconnect: vi.fn(),
    })
    view.rerender(<BridgePage direction="deposit" onDirectionChange={vi.fn()} />)

    await waitFor(() => expect(mocks.ledgerBalance).toHaveBeenCalledOnce())
    expect(mocks.ledgerAllowance).toHaveBeenCalledOnce()
    expect(mocks.getNextDepositSequence).toHaveBeenCalledOnce()
    expect(screen.getByText("Balance 10 TICRC1")).toBeInTheDocument()
  })

  it("loads the bSNS balance when a Base wallet appears later", async () => {
    const view = render(<BridgePage direction="withdraw" onDirectionChange={vi.fn()} />, { wrapper: Wrapper })
    expect(mocks.bsnsBalance).not.toHaveBeenCalled()

    mocks.useAccount.mockReturnValue({ address: "0x0000000000000000000000000000000000000002", isConnected: true })
    view.rerender(<BridgePage direction="withdraw" onDirectionChange={vi.fn()} />)

    await waitFor(() => expect(mocks.bsnsBalance).toHaveBeenCalledOnce())
  })

  it("loads the connected wallet balance while runtime validation is unavailable", async () => {
    mocks.runtimeWriteReadiness.mockReturnValue({ ready: false, reason: "Runtime validation expired" })
    mocks.useIcWallet.mockReturnValue({
      account: { owner: "aaaaa-aa" },
      provider: "plug",
      adapter: {},
      connecting: undefined,
      connect: vi.fn(),
      disconnect: vi.fn(),
    })

    render(<BridgePage direction="deposit" onDirectionChange={vi.fn()} />, { wrapper: Wrapper })

    await waitFor(() => expect(mocks.ledgerBalance).toHaveBeenCalledOnce())
    expect(screen.getByText("Balance 10 TICRC1")).toBeInTheDocument()
  })
})
