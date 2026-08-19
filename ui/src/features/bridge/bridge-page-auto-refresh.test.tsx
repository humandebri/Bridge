import { StrictMode, useState, type ReactNode } from "react"
import { QueryClient, QueryClientProvider } from "@tanstack/react-query"
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"
import { BridgePage } from "./bridge-page"
import { BridgeProgressProvider } from "./bridge-progress-provider"
import { browserLocalStorage } from "@/lib/browser-lock"
import type * as BrowserLockModule from "@/lib/browser-lock"
import { createBridgeProgress, saveLatestBridgeProgress } from "@/lib/bridge-progress"

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
  runtimeHeartbeatHook: vi.fn(),
  runtimeRefetch: vi.fn(),
  heartbeatIsError: { value: false },
  heartbeatIsFetching: { value: false },
  heartbeatDepositsPaused: { value: false },
  heartbeatWithdrawalsPaused: { value: false },
  baseRefetch: vi.fn(),
  writeContractAsync: vi.fn(),
  savePendingConfirmation: vi.fn(),
  getDepositByOwnerSequence: vi.fn(),
  removeDepositIntent: vi.fn(),
  saveDepositIntent: vi.fn(),
  requireWalletSnapshot: vi.fn(),
  runtimeValidation: { value: { ready: true, blockers: [] as string[], checkedAt: 0 } },
}))

vi.mock("wagmi", () => ({
  useAccount: mocks.useAccount,
  useChainId: () => 84_532,
  useConnectorClient: () => ({ data: undefined }),
  useWriteContract: () => ({ isPending: false, writeContractAsync: mocks.writeContractAsync }),
}))

vi.mock("@tanstack/react-router", () => ({
  Link: ({ children }: { children: ReactNode }) => <a>{children}</a>,
}))

vi.mock("@/features/wallet/ic-wallet-provider", () => ({ useIcWallet: mocks.useIcWallet }))
vi.mock("@/features/wallet/wallet-controls", () => ({ useWalletDialog: () => ({ openFor: vi.fn() }) }))
vi.mock("@/features/status/use-status", () => ({
  useRuntimeValidation: () => ({ data: mocks.runtimeValidation.value, isFetching: false, refetch: mocks.runtimeRefetch }),
  useRuntimeHeartbeat: (...args: unknown[]) => {
    mocks.runtimeHeartbeatHook(...args)
    return {
      data: {
        ready: true,
        blockers: [],
        checkedAt: Date.now(),
        snapshot: {
          serviceFee: 50_000_000n,
          maxServiceFee: 50_000_000n,
          perDepositLimit: 15_000_000_000_000n,
          minted: 0n,
          limit: 15_000_000_000_000n,
          startedAt: 0n,
          duration: 86_400n,
          depositsPaused: mocks.heartbeatDepositsPaused.value,
          withdrawalsPaused: mocks.heartbeatWithdrawalsPaused.value,
          bridgeSigner: "0x0000000000000000000000000000000000000001",
          mintAuthorizationEpoch: 1n,
          blockTimestamp: 1_000n,
        },
      },
      isError: mocks.heartbeatIsError.value,
      isFetching: mocks.heartbeatIsFetching.value,
      refetch: mocks.baseRefetch,
    }
  },
  finalizedObservationQuote: (observation: { snapshot?: unknown }) => observation?.snapshot,
  useRuntimeWriteReadiness: mocks.runtimeWriteReadiness,
}))

