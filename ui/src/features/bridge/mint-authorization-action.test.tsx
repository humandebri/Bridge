import { releaseFinalizedMintAttempt } from "@/lib/mint-execution"
import { clearMintObservations } from "@/lib/mint-observation"
import { QueryClient, QueryClientProvider } from "@tanstack/react-query"
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react"
import type { ReactNode } from "react"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"
import type { DepositView } from "@/generated/bridge.did"
import { deploymentProfile } from "@/config/profile"
import { MintAuthorizationAction } from "./mint-authorization-action"
import { DepositActivityRow } from "@/routes/history"

const mocks = vi.hoisted(() => ({
  rememberMintRecovery: vi.fn(),
  wasMintRequested: vi.fn(),
  getTransactionReceipt: vi.fn(),
  notifyMint: vi.fn(),
  getBlock: vi.fn(),
  simulateContract: vi.fn().mockResolvedValue({}),
  readPendingMint: vi.fn(),
  removePendingMint: vi.fn(),
  validateMintAuthorization: vi.fn(),
  runtimeRefetch: vi.fn(),
  refetchRuntimeWriteReady: vi.fn(),
  runtimeWriteBlocker: vi.fn(),
  withBrowserLock: vi.fn(),
  writeContractAsync: vi.fn(),
  savePendingMint: vi.fn(),
  useAccount: vi.fn(),
  exactMintReceiptFinalization: vi.fn(),
  receiptMatches: vi.fn(),
  toastSuccess: vi.fn(),
  toastError: vi.fn(),
  toastWarning: vi.fn(),
  heartbeatAgeMs: { value: 0 },
  heartbeatTimestamp: { value: 1_000n },
  latestTimestamp: { value: 1_000n },
  latestUnavailable: { value: false },
  authorizationDeadline: { value: 2_000n },
}))

vi.mock("@/lib/mint-recovery", () => ({
  rememberMintRecovery: mocks.rememberMintRecovery,
  wasMintRequested: mocks.wasMintRequested,
}))
vi.mock("@/lib/ic-history-owner", () => ({ loadIcHistoryOwner: () => undefined }))

vi.mock("@/lib/base-transaction-observation", () => ({
  readBaseReceipt: (hash: string) => mocks.getTransactionReceipt({ hash }),
  readBaseBlock: (block: bigint | string) =>
    mocks.getBlock(typeof block === "bigint" ? { blockNumber: block } : { blockTag: block }),
  sharedBaseRead: (_key: string, read: () => Promise<unknown>) => read(),
}))
vi.mock("@/lib/ic/bridge", () => ({
  createBridgeActor: async () => ({
    notify_deposit_mint: mocks.notifyMint,
  }),
}))
vi.mock("@/lib/ic/withdrawal-notification-client", () => ({
  getWithdrawalNotificationIdentity: async () => ({}),
}))

vi.mock("wagmi", () => ({
  useAccount: mocks.useAccount,
  useChainId: () => deploymentProfile.chainId,
  useWriteContract: () => ({ isPending: false, writeContractAsync: mocks.writeContractAsync }),
}))

vi.mock("@/features/status/use-status", () => ({
  useRuntimeValidation: () => ({
    data: { ready: true, blockers: [], checkedAt: Date.now() },
    refetch: mocks.runtimeRefetch,
  }),
  useRuntimeHeartbeat: () => ({
    data: {
      ready: true,
      blockers: [],
      checkedAt: Date.now() - mocks.heartbeatAgeMs.value,
      snapshot: {
        blockTimestamp: mocks.heartbeatTimestamp.value,
        bridgeSigner: "0x0303030303030303030303030303030303030303",
        mintAuthorizationEpoch: 1n,
        depositsPaused: false,
      },
    },
    refetch: mocks.runtimeRefetch,
    isError: false,
    isStale: false,
  }),
  useFinalizedBaseClock: () => ({
    data: { timestamp: mocks.heartbeatTimestamp.value },
    dataUpdatedAt: Date.now() - mocks.heartbeatAgeMs.value,
    isError: false,
    isStale: false,
  }),
  useLatestBaseClock: () => ({
    data: mocks.latestUnavailable.value ? undefined : { timestamp: mocks.latestTimestamp.value },
    dataUpdatedAt: Date.now() - mocks.heartbeatAgeMs.value,
    isError: mocks.latestUnavailable.value,
    isStale: false,
  }),
}))

vi.mock("@/lib/runtime-validation", () => ({
  refetchRuntimeAttestedWriteReady: mocks.refetchRuntimeWriteReady,
  refetchRuntimeWriteReady: mocks.refetchRuntimeWriteReady,
  runtimeWriteBlocker: mocks.runtimeWriteBlocker,
}))

vi.mock("@/lib/evm/client", () => ({
  baseTransactionExplorerUrl: () => undefined,
  basePublicClient: {
    getBlock: mocks.getBlock,
    simulateContract: mocks.simulateContract,
    getTransactionReceipt: mocks.getTransactionReceipt,
  },
}))

vi.mock("@/lib/mint-preflight", () => ({
  mintWalletConnected: () => true,
  prepareMint: async () => {
    const observation = await mocks.refetchRuntimeWriteReady()
    const validated = await mocks.validateMintAuthorization(record, observation)
    await mocks.simulateContract()
    return { validated, observedAt: performance.now() }
  },
  checkMintDeadline: () => {},
}))

