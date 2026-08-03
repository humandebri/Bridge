import { StrictMode, useState, type ReactNode } from "react"
import { QueryClient, QueryClientProvider } from "@tanstack/react-query"
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react"
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
  runtimeRefetch: vi.fn(),
  runtimeAutoRetryPending: vi.fn<() => boolean>(),
  baseRefetch: vi.fn(),
  getDepositByOwnerSequence: vi.fn(),
  removeDepositIntent: vi.fn(),
  saveDepositIntent: vi.fn(),
  runtimeValidation: { value: { ready: true, blockers: [] as string[], checkedAt: 0 } },
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
vi.mock("@/features/bridge/mint-authorization-action", () => ({
  MintAuthorizationAction: ({ onMintConfirmed }: {
    onMintConfirmed?: (confirmation: {
      transactionHash: `0x${string}`
      recipient: `0x${string}`
      mintedAmount: bigint
    }) => void
  }) => <button type="button" onClick={() => onMintConfirmed?.({
    transactionHash: `0x${"22".repeat(32)}`,
    recipient: "0x0000000000000000000000000000000000000002",
    mintedAmount: 150_000_000n,
  })}>Complete test mint</button>,
}))
vi.mock("@/features/status/use-status", () => ({
  useRuntimeValidation: () => ({ data: mocks.runtimeValidation.value, isAutoRetryPending: mocks.runtimeAutoRetryPending(), isFetching: false, refetch: mocks.runtimeRefetch }),
  useRuntimeHeartbeat: () => ({
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
    isFetching: false,
    refetch: mocks.baseRefetch,
  }),
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

vi.mock("@/lib/browser-lock", () => ({
  withBrowserLock: (_name: string, action: () => unknown) => action(),
}))

vi.mock("@/lib/wallet-snapshot", () => ({
  currentInjectedWallet: vi.fn().mockResolvedValue({
    address: "0x0000000000000000000000000000000000000002",
    chainId: 84_532,
  }),
  requireWalletSnapshot: vi.fn(),
  sameIcAccount: vi.fn().mockReturnValue(true),
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
    mocks.runtimeAutoRetryPending.mockReset().mockReturnValue(false)
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

  it("retries only full runtime validation when it has not succeeded", async () => {
    mocks.runtimeValidation.value = { ready: false, blockers: ["Runtime validation failed"], checkedAt: Date.now() }
    mocks.runtimeWriteReadiness.mockReturnValue({ ready: false, reason: "Runtime validation failed" })
    render(<BridgePage direction="deposit" onDirectionChange={vi.fn()} />, { wrapper: Wrapper })

    fireEvent.click(screen.getByRole("button", { name: "Refresh" }))

    await waitFor(() => expect(mocks.runtimeRefetch).toHaveBeenCalledOnce())
    expect(mocks.baseRefetch).not.toHaveBeenCalled()
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

  it("keeps the fail-closed availability check in progress while the initial retry is pending", () => {
    mocks.runtimeWriteReadiness.mockReturnValue({ ready: false, reason: "Runtime validation failed" })
    mocks.runtimeAutoRetryPending.mockReturnValue(true)

    render(<BridgePage direction="deposit" onDirectionChange={vi.fn()} />, { wrapper: Wrapper })

    expect(screen.getByText("Checking availability…")).toBeVisible()
    expect(screen.queryByText("Bridge is temporarily unavailable. Try Refresh.")).not.toBeInTheDocument()
    expect(screen.getByRole("button", { name: "Refreshing…" })).toBeDisabled()
  })

  it("restores the unavailable warning and manual refresh after the initial retry finishes", () => {
    mocks.runtimeWriteReadiness.mockReturnValue({ ready: false, reason: "Runtime validation failed" })

    render(<BridgePage direction="deposit" onDirectionChange={vi.fn()} />, { wrapper: Wrapper })

    expect(screen.getByText("Bridge is temporarily unavailable. Try Refresh.")).toBeVisible()
    expect(screen.getByRole("button", { name: "Refresh" })).toBeEnabled()
  })

  it("runs withdrawal preflight checks before exposing irreversible confirmation", async () => {
    const account = { owner: "aaaaa-aa" }
    mocks.useAccount.mockReturnValue({
      address: "0x0000000000000000000000000000000000000002",
      isConnected: true,
    })
    mocks.useIcWallet.mockReturnValue({
      account,
      provider: "oisy",
      adapter: { getAccount: vi.fn().mockResolvedValue(account) },
      connecting: undefined,
      connect: vi.fn(),
      disconnect: vi.fn(),
    })

    render(<BridgePage direction="withdraw" onDirectionChange={vi.fn()} />, { wrapper: Wrapper })
    await waitFor(() => expect(mocks.bsnsBalance).toHaveBeenCalled())
    fireEvent.change(screen.getByRole("textbox", { name: "You send" }), { target: { value: "2" } })
    fireEvent.click(screen.getByRole("button", { name: "Bridge to IC" }))

    expect(screen.getByRole("heading", { name: "Review bridge to IC" })).toBeVisible()
    expect(await screen.findByText("All checks passed. Review the transfer before opening your wallet.")).toBeVisible()
    expect(screen.getByText("Wallets connected").closest("li")).toHaveAttribute("data-status", "passed")
    expect(screen.getByText("Transfer availability checked").closest("li")).toHaveAttribute("data-status", "passed")
    expect(screen.getByRole("button", { name: "Confirm and open wallet" })).toBeDisabled()
    fireEvent.click(screen.getByRole("checkbox", { name: "Acknowledge irreversible burn" }))
    expect(screen.getByRole("button", { name: "Confirm and open wallet" })).toBeEnabled()
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
      adapter: { getAccount: vi.fn().mockResolvedValue(account) },
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
    expect(await screen.findByText("All checks passed. Review the transfer before opening your wallet.")).toBeVisible()
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
      adapter: { getAccount: vi.fn().mockResolvedValue(account) },
      connecting: undefined,
      connect: vi.fn(),
      disconnect: vi.fn(),
    })

    render(<BridgePage direction="withdraw" onDirectionChange={vi.fn()} />, { wrapper: Wrapper })
    await waitFor(() => expect(mocks.bsnsBalance).toHaveBeenCalled())
    fireEvent.change(screen.getByRole("textbox", { name: "You send" }), { target: { value: "2" } })
    fireEvent.click(screen.getByRole("button", { name: "Bridge to IC" }))
    expect(screen.getByRole("heading", { name: "Review bridge to IC" })).toBeVisible()
    await waitFor(() => expect(screen.getByText("Wallets connected").closest("li")).toHaveAttribute("data-status", "passed"))
    expect(screen.getByText("Bridge configuration verified").closest("li")).toHaveAttribute("data-status", "checking")
    expect(screen.getByText("Balance and fees checked").closest("li")).toHaveAttribute("data-status", "waiting")
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
    expect(screen.getByRole("list", { name: "Preflight checks" })).toBeVisible()
    expect(prepare).not.toHaveBeenCalled()
    fireEvent.click(await screen.findByRole("button", { name: "Confirm and open wallet" }))
    expect(prepare).toHaveBeenCalledOnce()

    await waitFor(() => expect(requestDeposit).toHaveBeenCalledOnce())
    expect(screen.queryByText("Deposit status unavailable")).not.toBeInTheDocument()
    expect(screen.getByRole("button", { name: "Confirming deposit…" })).toBeDisabled()
    expect(screen.getByText(/window stays open while the bridge verifies Deposit acceptance/)).toBeVisible()
    expect(closeWallet).not.toHaveBeenCalled()
  })

  it.each([
    { label: "with approval", allowance: 0n, expectedApprovals: 1, expectedRuntimeObservations: 2 },
    { label: "without approval", allowance: 300_010_000n, expectedApprovals: 0, expectedRuntimeObservations: 1 },
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
    expect(screen.getByRole("list", { name: "Preflight checks" })).toBeVisible()
    expect(adapter.prepare).not.toHaveBeenCalled()
    fireEvent.click(await screen.findByRole("button", { name: "Confirm and open wallet" }))
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
    fireEvent.click(await screen.findByRole("button", { name: "Confirm and open wallet" }))

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
    fireEvent.click(await screen.findByRole("button", { name: "Confirm and open wallet" }))

    await waitFor(() => expect(closeWallet).toHaveBeenCalledOnce())
    expect(screen.getByRole("button", { name: "Retry same deposit" })).toBeEnabled()
  })

  it("locks the active Deposit flow and resets the form behind the completion dialog", async () => {
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
    fireEvent.click(await screen.findByRole("button", { name: "Confirm and open wallet" }))

    expect(await screen.findByRole("button", { name: "Complete test mint" })).toBeVisible()
    expect(screen.queryByRole("button", { name: "Bridge to Base" })).not.toBeInTheDocument()
    expect(screen.getByRole("textbox", { name: "You send" })).toBeDisabled()
    expect(screen.getByRole("button", { name: "Reverse bridge direction" })).toBeDisabled()

    fireEvent.click(screen.getByRole("button", { name: "Complete test mint" }))

    expect(await screen.findByRole("heading", { name: "Bridge complete" })).toBeVisible()
    expect(screen.getByText("1.5 KINIC")).toBeVisible()
    expect((document.getElementById("bridge-amount") as HTMLInputElement).value).toBe("")
    fireEvent.click(screen.getByRole("button", { name: "Close" }))

    await waitFor(() => expect(screen.queryByRole("heading", { name: "Bridge complete" })).not.toBeInTheDocument())
    expect(screen.getByRole("button", { name: "Bridge to Base" })).toBeDisabled()
    expect(screen.getByRole("textbox", { name: "You send" })).toBeEnabled()
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
    fireEvent.click(await screen.findByRole("button", { name: "Confirm and open wallet" }))

    await waitFor(() => expect(mocks.getDepositByOwnerSequence).toHaveBeenCalled())
    await waitFor(() => expect(screen.getByRole("textbox", { name: "You send" })).toBeEnabled())
    expect(screen.getByRole("button", { name: "Reverse bridge direction" })).toBeEnabled()
    expect(screen.getByRole("button", { name: "Bridge to Base" })).toBeVisible()
    expect(screen.getByRole<HTMLInputElement>("textbox", { name: "You send" }).value).toBe("")
    fireEvent.change(screen.getByRole("textbox", { name: "You send" }), { target: { value: "3" } })
    expect(screen.getByRole<HTMLInputElement>("textbox", { name: "You send" }).value).toBe("3")
    expect(screen.queryByRole("heading", { name: "Bridge complete" })).not.toBeInTheDocument()
  })
})