vi.mock("@/lib/ic/bridge", () => ({
  createBridgeActor: () => Promise.resolve({
    get_next_deposit_sequence: mocks.getNextDepositSequence,
    get_deposit_by_owner_sequence: mocks.getDepositByOwnerSequence,
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
  removeDepositIntent: mocks.removeDepositIntent,
  saveDepositIntent: mocks.saveDepositIntent,
}))

vi.mock("@/lib/pending-confirmations", () => ({
  savePendingConfirmation: mocks.savePendingConfirmation,
}))

vi.mock("@/lib/browser-lock", async (importOriginal) => ({
  ...await importOriginal<typeof BrowserLockModule>(),
  withBrowserLock: (_name: string, action: () => unknown) => action(),
}))

vi.mock("@/lib/wallet-snapshot", () => ({
  currentInjectedWallet: vi.fn().mockResolvedValue({
    address: "0x0000000000000000000000000000000000000002",
    chainId: 84_532,
  }),
  requireWalletSnapshot: mocks.requireWalletSnapshot,
  sameIcAccount: vi.fn().mockReturnValue(true),
}))

function Wrapper({ children }: { children: ReactNode }) {
  const [client] = useState(() => new QueryClient({ defaultOptions: { queries: { retry: false } } }))
  return <StrictMode><QueryClientProvider client={client}><BridgeProgressProvider>{children}</BridgeProgressProvider></QueryClientProvider></StrictMode>
}

describe("BridgePage automatic wallet refresh", () => {
  afterEach(cleanup)

  beforeEach(() => {
    browserLocalStorage().clear()
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
    mocks.getPublicConfig.mockReset().mockResolvedValue({ ledger_fee: 100_000n })
    mocks.ledgerBalance.mockReset().mockResolvedValue(1_000_000_000n)
    mocks.ledgerAllowance.mockReset().mockResolvedValue({ allowance: 0n })
    mocks.bsnsBalance.mockReset().mockResolvedValue(1_000_000_000n)
    mocks.readDepositIntent.mockReset().mockReturnValue(undefined)
    mocks.runtimeWriteReadiness.mockReset().mockReturnValue({ ready: true, reason: undefined })
    mocks.runtimeHeartbeatHook.mockReset()
    mocks.heartbeatIsError.value = false
    mocks.heartbeatIsFetching.value = false
    mocks.heartbeatDepositsPaused.value = false
    mocks.heartbeatWithdrawalsPaused.value = false
    mocks.runtimeValidation.value = { ready: true, blockers: [], checkedAt: Date.now() }
    mocks.runtimeRefetch.mockReset().mockResolvedValue({
      data: { ready: true, blockers: [], checkedAt: Date.now() },
    })
    mocks.baseRefetch.mockReset().mockResolvedValue({
      data: {
        ready: true,
        blockers: [],
        checkedAt: Date.now(),
        snapshot: {
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
          mintAuthorizationEpoch: 1n,
          blockTimestamp: 1_000n,
        },
      },
      isError: false,
      isStale: false,
    })
    mocks.writeContractAsync.mockReset().mockResolvedValue(`0x${"77".repeat(32)}`)
    mocks.savePendingConfirmation.mockReset().mockResolvedValue(undefined)
    mocks.requireWalletSnapshot.mockReset()
    mocks.getDepositByOwnerSequence.mockReset().mockResolvedValue([{
      state: { AuthorizationPending: null },
    }])
    mocks.removeDepositIntent.mockReset().mockResolvedValue(undefined)
    mocks.saveDepositIntent.mockReset().mockResolvedValue(undefined)
  })

  it("refreshes only the heartbeat after full runtime validation succeeds", async () => {
    render(<BridgePage direction="deposit" onDirectionChange={vi.fn()} />, { wrapper: Wrapper })

    fireEvent.click(screen.getByRole("button", { name: "Refresh" }))

    await waitFor(() => expect(mocks.baseRefetch).toHaveBeenCalledOnce())
    expect(mocks.runtimeRefetch).not.toHaveBeenCalled()
  })

  it("manual refresh stays lightweight when full runtime validation has not succeeded", async () => {
    mocks.runtimeValidation.value = { ready: false, blockers: ["Runtime validation failed"], checkedAt: Date.now() }
    mocks.runtimeWriteReadiness.mockReturnValue({ ready: false, reason: "Runtime validation failed" })
    render(<BridgePage direction="deposit" onDirectionChange={vi.fn()} />, { wrapper: Wrapper })

    fireEvent.click(screen.getByRole("button", { name: "Refresh" }))

    await waitFor(() => expect(mocks.baseRefetch).toHaveBeenCalledOnce())
    expect(mocks.runtimeRefetch).not.toHaveBeenCalled()
  })

  it("refreshes expired full runtime validation only after review starts", async () => {
    mocks.runtimeValidation.value = { ready: true, blockers: [], checkedAt: Date.now() - 60_001 }
    const account = { owner: "aaaaa-aa" }
    mocks.useAccount.mockReturnValue({ address: "0x0000000000000000000000000000000000000002", isConnected: true })
    mocks.useIcWallet.mockReturnValue({ account, provider: "oisy", adapter: { getAccount: vi.fn().mockResolvedValue(account) }, connect: vi.fn(), disconnect: vi.fn() })
    render(<BridgePage direction="deposit" onDirectionChange={vi.fn()} />, { wrapper: Wrapper })
    await waitFor(() => expect(mocks.ledgerBalance).toHaveBeenCalled())
    fireEvent.change(screen.getByRole("textbox", { name: "You send" }), { target: { value: "2" } })

    fireEvent.click(screen.getByRole("button", { name: "Bridge to Base" }))

    await waitFor(() => expect(mocks.runtimeRefetch).toHaveBeenCalledOnce())
    expect(screen.getByRole("heading", { name: "Review bridge to Base" })).toBeVisible()
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

  it("treats stale live status as non-blocking and does not report an outage", () => {
    mocks.runtimeWriteReadiness.mockReturnValue({ ready: false, reason: "Runtime validation failed" })

    render(<BridgePage direction="deposit" onDirectionChange={vi.fn()} />, { wrapper: Wrapper })

    expect(screen.getByText("Live status is not confirmed. Current conditions will be checked before continuing.")).toBeVisible()
    expect(screen.queryByText("Bridge is temporarily unavailable. Try Refresh.")).not.toBeInTheDocument()
    expect(screen.getByRole("button", { name: "Refresh" })).toBeEnabled()
  })

  it("hides an RPC refresh error while keeping the last known fee", () => {
    mocks.heartbeatIsError.value = true

    render(<BridgePage direction="deposit" onDirectionChange={vi.fn()} />, { wrapper: Wrapper })

    expect(screen.queryByText("Live status could not be refreshed. Current conditions will be checked before continuing.")).not.toBeInTheDocument()
    expect(screen.queryByRole("link", { name: "View status" })).not.toBeInTheDocument()
    expect(screen.getByText("Last known bridge fee")).toBeVisible()
    expect(screen.getByText("0.5 TICRC1")).toBeVisible()
    expect(screen.getByRole("button", { name: "Refresh" })).toBeEnabled()
  })

  it("disables focus and reconnect heartbeat refreshes on the Bridge page", () => {
    render(<BridgePage direction="deposit" onDirectionChange={vi.fn()} />, { wrapper: Wrapper })

    expect(mocks.runtimeHeartbeatHook).toHaveBeenCalledWith(84_532, mocks.runtimeValidation.value, {
      enabled: true,
      refetchOnWindowFocus: false,
      refetchOnReconnect: false,
    })
  })

  it("blocks the entry point only when a fresh heartbeat confirms the selected direction is paused", () => {
    const account = { owner: "aaaaa-aa" }
    mocks.heartbeatDepositsPaused.value = true
    mocks.useAccount.mockReturnValue({ address: "0x0000000000000000000000000000000000000002", isConnected: true })
    mocks.useIcWallet.mockReturnValue({
      account,
      provider: "oisy",
      adapter: {
        prepare: vi.fn().mockResolvedValue(() => Promise.resolve()),
        getAccount: vi.fn().mockResolvedValue(account),
      },
      connect: vi.fn(),
      disconnect: vi.fn(),
    })

    render(<BridgePage direction="deposit" onDirectionChange={vi.fn()} />, { wrapper: Wrapper })
    fireEvent.change(screen.getByRole("textbox", { name: "You send" }), { target: { value: "2" } })

    expect(screen.getByRole("button", { name: "Bridge to Base" })).toBeDisabled()
    expect(screen.getByText("Next: Deposits are paused on Base")).toBeVisible()
  })

  it("runs withdrawal preflight checks before enabling confirmation", async () => {
    const account = { owner: "aaaaa-aa" }
    mocks.useAccount.mockReturnValue({
      address: "0x0000000000000000000000000000000000000002",
      isConnected: true,
    })
    mocks.useIcWallet.mockReturnValue({
      account,
      provider: "oisy",
      adapter: {
        prepare: vi.fn().mockResolvedValue(() => Promise.resolve()),
        getAccount: vi.fn().mockResolvedValue(account),
      },
      connecting: undefined,
      connect: vi.fn(),
      disconnect: vi.fn(),
    })

    render(<BridgePage direction="withdraw" onDirectionChange={vi.fn()} />, { wrapper: Wrapper })
    await waitFor(() => expect(mocks.bsnsBalance).toHaveBeenCalled())
    fireEvent.change(screen.getByRole("textbox", { name: "You send" }), { target: { value: "2" } })
    fireEvent.click(screen.getByRole("button", { name: "Bridge to IC" }))

    expect(screen.getByRole("heading", { name: "Review bridge to IC" })).toBeVisible()
    expect(await screen.findByText("Review the transfer details before continuing.")).toBeVisible()
    expect(screen.queryByText("Wallets connected")).not.toBeInTheDocument()
    expect(screen.queryByText("Transfer availability checked")).not.toBeInTheDocument()
    expect(screen.getByRole("button", { name: "Continue to Base wallet" })).toBeEnabled()
    expect(screen.queryByText(/no Base refund/)).not.toBeInTheDocument()
  })

  it("submits the reviewed IC destination without reopening OISY", async () => {
    const account = { owner: "2vxsx-fae", subaccount: new Uint8Array(32).fill(0x55) }
    const events: string[] = []
    const close = vi.fn().mockResolvedValue(undefined)
    const prepare = vi.fn(() => {
      events.push("prepare")
      return Promise.resolve(close)
    })
    const getAccount = vi.fn(() => {
      events.push("getAccount")
      return Promise.resolve({ owner: account.owner, subaccount: account.subaccount.slice() })
    })
    mocks.writeContractAsync.mockImplementation(() => {
      events.push("baseWrite")
      return Promise.resolve(`0x${"77".repeat(32)}`)
    })
    mocks.useAccount.mockReturnValue({
      address: "0x0000000000000000000000000000000000000002",
      isConnected: true,
    })
    mocks.useIcWallet.mockReturnValue({
      account,
      provider: "oisy",
      adapter: { prepare, getAccount },
      connecting: undefined,
      connect: vi.fn(),
      disconnect: vi.fn(),
    })

    render(<BridgePage direction="withdraw" onDirectionChange={vi.fn()} />, { wrapper: Wrapper })
    await waitFor(() => expect(mocks.bsnsBalance).toHaveBeenCalled())
    fireEvent.change(screen.getByRole("textbox", { name: "You send" }), { target: { value: "2" } })
    fireEvent.click(screen.getByRole("button", { name: "Bridge to IC" }))
    fireEvent.click(await screen.findByRole("button", { name: "Continue to Base wallet" }))

    expect(screen.getByRole("listitem", { current: "step" })).toHaveTextContent("IC destination verification")
    await waitFor(() => expect(mocks.writeContractAsync).toHaveBeenCalledOnce())
    expect(screen.getByText("Base token approval").parentElement).toHaveTextContent("Not required")
    expect(screen.getByRole("listitem", { current: "step" })).toHaveTextContent("Base withdrawal transaction")
    expect(prepare).toHaveBeenCalledOnce()
    expect(close).toHaveBeenCalledOnce()
    expect(events.indexOf("getAccount")).toBeLessThan(events.indexOf("baseWrite"))
    expect(mocks.writeContractAsync).toHaveBeenCalledWith(expect.objectContaining({
      functionName: "createWithdrawal",
      args: [200_000_000n, 50_000_000n, "0x04", `0x${"55".repeat(32)}`],
    }))
    await waitFor(() => expect(mocks.savePendingConfirmation).toHaveBeenCalledWith(expect.objectContaining({
      kind: "withdrawal",
      owner: account.owner,
    })))
  })

  it("rejects_a_changed_OISY_account_before_reviewing_the_irreversible_destination", async () => {
    const remembered = { owner: "aaaaa-aa" }
    const prepare = vi.fn().mockRejectedValue(new Error("OISY account changed; reconnect and review the transaction"))
    mocks.useAccount.mockReturnValue({
      address: "0x0000000000000000000000000000000000000002",
      isConnected: true,
    })
    mocks.useIcWallet.mockReturnValue({
      account: remembered,
      provider: "oisy",
      adapter: {
        prepare,
        getAccount: vi.fn().mockResolvedValue({ owner: "2vxsx-fae" }),
      },
      connecting: undefined,
      connect: vi.fn(),
      disconnect: vi.fn(),
    })

    render(<BridgePage direction="withdraw" onDirectionChange={vi.fn()} />, { wrapper: Wrapper })
    await waitFor(() => expect(mocks.bsnsBalance).toHaveBeenCalled())
    fireEvent.change(screen.getByRole("textbox", { name: "You send" }), { target: { value: "2" } })
    fireEvent.click(screen.getByRole("button", { name: "Bridge to IC" }))

    expect(await screen.findByText("OISY account changed; reconnect and review the transaction")).toBeVisible()
    expect(screen.queryByRole("button", { name: "Continue to Base wallet" })).not.toBeInTheDocument()
    expect(mocks.writeContractAsync).not.toHaveBeenCalled()
    expect(prepare).toHaveBeenCalledOnce()
  })

  it("shows a failed preflight check and restarts all checks on retry", async () => {
    const account = { owner: "aaaaa-aa" }
    mocks.useAccount.mockReturnValue({
      address: "0x0000000000000000000000000000000000000002",
      isConnected: true,
    })
    mocks.useIcWallet.mockReturnValue({
      account,
      provider: "oisy",
      adapter: {
        prepare: vi.fn().mockResolvedValue(() => Promise.resolve()),
        getAccount: vi.fn().mockResolvedValue(account),
      },
      connecting: undefined,
      connect: vi.fn(),
      disconnect: vi.fn(),
    })
    mocks.baseRefetch.mockRejectedValueOnce(new Error("Bridge configuration could not be verified"))

    render(<BridgePage direction="withdraw" onDirectionChange={vi.fn()} />, { wrapper: Wrapper })
    await waitFor(() => expect(mocks.bsnsBalance).toHaveBeenCalled())
    fireEvent.change(screen.getByRole("textbox", { name: "You send" }), { target: { value: "2" } })
    fireEvent.click(screen.getByRole("button", { name: "Bridge to IC" }))

    expect(await screen.findByText("Bridge configuration could not be verified")).toBeVisible()
    fireEvent.click(screen.getByRole("button", { name: "Try again" }))
    expect(await screen.findByText("Review the transfer details before continuing.")).toBeVisible()
    expect(mocks.baseRefetch).toHaveBeenCalledTimes(2)
  })

  it("ignores a preflight result after the dialog is closed", async () => {
    const account = { owner: "aaaaa-aa" }
    let resolveRuntime!: (value: { data: {
      ready: boolean
      blockers: string[]
      checkedAt: number
      snapshot: {
        serviceFee: bigint
        maxServiceFee: bigint
        perDepositLimit: bigint
        minted: bigint
        limit: bigint
        startedAt: bigint
        duration: bigint
        depositsPaused: boolean
        withdrawalsPaused: boolean
        bridgeSigner: string
        mintAuthorizationEpoch: bigint
        blockTimestamp: bigint
      }
    } }) => void
    mocks.baseRefetch.mockReturnValueOnce(new Promise((resolve) => { resolveRuntime = resolve }))
    mocks.useAccount.mockReturnValue({
      address: "0x0000000000000000000000000000000000000002",
      isConnected: true,
    })
    mocks.useIcWallet.mockReturnValue({
      account,
      provider: "oisy",
      adapter: {
        prepare: vi.fn().mockResolvedValue(() => Promise.resolve()),
        getAccount: vi.fn().mockResolvedValue(account),
      },
      connecting: undefined,
      connect: vi.fn(),
      disconnect: vi.fn(),
    })

    render(<BridgePage direction="withdraw" onDirectionChange={vi.fn()} />, { wrapper: Wrapper })
    await waitFor(() => expect(mocks.bsnsBalance).toHaveBeenCalled())
    fireEvent.change(screen.getByRole("textbox", { name: "You send" }), { target: { value: "2" } })
    fireEvent.click(screen.getByRole("button", { name: "Bridge to IC" }))
    expect(screen.getByRole("heading", { name: "Review bridge to IC" })).toBeVisible()
    await waitFor(() => expect(mocks.baseRefetch).toHaveBeenCalled())
    expect(screen.getByText("Checking your wallets, balance, fees, and bridge availability…")).toBeVisible()
    expect(screen.queryByText("Wallets connected")).not.toBeInTheDocument()
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }))

    act(() => resolveRuntime({ data: {
      ready: true,
      blockers: [],
      checkedAt: Date.now(),
      snapshot: {
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
        mintAuthorizationEpoch: 1n,
        blockTimestamp: 1_000n,
      },
    } }))
    expect(screen.queryByRole("heading", { name: "Review bridge to IC" })).not.toBeInTheDocument()
  })

  it("checks a retry before opening Oisy and then shows its in-flight progress", async () => {
    const requestDeposit = vi.fn(() => new Promise(() => undefined))
    const closeWallet = vi.fn().mockResolvedValue(undefined)
    const prepare = vi.fn().mockResolvedValue(closeWallet)
    const account = { owner: "aaaaa-aa" }
    mocks.useAccount.mockReturnValue({
      address: "0x0000000000000000000000000000000000000002",
      isConnected: true,
    })
    mocks.useIcWallet.mockReturnValue({
      account,
      provider: "plug",
      adapter: {
        prepare,
        getAccount: vi.fn().mockResolvedValue(account),
        requestDeposit,
      },
      connecting: undefined,
      connect: vi.fn(),
      disconnect: vi.fn(),
    })
    mocks.readDepositIntent.mockReturnValue({
      account,
      recipient: "0x0000000000000000000000000000000000000002",
      call: {
        ownerSequence: 3n,
        baseRecipient: new Uint8Array(20).fill(2),
        grossAmount: 200_000_000n,
        maxServiceFee: 50_000_000n,
      },
      state: "submitted",
    })

    render(<BridgePage direction="deposit" onDirectionChange={vi.fn()} />, { wrapper: Wrapper })

    expect(await screen.findByText("Deposit status unavailable")).toBeVisible()
    fireEvent.click(screen.getByRole("button", { name: "Retry same deposit" }))
    expect(screen.getByRole("heading", { name: "Review bridge to Base" })).toBeVisible()
    expect(screen.getByText("Checking your wallets, balance, fees, and bridge availability…")).toBeVisible()
    expect(prepare).not.toHaveBeenCalled()
    fireEvent.click(await screen.findByRole("button", { name: "Continue to IC wallet" }))
    expect(prepare).toHaveBeenCalledOnce()

    await waitFor(() => expect(requestDeposit).toHaveBeenCalledOnce())
    expect(screen.getByRole("listitem", { current: "step" })).toHaveTextContent("IC deposit transaction")
    expect(closeWallet).not.toHaveBeenCalled()
  })

  it("restores_a_prepared_Deposit_without_automatically_reopening_the_IC_wallet", async () => {
    const requestDeposit = vi.fn()
    const account = { owner: "aaaaa-aa" }
    mocks.useAccount.mockReturnValue({
      address: "0x0000000000000000000000000000000000000002",
      isConnected: true,
    })
    mocks.useIcWallet.mockReturnValue({
      account,
      provider: "oisy",
      adapter: { requestDeposit },
      connecting: undefined,
      connect: vi.fn(),
      disconnect: vi.fn(),
    })
    mocks.readDepositIntent.mockReturnValue({
      account,
      recipient: "0x0000000000000000000000000000000000000002",
      call: {
        ownerSequence: 3n,
        baseRecipient: new Uint8Array(20).fill(2),
        grossAmount: 200_000_000n,
        maxServiceFee: 50_000_000n,
      },
      state: "prepared",
    })

    render(<BridgePage direction="deposit" onDirectionChange={vi.fn()} />, { wrapper: Wrapper })

    expect(await screen.findByText("Deposit status unavailable")).toBeVisible()
    expect(screen.getByRole("button", { name: "Check status" })).toBeEnabled()
    expect(requestDeposit).not.toHaveBeenCalled()
    expect(mocks.saveDepositIntent).not.toHaveBeenCalled()
  })

  it("rebuilds global progress when a saved Deposit intent is already accepted", async () => {
    const account = { owner: "aaaaa-aa" }
    const recipient = "0x0000000000000000000000000000000000000002" as const
    mocks.useAccount.mockReturnValue({ address: recipient, isConnected: true })
    mocks.useIcWallet.mockReturnValue({
      account,
      provider: "plug",
      adapter: {},
      connecting: undefined,
      connect: vi.fn(),
      disconnect: vi.fn(),
    })
    mocks.readDepositIntent.mockReturnValue({
      account,
      recipient,
      call: {
        ownerSequence: 3n,
        baseRecipient: new Uint8Array(20).fill(2),
        grossAmount: 200_000_000n,
        maxServiceFee: 50_000_000n,
      },
      state: "submitted",
    })
    mocks.getNextDepositSequence.mockResolvedValue(4n)
    mocks.getDepositByOwnerSequence.mockResolvedValue([{
      state: { AuthorizationAvailable: null },
      deposit_id: new Uint8Array(32).fill(7),
      owner_sequence: 3n,
      gross_amount: 200_000_000n,
      max_service_fee: 50_000_000n,
      base_recipient: new Uint8Array(20).fill(2),
      from_subaccount: [],
      quote: [{ net_amount: 150_000_000n, service_fee: 50_000_000n }],
      mint_authorization: [],
    }])

    render(<BridgePage direction="deposit" onDirectionChange={vi.fn()} />, { wrapper: Wrapper })

    fireEvent.click(await screen.findByRole("button", { name: "Check status" }))

    expect(await screen.findByRole("heading", { name: "Bridge to Base" })).toBeVisible()
    expect(screen.getByRole("listitem", { current: "step" })).toHaveTextContent("Base mint transaction")
    expect(mocks.removeDepositIntent).toHaveBeenCalledOnce()
  })

  it("rejects_a_canonical_Deposit_that_does_not_match_the_saved_intent", async () => {
    const account = { owner: "aaaaa-aa" }
    const recipient = "0x0000000000000000000000000000000000000002" as const
    mocks.useAccount.mockReturnValue({ address: recipient, isConnected: true })
    mocks.useIcWallet.mockReturnValue({
      account,
      provider: "plug",
      adapter: {},
      connecting: undefined,
      connect: vi.fn(),
      disconnect: vi.fn(),
    })
    mocks.readDepositIntent.mockReturnValue({
      account,
      recipient,
      call: {
        ownerSequence: 3n,
        baseRecipient: new Uint8Array(20).fill(2),
        grossAmount: 200_000_000n,
        maxServiceFee: 50_000_000n,
      },
      state: "submitted",
    })
    mocks.getNextDepositSequence.mockResolvedValue(4n)
    mocks.getDepositByOwnerSequence.mockResolvedValue([{
      state: { AuthorizationPending: null },
      deposit_id: new Uint8Array(32).fill(7),
      owner_sequence: 3n,
      gross_amount: 200_000_001n,
      max_service_fee: 50_000_000n,
      base_recipient: new Uint8Array(20).fill(2),
      from_subaccount: [],
      quote: [],
      mint_authorization: [],
    }])

    render(<BridgePage direction="deposit" onDirectionChange={vi.fn()} />, { wrapper: Wrapper })
    fireEvent.click(await screen.findByRole("button", { name: "Check status" }))

    await waitFor(() => expect(mocks.getDepositByOwnerSequence).toHaveBeenCalledOnce())
    expect(mocks.getDepositByOwnerSequence.mock.calls[0]?.[1]).toBe(3n)
    expect(mocks.removeDepositIntent).not.toHaveBeenCalled()
    expect(screen.queryByRole("button", { name: /Open transfer progress/ })).not.toBeInTheDocument()
    expect(screen.getByRole("button", { name: "Check status" })).toBeEnabled()
  })

  it("does not replace an unrelated active transfer while recovering a saved Deposit", async () => {
    const account = { owner: "aaaaa-aa" }
    mocks.useAccount.mockReturnValue({ address: "0x0000000000000000000000000000000000000002", isConnected: true })
    mocks.useIcWallet.mockReturnValue({
      account,
      provider: "plug",
      adapter: {},
      connecting: undefined,
      connect: vi.fn(),
      disconnect: vi.fn(),
    })
    saveLatestBridgeProgress(createBridgeProgress({
      direction: "withdraw",
      phase: "base-withdrawal-submitted",
      source: "0x0000000000000000000000000000000000000003",
      destination: "aaaaa-aa",
      sendAmount: "2",
      receiveAmount: "1.5",
      sendSymbol: "KINIC",
      receiveSymbol: "TICRC1",
      transactionHash: `0x${"33".repeat(32)}`,
      withdrawal: { owner: "aaaaa-aa" },
    }))
    mocks.readDepositIntent.mockReturnValue({
      account,
      recipient: "0x0000000000000000000000000000000000000002",
      call: {
        ownerSequence: 3n,
        baseRecipient: new Uint8Array(20).fill(2),
        grossAmount: 200_000_000n,
        maxServiceFee: 50_000_000n,
      },
      state: "submitted",
    })
    mocks.getNextDepositSequence.mockResolvedValue(4n)
    mocks.getDepositByOwnerSequence.mockResolvedValue([{
      state: { AuthorizationPending: null },
      deposit_id: new Uint8Array(32).fill(7),
      owner_sequence: 3n,
      gross_amount: 200_000_000n,
      max_service_fee: 50_000_000n,
      base_recipient: new Uint8Array(20).fill(2),
      from_subaccount: [],
      quote: [],
      mint_authorization: [],
    }])

    render(<BridgePage direction="deposit" onDirectionChange={vi.fn()} />, { wrapper: Wrapper })

    fireEvent.click(await screen.findByRole("button", { name: "Check status" }))

    expect(await screen.findByRole("button", { name: /Open transfer progress: Waiting for the Base transaction/ })).toBeVisible()
    expect(mocks.removeDepositIntent).not.toHaveBeenCalled()
  })

  it.each([
    { label: "with approval", allowance: 0n, expectedApprovals: 1, expectedRuntimeObservations: 2 },
    { label: "without approval", allowance: 300_010_000n, expectedApprovals: 0, expectedRuntimeObservations: 2 },
  ])("reuses one Oisy session $label, then closes it before authorization polling", async ({ allowance, expectedApprovals, expectedRuntimeObservations }) => {
    const closeWallet = vi.fn().mockResolvedValue(undefined)
    const account = { owner: "aaaaa-aa" }
    const adapter = {
      prepare: vi.fn().mockResolvedValue(closeWallet),
      getAccount: vi.fn().mockResolvedValue(account),
      approve: vi.fn().mockResolvedValue(7n),
      requestDeposit: vi.fn().mockResolvedValue({
        deposit_id: new Uint8Array(32).fill(7),
        owner_sequence: 3n,
        state: { EscrowedUnquoted: null },
      }),
    }
    mocks.useAccount.mockReturnValue({
      address: "0x0000000000000000000000000000000000000002",
      isConnected: true,
    })
    mocks.useIcWallet.mockReturnValue({
      account,
      provider: "oisy",
      adapter,
      connecting: undefined,
      connect: vi.fn(),
      disconnect: vi.fn(),
    })
    mocks.ledgerAllowance.mockResolvedValue({ allowance })

    render(<BridgePage direction="deposit" onDirectionChange={vi.fn()} />, { wrapper: Wrapper })
    await waitFor(() => expect(mocks.ledgerBalance).toHaveBeenCalled())
    fireEvent.change(screen.getByRole("textbox", { name: "You send" }), { target: { value: "2" } })
    fireEvent.click(screen.getByRole("button", { name: "Bridge to Base" }))

    expect(screen.getByRole("heading", { name: "Review bridge to Base" })).toBeVisible()
    expect(screen.getByText("Checking your wallets, balance, fees, and bridge availability…")).toBeVisible()
    expect(adapter.prepare).not.toHaveBeenCalled()
    fireEvent.click(await screen.findByRole("button", { name: "Continue to IC wallet" }))
    expect(adapter.prepare).toHaveBeenCalledOnce()

    await waitFor(() => {
      expect(adapter.requestDeposit).toHaveBeenCalledOnce()
      expect(closeWallet).toHaveBeenCalledOnce()
      expect(mocks.removeDepositIntent).toHaveBeenCalledOnce()
    })
    expect(adapter.approve).toHaveBeenCalledTimes(expectedApprovals)
    expect(mocks.baseRefetch).toHaveBeenCalledTimes(expectedRuntimeObservations)
    expect(closeWallet.mock.invocationCallOrder[0]).toBeLessThan(mocks.removeDepositIntent.mock.invocationCallOrder[0]!)
    expect(screen.queryByRole("button", { name: "Generating authorization…" })).not.toBeInTheDocument()
    expect(screen.getByText("The Deposit was accepted. Waiting for the Mint Authorization to become available.")).toBeVisible()
    expect(screen.getByText(/Complete the current deposit above/)).toBeVisible()
  })

  it("fails closed before saving or requesting a Deposit when the post-approval heartbeat fails", async () => {
    const account = { owner: "aaaaa-aa" }
    const adapter = {
      prepare: vi.fn().mockResolvedValue(vi.fn().mockResolvedValue(undefined)),
      getAccount: vi.fn().mockResolvedValue(account),
      approve: vi.fn().mockResolvedValue(7n),
      requestDeposit: vi.fn(),
    }
    mocks.useAccount.mockReturnValue({
      address: "0x0000000000000000000000000000000000000002",
      isConnected: true,
    })
    mocks.useIcWallet.mockReturnValue({
      account,
      provider: "oisy",
      adapter,
      connecting: undefined,
      connect: vi.fn(),
      disconnect: vi.fn(),
    })
    mocks.ledgerAllowance.mockResolvedValue({ allowance: 0n })
    mocks.baseRefetch
      .mockResolvedValueOnce({
        data: {
          ready: true,
          blockers: [],
          checkedAt: Date.now(),
          snapshot: {
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
            mintAuthorizationEpoch: 1n,
            blockTimestamp: 1_000n,
          },
        },
        isError: false,
        isStale: false,
      })
      .mockResolvedValueOnce({
        data: { ready: false, blockers: ["Runtime heartbeat failed"], checkedAt: Date.now() },
        isError: false,
        isStale: false,
      })

    render(<BridgePage direction="deposit" onDirectionChange={vi.fn()} />, { wrapper: Wrapper })
    await waitFor(() => expect(mocks.ledgerBalance).toHaveBeenCalled())
    fireEvent.change(screen.getByRole("textbox", { name: "You send" }), { target: { value: "2" } })
    fireEvent.click(screen.getByRole("button", { name: "Bridge to Base" }))
    fireEvent.click(await screen.findByRole("button", { name: "Continue to IC wallet" }))

    await waitFor(() => expect(adapter.approve).toHaveBeenCalledOnce())
    await waitFor(() => expect(mocks.baseRefetch).toHaveBeenCalledTimes(2))
    expect(adapter.requestDeposit).not.toHaveBeenCalled()
    expect(mocks.saveDepositIntent).not.toHaveBeenCalled()
  })

  it("closes Oisy when the Deposit response fails and restores retry recovery", async () => {
    const closeWallet = vi.fn().mockResolvedValue(undefined)
    const account = { owner: "aaaaa-aa" }
    const adapter = {
      prepare: vi.fn().mockResolvedValue(closeWallet),
      getAccount: vi.fn().mockResolvedValue(account),
      requestDeposit: vi.fn().mockRejectedValue(new Error("wallet timeout")),
    }
    mocks.useAccount.mockReturnValue({
      address: "0x0000000000000000000000000000000000000002",
      isConnected: true,
    })
    mocks.useIcWallet.mockReturnValue({
      account,
      provider: "oisy",
      adapter,
      connecting: undefined,
      connect: vi.fn(),
      disconnect: vi.fn(),
    })
    mocks.readDepositIntent.mockReturnValue({
      account,
      recipient: "0x0000000000000000000000000000000000000002",
      call: {
        ownerSequence: 3n,
        baseRecipient: new Uint8Array(20).fill(2),
        grossAmount: 200_000_000n,
        maxServiceFee: 50_000_000n,
      },
      state: "submitted",
    })

    render(<BridgePage direction="deposit" onDirectionChange={vi.fn()} />, { wrapper: Wrapper })
    fireEvent.click(await screen.findByRole("button", { name: "Retry same deposit" }))
    fireEvent.click(await screen.findByRole("button", { name: "Continue to IC wallet" }))

    await waitFor(() => expect(closeWallet).toHaveBeenCalledOnce())
    fireEvent.click(screen.getByRole("button", { name: "Close" }))
    expect(screen.getByRole("button", { name: "Retry same deposit" })).toBeEnabled()
  })

  it("keeps the accepted Deposit in the global progress dialog and allows minimizing", async () => {
    const account = { owner: "aaaaa-aa" }
    const adapter = {
      prepare: vi.fn().mockResolvedValue(vi.fn().mockResolvedValue(undefined)),
      getAccount: vi.fn().mockResolvedValue(account),
      approve: vi.fn().mockResolvedValue(7n),
      requestDeposit: vi.fn().mockResolvedValue({
        deposit_id: new Uint8Array(32).fill(7),
        owner_sequence: 3n,
        state: { EscrowedUnquoted: null },
      }),
    }
    mocks.useAccount.mockReturnValue({
      address: "0x0000000000000000000000000000000000000002",
      isConnected: true,
    })
    mocks.useIcWallet.mockReturnValue({
      account,
      provider: "oisy",
      adapter,
      connecting: undefined,
      connect: vi.fn(),
      disconnect: vi.fn(),
    })
    mocks.getDepositByOwnerSequence.mockResolvedValue([{
      state: { AuthorizationAvailable: null },
    }])

    render(<BridgePage direction="deposit" onDirectionChange={vi.fn()} />, { wrapper: Wrapper })
    await waitFor(() => expect(mocks.ledgerBalance).toHaveBeenCalled())
    fireEvent.change(screen.getByRole("textbox", { name: "You send" }), { target: { value: "2" } })
    fireEvent.click(screen.getByRole("button", { name: "Bridge to Base" }))
    fireEvent.click(await screen.findByRole("button", { name: "Continue to IC wallet" }))

    expect(await screen.findByRole("heading", { name: "Bridge to Base" })).toBeVisible()
    fireEvent.click(screen.getAllByRole("button", { name: "Minimize" })[0]!)
    expect(await screen.findByRole("button", { name: /Open transfer progress/ })).toBeVisible()
    expect(screen.queryByRole("button", { name: "Bridge to Base" })).not.toBeInTheDocument()
    expect(screen.getByRole("textbox", { name: "You send" })).toBeDisabled()
    expect(screen.getByRole("button", { name: "Reverse bridge direction" })).toBeDisabled()
    expect(await screen.findByText("Continue from the transfer progress window to confirm the Base mint transaction.")).toBeVisible()
  })

  it.each([
    ["Minted", { Minted: null }],
    ["Refunded", { Refunded: null }],
    ["Cancelled", { Cancelled: null }],
  ])("unlocks the form when the canonical Deposit becomes %s elsewhere", async (_label, state) => {
    const account = { owner: "aaaaa-aa" }
    const adapter = {
      prepare: vi.fn().mockResolvedValue(vi.fn().mockResolvedValue(undefined)),
      getAccount: vi.fn().mockResolvedValue(account),
      approve: vi.fn().mockResolvedValue(7n),
      requestDeposit: vi.fn().mockResolvedValue({
        deposit_id: new Uint8Array(32).fill(7),
        owner_sequence: 3n,
        state: { EscrowedUnquoted: null },
      }),
    }
    mocks.useAccount.mockReturnValue({
      address: "0x0000000000000000000000000000000000000002",
      isConnected: true,
    })
    mocks.useIcWallet.mockReturnValue({
      account,
      provider: "oisy",
      adapter,
      connecting: undefined,
      connect: vi.fn(),
      disconnect: vi.fn(),
    })
    mocks.getDepositByOwnerSequence.mockResolvedValue([{ state }])

    render(<BridgePage direction="deposit" onDirectionChange={vi.fn()} />, { wrapper: Wrapper })
    await waitFor(() => expect(mocks.ledgerBalance).toHaveBeenCalled())
    fireEvent.change(screen.getByRole("textbox", { name: "You send" }), { target: { value: "2" } })
    fireEvent.click(screen.getByRole("button", { name: "Bridge to Base" }))
    fireEvent.click(await screen.findByRole("button", { name: "Continue to IC wallet" }))

    await waitFor(() => expect(mocks.getDepositByOwnerSequence).toHaveBeenCalled())
    if ("Minted" in state) expect(await screen.findByRole("heading", { name: "Bridge to Base" })).toBeVisible()
    else expect(await screen.findByText("This transfer needs attention")).toBeVisible()
    fireEvent.click(screen.getByRole("button", { name: "Close" }))
    await waitFor(() => expect(screen.getByRole("textbox", { name: "You send" })).toBeEnabled())
    expect(screen.getByRole("button", { name: "Reverse bridge direction" })).toBeEnabled()
    expect(screen.getByRole("button", { name: "Bridge to Base" })).toBeVisible()
    expect(screen.getByRole<HTMLInputElement>("textbox", { name: "You send" }).value).toBe("")
    fireEvent.change(screen.getByRole("textbox", { name: "You send" }), { target: { value: "3" } })
    expect(screen.getByRole<HTMLInputElement>("textbox", { name: "You send" }).value).toBe("3")
    expect(screen.queryByRole("heading", { name: "Bridge complete" })).not.toBeInTheDocument()
  })
})