vi.mock("@/lib/mint-authorization", () => ({
  contractAuthorization: () => ({
    depositId: `0x${"11".repeat(32)}`,
    recipient: "0x0303030303030303030303030303030303030303",
    grossAmount: 500_000_000n,
    maxServiceFee: 50_000_000n,
    chargedServiceFee: 50_000_000n,
    deadline: mocks.authorizationDeadline.value,
    authorizationEpoch: 1n,
  }),
  validateMintAuthorization: mocks.validateMintAuthorization,
}))

vi.mock("@/lib/deposit-mint-finalization", () => ({
  exactMintReceiptFinalization: mocks.exactMintReceiptFinalization,
  receiptContainsExactDepositMint: mocks.receiptMatches,
}))

vi.mock("@/lib/pending-confirmations", () => ({
  readPendingMint: mocks.readPendingMint,
  removePendingMint: mocks.removePendingMint,
  savePendingMint: mocks.savePendingMint,
}))

vi.mock("@/lib/browser-lock", () => ({
  browserLocalStorage: () => window.localStorage,
  withBrowserLock: mocks.withBrowserLock,
}))

vi.mock("sonner", () => ({
  toast: { error: mocks.toastError, success: mocks.toastSuccess, warning: mocks.toastWarning },
}))

const pendingHash = `0x${"22".repeat(32)}`
const finalizedBlockHash = `0x${"aa".repeat(32)}`
const originalBridgeAddress = deploymentProfile.bridgeAddress
const pendingExpectation = {
  depositId: `0x${"11".repeat(32)}`,
  authorizationDigest: `0x${"11".repeat(32)}`,
  recipient: "0x0303030303030303030303030303030303030303",
  grossAmount: "500000000",
  chargedServiceFee: "50000000",
  mintedAmount: "450000000",
}
const pendingMint = { ...pendingExpectation, transactionHash: pendingHash }
const record = {
  state: { AuthorizationAvailable: null },
  mint_authorization: [
    {
      deadline: 2_000n,
      recipient: Array(20).fill(3),
      digest: Array(32).fill(0x11),
    },
  ],
} as unknown as DepositView

function Wrapper({ children }: { children: ReactNode }) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  return <QueryClientProvider client={client}>{children}</QueryClientProvider>
}

describe("MintAuthorizationAction pending retry", () => {
  afterEach(() => {
    deploymentProfile.bridgeAddress = originalBridgeAddress
    cleanup()
  })

  beforeEach(() => {
    mocks.rememberMintRecovery.mockReset().mockResolvedValue(true)
    mocks.wasMintRequested.mockReset().mockReturnValue(false)
    clearMintObservations()
    localStorage.clear()
    releaseFinalizedMintAttempt(
      [
        deploymentProfile.chainId,
        "0x1111111111111111111111111111111111111111",
        deploymentProfile.bridgeCanisterId,
        deploymentProfile.deploymentInstanceId,
        pendingExpectation.depositId,
        pendingExpectation.authorizationDigest,
      ]
        .join(":")
        .toLowerCase(),
    )
    Object.defineProperty(navigator, "locks", {
      configurable: true,
      value: { request: vi.fn(async (_name, _options, action) => action({ name: "mint" })) },
    })
    mocks.notifyMint
      .mockReset()
      .mockResolvedValue({ Ok: { Recorded: { deposit_id: new Uint8Array(32).fill(0x11) } } })
    deploymentProfile.bridgeAddress = "0x1111111111111111111111111111111111111111"
    mocks.getBlock
      .mockReset()
      .mockImplementation(
        ({ blockTag, blockNumber }: { blockTag?: string; blockNumber?: bigint }) =>
          Promise.resolve({
            number: blockTag === "finalized" ? 100n : blockNumber,
            hash: finalizedBlockHash,
            timestamp: 1_000n,
          }),
      )
    mocks.getTransactionReceipt.mockReset().mockRejectedValue(new Error("not found"))
    mocks.readPendingMint.mockReset().mockReturnValue(pendingMint)
    mocks.removePendingMint.mockReset().mockImplementation(async () => {
      mocks.readPendingMint.mockReturnValue(undefined)
    })
    mocks.validateMintAuthorization.mockReset().mockResolvedValue({
      authorization: { depositId: `0x${"11".repeat(32)}` },
      digest: `0x${"11".repeat(32)}`,
      signature: "0xsigned",
      recipient: "0x0303030303030303030303030303030303030303",
    })
    mocks.runtimeRefetch.mockReset()
    mocks.runtimeWriteBlocker.mockReset().mockReturnValue(undefined)
    mocks.withBrowserLock
      .mockReset()
      .mockImplementation((_name: string, action: () => unknown) => action())
    mocks.refetchRuntimeWriteReady.mockReset().mockResolvedValue({
      ready: true,
      blockers: [],
      checkedAt: Date.now(),
      snapshot: {
        blockTimestamp: 1_000n,
        bridgeSigner: "0x0303030303030303030303030303030303030303",
        mintAuthorizationEpoch: 1n,
        depositsPaused: false,
      },
    })
    mocks.writeContractAsync.mockReset().mockResolvedValue(pendingHash)
    mocks.receiptMatches.mockReset().mockReturnValue(true)
    mocks.exactMintReceiptFinalization.mockReset().mockReturnValue("finalized")
    mocks.savePendingMint.mockReset().mockResolvedValue(undefined)
    mocks.toastSuccess.mockReset()
    mocks.toastError.mockReset()
    mocks.toastWarning.mockReset()
    mocks.heartbeatAgeMs.value = 0
    mocks.heartbeatTimestamp.value = 1_000n
    mocks.latestTimestamp.value = 1_000n
    mocks.latestUnavailable.value = false
    mocks.authorizationDeadline.value = 2_000n
    mocks.useAccount.mockReset().mockReturnValue({
      address: "0x0000000000000000000000000000000000000001",
    })
  })

  it("shares_preflight_between_modal_and_history_without_duplicate_wallet_requests", async () => {
    mocks.readPendingMint.mockReturnValue(undefined)
    mocks.useAccount.mockReturnValue({ address: "0x0303030303030303030303030303030303030303" })
    let resolvePreflight!: (value: unknown) => void
    mocks.refetchRuntimeWriteReady.mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          resolvePreflight = resolve
        }),
    )
    const view = render(
      <>
        <MintAuthorizationAction record={record} headless autoPromptOwner="shared-owner" />
        <MintAuthorizationAction record={record} compact />
      </>,
      { wrapper: Wrapper },
    )
    await waitFor(() => expect(mocks.refetchRuntimeWriteReady).toHaveBeenCalledOnce())
    expect(screen.getByRole("button", { name: "Checking IC state…" })).toBeDisabled()
    view.rerender(<MintAuthorizationAction record={record} compact />)
    resolvePreflight({ ready: true })
    await waitFor(() => expect(mocks.writeContractAsync).toHaveBeenCalledOnce())
    expect(mocks.refetchRuntimeWriteReady).toHaveBeenCalledOnce()
  })

  it("keeps an in-flight receipt read when the same authorization is polled again", async () => {
    let resolveReceipt!: (receipt: unknown) => void
    mocks.getTransactionReceipt.mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          resolveReceipt = resolve
        }),
    )
    const onProgress = vi.fn()
    const view = render(<MintAuthorizationAction record={record} onProgress={onProgress} />, {
      wrapper: Wrapper,
    })
    await waitFor(() => expect(mocks.getTransactionReceipt).toHaveBeenCalledOnce())
    view.rerender(
      <MintAuthorizationAction record={structuredClone(record)} onProgress={onProgress} />,
    )
    resolveReceipt({
      status: "success",
      blockNumber: 101n,
      blockHash: finalizedBlockHash,
      logs: [],
    })
    await waitFor(() =>
      expect(onProgress).toHaveBeenCalledWith({
        phase: "included",
        transactionHash: pendingHash,
        blockNumber: 101n,
        outcome: "success",
      }),
    )
    expect(mocks.getTransactionReceipt).toHaveBeenCalledOnce()
    expect(mocks.writeContractAsync).not.toHaveBeenCalled()
  })

  it("does not clear a transaction hash when its receipt is not found", async () => {
    render(<MintAuthorizationAction record={record} />, { wrapper: Wrapper })

    await waitFor(() =>
      expect(mocks.getTransactionReceipt).toHaveBeenCalledWith({ hash: pendingHash }),
    )
    expect(mocks.removePendingMint).not.toHaveBeenCalled()
    expect(screen.queryByText(/Authorization valid for/)).not.toBeInTheDocument()
    expect(screen.getByText(/Waiting for inclusion\./)).toBeInTheDocument()
    await waitFor(() => expect(screen.getByText("Review saved transaction")).toBeEnabled())
  })

  it("restores_a_saved_mint_without_repeating_the_Base_write", async () => {
    const first = render(<MintAuthorizationAction record={record} />, { wrapper: Wrapper })
    await waitFor(() => expect(mocks.getTransactionReceipt).toHaveBeenCalledOnce())

    first.unmount()
    render(<MintAuthorizationAction record={record} />, { wrapper: Wrapper })

    await waitFor(() => expect(mocks.getTransactionReceipt).toHaveBeenCalledTimes(2))
    expect(mocks.writeContractAsync).not.toHaveBeenCalled()
    expect(mocks.savePendingMint).not.toHaveBeenCalled()
    expect(screen.getByText(/Waiting for inclusion\./)).toBeInTheDocument()
  })

  it("recovers_a_saved_mint_after_a_temporary_Base_query_failure", async () => {
    const onMintConfirmed = vi.fn()
    const first = render(
      <MintAuthorizationAction record={record} onMintConfirmed={onMintConfirmed} />,
      { wrapper: Wrapper },
    )
    await waitFor(() => expect(mocks.getTransactionReceipt).toHaveBeenCalledOnce())
    expect(onMintConfirmed).not.toHaveBeenCalled()

    first.unmount()
    mocks.getTransactionReceipt.mockResolvedValue({
      status: "success",
      blockNumber: 99n,
      blockHash: finalizedBlockHash,
      logs: [],
    })
    render(<MintAuthorizationAction record={record} onMintConfirmed={onMintConfirmed} />, {
      wrapper: Wrapper,
    })

    await waitFor(() => expect(mocks.exactMintReceiptFinalization).toHaveBeenCalled())
    fireEvent(document, new Event("visibilitychange"))
    await waitFor(() => expect(onMintConfirmed).toHaveBeenCalledOnce())
    expect(mocks.writeContractAsync).not.toHaveBeenCalled()
    expect(mocks.removePendingMint).not.toHaveBeenCalled()
  })

  it("keeps retry recovery out of the compact History action cell", async () => {
    render(<MintAuthorizationAction record={record} compact />, { wrapper: Wrapper })

    expect(
      await screen.findByText("Submitted on Base; refreshing transaction status."),
    ).toBeInTheDocument()
    expect(screen.queryByText("Review saved transaction")).not.toBeInTheDocument()
    expect(screen.getByRole("button", { name: "Copy mint diagnostics" })).toBeEnabled()
  })

  it("keeps the hash when Base revalidation fails", async () => {
    mocks.validateMintAuthorization.mockRejectedValue(
      new Error("Mint authorization is no longer valid on Base"),
    )
    render(<MintAuthorizationAction record={record} />, { wrapper: Wrapper })

    fireEvent.click(await screen.findByText("Review saved transaction"))

    await waitFor(() =>
      expect(mocks.validateMintAuthorization).toHaveBeenCalledWith(record, expect.any(Object)),
    )
    expect(mocks.removePendingMint).not.toHaveBeenCalled()
    expect(screen.queryByText("Clear the saved transaction reference?")).not.toBeInTheDocument()
  })

  it("does not treat a successful receipt without the exact mint event as current", async () => {
    const onMintConfirmed = vi.fn()
    mocks.getTransactionReceipt.mockResolvedValue({
      status: "success",
      blockNumber: 99n,
      blockHash: finalizedBlockHash,
      logs: [],
    })
    mocks.receiptMatches.mockReturnValue(false)
    mocks.exactMintReceiptFinalization.mockReturnValue("conflict")
    render(<MintAuthorizationAction record={record} onMintConfirmed={onMintConfirmed} />, {
      wrapper: Wrapper,
    })

    expect(
      await screen.findByText("Deposit identity conflict. Do not submit another transaction."),
    ).toBeInTheDocument()
    expect(screen.queryByText("Minted on Base")).not.toBeInTheDocument()
    expect(mocks.removePendingMint).not.toHaveBeenCalled()
    expect(onMintConfirmed).not.toHaveBeenCalled()
  })

  it("keeps_a_mined_transaction_pending_until_its_block_is_finalized", async () => {
    const onMintConfirmed = vi.fn()
    const readBlock = mocks.getBlock.getMockImplementation()!
    mocks.getBlock.mockImplementation((args: { blockTag?: string; blockNumber?: bigint }) =>
      args.blockTag === "finalized" ? new Promise(() => undefined) : readBlock(args),
    )
    const onProgress = vi.fn()
    mocks.getTransactionReceipt.mockResolvedValue({
      status: "success",
      blockNumber: 101n,
      blockHash: finalizedBlockHash,
      logs: [],
    })

    const view = render(
      <MintAuthorizationAction
        record={record}
        onMintConfirmed={onMintConfirmed}
        onProgress={onProgress}
      />,
      { wrapper: Wrapper },
    )

    await waitFor(() => expect(mocks.getTransactionReceipt).toHaveBeenCalled())
    expect(mocks.exactMintReceiptFinalization).not.toHaveBeenCalled()
    expect(mocks.removePendingMint).not.toHaveBeenCalled()
    expect(onMintConfirmed).not.toHaveBeenCalled()
    expect(onProgress).toHaveBeenCalledWith({
      phase: "included",
      transactionHash: pendingHash,
      blockNumber: 101n,
      outcome: "success",
    })
    expect(screen.getByText("Success")).toBeInTheDocument()
    expect(screen.queryByText("Review saved transaction")).not.toBeInTheDocument()

    mocks.getTransactionReceipt.mockRejectedValue(new Error("RPC unavailable"))
    view.rerender(
      <MintAuthorizationAction
        record={record}
        onMintConfirmed={onMintConfirmed}
        onProgress={vi.fn()}
      />,
    )
    await waitFor(() => expect(mocks.getTransactionReceipt).toHaveBeenCalledTimes(2))
    expect(screen.getByText("Success")).toBeInTheDocument()
    expect(screen.queryByRole("button", { name: "Mint on Base" })).not.toBeInTheDocument()
    expect(mocks.writeContractAsync).not.toHaveBeenCalled()

    const missing = new Error("Receipt missing")
    missing.name = "TransactionReceiptNotFoundError"
    mocks.getTransactionReceipt.mockRejectedValue(missing)
    const correctedProgress = vi.fn()
    view.rerender(<MintAuthorizationAction record={record} onProgress={correctedProgress} />)
    await waitFor(() =>
      expect(correctedProgress).toHaveBeenCalledWith({
        phase: "submitted",
        transactionHash: pendingHash,
      }),
    )
    expect(screen.queryByText("Success")).not.toBeInTheDocument()
    expect(mocks.writeContractAsync).not.toHaveBeenCalled()
  })

  it("does not offer a retry for a reverted receipt before its block is finalized", async () => {
    const onProgress = vi.fn()
    mocks.getTransactionReceipt.mockResolvedValue({
      status: "reverted",
      blockNumber: 101n,
      blockHash: finalizedBlockHash,
      logs: [],
    })

    render(<MintAuthorizationAction record={record} onProgress={onProgress} />, {
      wrapper: Wrapper,
    })

    expect(await screen.findByText("Transaction reverted")).toBeInTheDocument()
    expect(screen.queryByText("Review saved transaction")).not.toBeInTheDocument()
    expect(mocks.removePendingMint).not.toHaveBeenCalled()
    expect(onProgress).toHaveBeenCalledWith({
      phase: "included",
      transactionHash: pendingHash,
      blockNumber: 101n,
      outcome: "reverted",
    })
  })

  it("offers_an_explicit_retry_only_after_the_reverted_receipt_is_finalized", async () => {
    const onMintConfirmed = vi.fn()
    const onProgress = vi.fn()
    mocks.getTransactionReceipt.mockResolvedValue({
      status: "reverted",
      blockNumber: 99n,
      blockHash: finalizedBlockHash,
      logs: [],
    })
    mocks.exactMintReceiptFinalization.mockReturnValue("reverted")

    render(
      <MintAuthorizationAction
        record={record}
        onMintConfirmed={onMintConfirmed}
        onProgress={onProgress}
      />,
      { wrapper: Wrapper },
    )

    await waitFor(() => expect(mocks.exactMintReceiptFinalization).toHaveBeenCalled())
    fireEvent(document, new Event("visibilitychange"))
    expect(await screen.findByRole("button", { name: "Review saved transaction" })).toBeEnabled()
    expect(mocks.removePendingMint).not.toHaveBeenCalled()
    expect(onMintConfirmed).not.toHaveBeenCalled()
    expect(mocks.writeContractAsync).not.toHaveBeenCalled()
    expect(onProgress).toHaveBeenCalledWith(
      expect.objectContaining({
        phase: "attention",
        transactionHash: pendingHash,
      }),
    )
    fireEvent.click(screen.getByRole("button", { name: "Review saved transaction" }))
    fireEvent.click(await screen.findByRole("button", { name: "Clear saved transaction" }))
    await waitFor(() => expect(mocks.removePendingMint).toHaveBeenCalledWith(pendingExpectation))
    expect(await screen.findByRole("button", { name: "Mint on Base" })).toBeEnabled()
  })

  it("allows_refund_after_a_finalized_revert_when_mint_authorization_has_expired", async () => {
    mocks.heartbeatTimestamp.value = 2_001n
    mocks.latestTimestamp.value = 2_001n
    mocks.validateMintAuthorization.mockRejectedValue(new Error("Mint authorization expired"))
    mocks.refetchRuntimeWriteReady.mockRejectedValue(new Error("Minting is paused"))
    mocks.getTransactionReceipt.mockResolvedValue({
      status: "reverted",
      blockNumber: 99n,
      blockHash: finalizedBlockHash,
      logs: [],
    })
    mocks.exactMintReceiptFinalization.mockReturnValue("reverted")
    const onRequestRefund = vi.fn()

    render(<MintAuthorizationAction record={record} compact onRequestRefund={onRequestRefund} />, {
      wrapper: Wrapper,
    })

    await waitFor(() => expect(mocks.exactMintReceiptFinalization).toHaveBeenCalled())
    fireEvent(document, new Event("visibilitychange"))
    fireEvent.click(await screen.findByRole("button", { name: "Review saved transaction" }))
    fireEvent.click(await screen.findByRole("button", { name: "Clear saved transaction" }))
    fireEvent.click(await screen.findByRole("button", { name: "Claim refund" }))

    expect(mocks.removePendingMint).toHaveBeenCalledWith(pendingExpectation)
    expect(onRequestRefund).toHaveBeenCalledOnce()
    expect(mocks.refetchRuntimeWriteReady).not.toHaveBeenCalled()
    expect(mocks.validateMintAuthorization).not.toHaveBeenCalled()
    expect(mocks.writeContractAsync).not.toHaveBeenCalled()
    expect(screen.queryByRole("button", { name: "Mint on Base" })).not.toBeInTheDocument()
  })

  it("reports a saved transaction exactly once after its exact receipt is confirmed", async () => {
    const onMintConfirmed = vi.fn()
    const onProgress = vi.fn()
    mocks.notifyMint.mockImplementation(() => new Promise(() => undefined))
    mocks.getTransactionReceipt.mockResolvedValue({
      status: "success",
      blockNumber: 99n,
      blockHash: finalizedBlockHash,
      logs: [],
    })

    render(
      <MintAuthorizationAction
        record={record}
        onMintConfirmed={onMintConfirmed}
        onProgress={onProgress}
      />,
      {
        wrapper: Wrapper,
      },
    )

    await waitFor(() => expect(mocks.exactMintReceiptFinalization).toHaveBeenCalled())
    fireEvent(document, new Event("visibilitychange"))
    await waitFor(() => expect(onMintConfirmed).toHaveBeenCalledOnce())
    expect(onMintConfirmed).toHaveBeenCalledWith({
      transactionHash: pendingHash,
      recipient: pendingExpectation.recipient,
      mintedAmount: 450_000_000n,
    })
    expect(mocks.toastSuccess).not.toHaveBeenCalled()
    const readBlock = mocks.getBlock.getMockImplementation()!
    mocks.getBlock.mockImplementation((args: { blockTag?: string; blockNumber?: bigint }) =>
      args.blockNumber === 99n
        ? Promise.resolve({ number: 99n, hash: `0x${"bb".repeat(32)}`, timestamp: 1_000n })
        : readBlock(args),
    )
    const previousChecks = mocks.exactMintReceiptFinalization.mock.calls.length
    fireEvent(document, new Event("visibilitychange"))
    await waitFor(() =>
      expect(mocks.exactMintReceiptFinalization.mock.calls.length).toBeGreaterThan(previousChecks),
    )
    mocks.getTransactionReceipt.mockRejectedValue(new Error("RPC unavailable after reorg"))
    fireEvent(document, new Event("visibilitychange"))
    await waitFor(() =>
      expect(onProgress).toHaveBeenCalledWith({ phase: "submitted", transactionHash: pendingHash }),
    )
    expect(screen.queryByText("Success")).not.toBeInTheDocument()
    expect(mocks.writeContractAsync).not.toHaveBeenCalled()
  })

  it("clears the hash only after successful revalidation and explicit confirmation", async () => {
    render(<MintAuthorizationAction record={record} />, { wrapper: Wrapper })
    fireEvent.click(await screen.findByText("Review saved transaction"))

    expect(await screen.findByText("Clear the saved transaction reference?")).toBeInTheDocument()
    expect(screen.getByText(/original transaction is mined later/)).toBeInTheDocument()
    fireEvent.click(screen.getByText("Cancel"))
    await waitFor(() =>
      expect(screen.queryByText("Clear the saved transaction reference?")).not.toBeInTheDocument(),
    )
    expect(mocks.removePendingMint).not.toHaveBeenCalled()

    fireEvent.click(screen.getByText("Review saved transaction"))
    fireEvent.click(await screen.findByText("Clear saved transaction"))
    await waitFor(() => expect(mocks.removePendingMint).toHaveBeenCalledWith(pendingExpectation))
    await waitFor(() => expect(screen.getByText("Mint on Base")).toBeEnabled())
  })

  it("opens the Base wallet once and does not request an IC success confirmation", async () => {
    mocks.readPendingMint.mockReturnValue(undefined)
    mocks.savePendingMint.mockRejectedValue(new Error("browser storage unavailable"))
    mocks.getTransactionReceipt.mockResolvedValue({
      status: "success",
      blockNumber: 99n,
      blockHash: finalizedBlockHash,
      logs: [],
    })
    mocks.useAccount.mockReturnValue({
      address: "0x0303030303030303030303030303030303030303",
    })
    const view = render(<MintAuthorizationAction record={record} autoPromptOwner="aaaaa-aa" />, {
      wrapper: Wrapper,
    })

    await waitFor(() => expect(mocks.writeContractAsync).toHaveBeenCalledOnce())
    expect(mocks.refetchRuntimeWriteReady).toHaveBeenCalledOnce()
    expect(mocks.savePendingMint).toHaveBeenCalledWith(
      expect.objectContaining({ transactionHash: pendingHash }),
    )
    expect(mocks.toastError).not.toHaveBeenCalled()
    expect(await screen.findByText("Success")).toBeInTheDocument()
    expect(screen.queryByText("Confirm mint on IC")).not.toBeInTheDocument()
    expect(mocks.writeContractAsync).toHaveBeenCalledWith(
      expect.objectContaining({
        functionName: "mintDepositWithAuthorization",
      }),
    )

    view.unmount()
    render(<MintAuthorizationAction record={record} autoPromptOwner="aaaaa-aa" />, {
      wrapper: Wrapper,
    })
    expect(mocks.writeContractAsync).toHaveBeenCalledOnce()
  })

  it("does_not_create_a_mint_after_the_automatic_wallet_prompt_is_rejected", async () => {
    mocks.readPendingMint.mockReturnValue(undefined)
    mocks.useAccount.mockReturnValue({
      address: "0x0303030303030303030303030303030303030303",
    })
    mocks.writeContractAsync.mockRejectedValue(
      Object.assign(new Error("User rejected the request"), { code: 4001 }),
    )

    const first = render(
      <MintAuthorizationAction record={record} autoPromptOwner="rejected-owner" />,
      { wrapper: Wrapper },
    )
    await waitFor(() => expect(mocks.writeContractAsync).toHaveBeenCalledOnce())
    await screen.findByText("Wallet request rejected. You can retry manually.")
    expect(mocks.savePendingMint).not.toHaveBeenCalled()

    first.unmount()
    render(<MintAuthorizationAction record={record} autoPromptOwner="rejected-owner" />, {
      wrapper: Wrapper,
    })
    await Promise.resolve()
    expect(mocks.writeContractAsync).toHaveBeenCalledOnce()
    expect(screen.getByRole("button", { name: "Retry mint" })).toBeEnabled()
  })

  it("refreshes the dynamic heartbeat before every mint write", async () => {
    mocks.readPendingMint.mockReturnValue(undefined)
    mocks.useAccount.mockReturnValue({
      address: "0x0303030303030303030303030303030303030303",
    })
    mocks.runtimeWriteBlocker.mockReturnValue("Runtime verification expired")
    render(<MintAuthorizationAction record={record} />, { wrapper: Wrapper })

    fireEvent.click(await screen.findByRole("button", { name: "Mint on Base" }))

    await waitFor(() => expect(mocks.refetchRuntimeWriteReady).toHaveBeenCalledOnce())
    await waitFor(() => expect(mocks.writeContractAsync).toHaveBeenCalledOnce())
  })

  it("revalidates_the_authorization_only_after_acquiring_the_wallet_prompt_lock", async () => {
    mocks.readPendingMint.mockReturnValue(undefined)
    vi.mocked(navigator.locks.request).mockImplementation((async (
      _name: string,
      _options: unknown,
      action: (lock: null) => Promise<void>,
    ) => action(null)) as typeof navigator.locks.request)
    render(<MintAuthorizationAction record={record} />, { wrapper: Wrapper })
    fireEvent.click(await screen.findByRole("button", { name: "Mint on Base" }))
    await screen.findByText("Another wallet operation is in progress. Retry after it finishes.")
    expect(mocks.refetchRuntimeWriteReady).not.toHaveBeenCalled()
    expect(mocks.validateMintAuthorization).not.toHaveBeenCalled()
    expect(mocks.writeContractAsync).not.toHaveBeenCalled()
  })

  it("reports a directly submitted mint without also showing a success toast", async () => {
    const onMintConfirmed = vi.fn()
    mocks.readPendingMint.mockReturnValue(undefined)
    mocks.getTransactionReceipt.mockResolvedValue({
      status: "success",
      blockNumber: 99n,
      blockHash: finalizedBlockHash,
      logs: [],
    })
    mocks.useAccount.mockReturnValue({
      address: "0x0303030303030303030303030303030303030303",
    })

    render(<MintAuthorizationAction record={record} onMintConfirmed={onMintConfirmed} />, {
      wrapper: Wrapper,
    })
    const mintButton = await screen.findByRole("button", { name: "Mint on Base" })
    await waitFor(() => expect(mintButton).toBeEnabled())
    fireEvent.click(mintButton)

    await waitFor(() => expect(mocks.exactMintReceiptFinalization).toHaveBeenCalled())
    fireEvent(document, new Event("visibilitychange"))
    await waitFor(() => expect(onMintConfirmed).toHaveBeenCalledOnce())
    expect(onMintConfirmed).toHaveBeenCalledWith({
      transactionHash: pendingHash,
      recipient: pendingExpectation.recipient,
      mintedAmount: 450_000_000n,
    })
    expect(mocks.toastSuccess).not.toHaveBeenCalled()
  })

  it.each(["unavailable", "checking"] as const)(
    "allows mint validation when history is %s and rejects a processed deposit",
    async (mintFinalization) => {
      mocks.readPendingMint.mockReturnValue(undefined)
      mocks.validateMintAuthorization.mockRejectedValue(new Error("Deposit already processed"))
      const deposit = {
        ...record,
        deposit_id: new Uint8Array(32).fill(0x11),
        quote: [],
        refund: [],
        gross_amount: 500_000_000n,
        created_at_ns: 1n,
        funding_ledger_block_index: [],
        automatic_progress: [],
        last_settlement_stop_reason: [],
      } as unknown as DepositView
      render(
        <DepositActivityRow
          item={{ key: "deposit:history-outage", direction: "to-base", createdAtNs: 1n, deposit }}
          mintFinalization={mintFinalization}
          writesEnabled
          onRequestRefund={vi.fn()}
          onContinue={vi.fn()}
        />,
        { wrapper: Wrapper },
      )

      const button = await screen.findByRole("button", { name: "Mint on Base" })
      expect(button).toBeEnabled()
      expect(
        screen.queryByText(/Refresh before minting|Checking finalized Base mint history/),
      ).not.toBeInTheDocument()
      fireEvent.click(button)
      await waitFor(() => expect(mocks.validateMintAuthorization).toHaveBeenCalledOnce())
      expect(mocks.writeContractAsync).not.toHaveBeenCalled()
    },
  )

  it("removes the mint action when History discovers a saved pending transaction", async () => {
    let savedPendingMint: typeof pendingMint | undefined
    mocks.readPendingMint.mockImplementation(() => savedPendingMint)
    const deposit = {
      ...record,
      deposit_id: new Uint8Array(32).fill(0x11),
      quote: [{ net_amount: 450_000_000n, service_fee: 50_000_000n }],
      refund: [],
      gross_amount: 500_000_000n,
      created_at_ns: 1n,
      funding_ledger_block_index: [],
      automatic_progress: [],
      last_settlement_stop_reason: [],
    } as unknown as DepositView
    const props = {
      item: { key: "deposit:pending-discovered", direction: "to-base", createdAtNs: 1n, deposit },
      mintFinalization: "absent" as const,
      writesEnabled: true,
      onRequestRefund: vi.fn(),
      onContinue: vi.fn(),
    } as const
    const view = render(<DepositActivityRow {...props} />, { wrapper: Wrapper })

    expect(await screen.findByRole("button", { name: "Mint on Base" })).toBeEnabled()

    savedPendingMint = pendingMint
    view.rerender(<DepositActivityRow {...props} actioningId="refresh" />)

    expect(screen.getByText("Mint pending")).toBeInTheDocument()
    expect(screen.queryByRole("button", { name: "Mint on Base" })).not.toBeInTheDocument()
    expect(
      await screen.findByText("Submitted on Base; refreshing transaction status."),
    ).toBeVisible()
  })

  it("does not enable refund from a locally extrapolated timestamp", async () => {
    mocks.readPendingMint.mockReturnValue(undefined)
    mocks.heartbeatAgeMs.value = 2_000_000
    const onRequestRefund = vi.fn()

    render(<MintAuthorizationAction record={record} onRequestRefund={onRequestRefund} />, {
      wrapper: Wrapper,
    })

    expect(
      await screen.findByText(
        /The mint authorization has expired, so no Base transaction will be sent/,
      ),
    ).toBeInTheDocument()
    expect(screen.queryByRole("button", { name: "Claim refund" })).not.toBeInTheDocument()
    expect(screen.queryByRole("button", { name: "Mint on Base" })).not.toBeInTheDocument()
  })

  it("keeps_mint_available_with_exactly_300_seconds_on_the_latest_Base_clock", async () => {
    mocks.readPendingMint.mockReturnValue(undefined)
    mocks.heartbeatTimestamp.value = 2_000n
    mocks.latestTimestamp.value = 1_700n

    render(<MintAuthorizationAction record={record} onRequestRefund={vi.fn()} />, {
      wrapper: Wrapper,
    })

    expect(screen.queryByRole("button", { name: "Claim refund" })).not.toBeInTheDocument()
    expect(await screen.findByRole("button", { name: "Mint on Base" })).toBeEnabled()
  })

  it("keeps_mint_available_with_only_299_seconds_remaining", async () => {
    mocks.readPendingMint.mockReturnValue(undefined)
    mocks.latestTimestamp.value = 1_701n

    render(<MintAuthorizationAction record={record} onRequestRefund={vi.fn()} />, {
      wrapper: Wrapper,
    })

    expect(await screen.findByRole("button", { name: "Mint on Base" })).toBeEnabled()
    expect(mocks.writeContractAsync).not.toHaveBeenCalled()
  })

  it("enables_refund_only_after_the_finalized_timestamp_passes_the_deadline", async () => {
    mocks.readPendingMint.mockReturnValue(undefined)
    mocks.heartbeatTimestamp.value = 2_001n
    mocks.latestTimestamp.value = 2_001n
    const onRequestRefund = vi.fn()

    render(<MintAuthorizationAction record={record} onRequestRefund={onRequestRefund} />, {
      wrapper: Wrapper,
    })

    const button = await screen.findByRole("button", { name: "Claim refund" })
    fireEvent.click(button)
    expect(onRequestRefund).toHaveBeenCalledOnce()
  })

  it("keeps_the_finalized_refund_path_available_when_the_latest_Base_clock_fails", async () => {
    mocks.readPendingMint.mockReturnValue(undefined)
    mocks.heartbeatTimestamp.value = 2_001n
    mocks.latestUnavailable.value = true
    const onRequestRefund = vi.fn()

    render(<MintAuthorizationAction record={record} onRequestRefund={onRequestRefund} />, {
      wrapper: Wrapper,
    })

    expect(
      await screen.findByText(/Latest Base time is unavailable, but it is not required/),
    ).toBeInTheDocument()
    const button = screen.getByRole("button", { name: "Claim refund" })
    fireEvent.click(button)
    expect(onRequestRefund).toHaveBeenCalledOnce()
  })
  it("persists_wallet_request_before_prompt_and_hash_before_progress", async () => {
    mocks.readPendingMint.mockReturnValue(undefined)
    const order: string[] = []
    mocks.rememberMintRecovery.mockImplementation(async () => {
      order.push("remember")
      return true
    })
    mocks.writeContractAsync.mockImplementation(async () => {
      order.push("wallet")
      return pendingHash
    })
    mocks.savePendingMint.mockImplementation(async () => {
      order.push("save")
    })
    render(
      <MintAuthorizationAction
        record={record}
        onProgress={(event) => {
          if (event.phase === "submitted") order.push("submitted")
        }}
      />,
      { wrapper: Wrapper },
    )
    fireEvent.click(await screen.findByRole("button", { name: "Mint on Base" }))
    await waitFor(() => expect(order).toEqual(["remember", "wallet", "save", "submitted"]))
  })

  it("restored_wallet_request_never_automatically_prompts_again", async () => {
    mocks.readPendingMint.mockReturnValue(undefined)
    mocks.wasMintRequested.mockReturnValue(true)
    mocks.useAccount.mockReturnValue({ address: "0x0303030303030303030303030303030303030303" })
    render(<MintAuthorizationAction record={record} autoPromptOwner="restored-owner" />, {
      wrapper: Wrapper,
    })
    expect(
      await screen.findByText(/Wallet submission will not restart automatically/),
    ).toBeVisible()
    expect(mocks.writeContractAsync).not.toHaveBeenCalled()
    fireEvent.click(screen.getByRole("button", { name: "Mint on Base" }))
    await waitFor(() => expect(mocks.writeContractAsync).toHaveBeenCalledOnce())
  })
})
