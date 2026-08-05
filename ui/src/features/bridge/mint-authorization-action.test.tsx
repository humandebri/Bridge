import { QueryClient, QueryClientProvider } from "@tanstack/react-query"
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react"
import type { ReactNode } from "react"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"
import type { DepositView } from "@/generated/bridge.did"
import { deploymentProfile } from "@/config/profile"
import { MintAuthorizationAction } from "./mint-authorization-action"

const mocks = vi.hoisted(() => ({
  getTransactionReceipt: vi.fn(),
  getBlock: vi.fn(),
  readPendingMint: vi.fn(),
  removePendingMint: vi.fn(),
  validateMintAuthorization: vi.fn(),
  runtimeRefetch: vi.fn(),
  refetchRuntimeWriteReady: vi.fn(),
  runtimeWriteBlocker: vi.fn(),
  writeContractAsync: vi.fn(),
  savePendingMint: vi.fn(),
  useAccount: vi.fn(),
  exactMintReceiptFinalization: vi.fn(),
  toastSuccess: vi.fn(),
  toastError: vi.fn(),
  heartbeatAgeMs: { value: 0 },
  heartbeatTimestamp: { value: 1_000n },
  authorizationDeadline: { value: 2_000n },
}))

vi.mock("wagmi", () => ({
  useAccount: mocks.useAccount,
  useChainId: () => deploymentProfile.chainId,
  useWriteContract: () => ({ isPending: false, writeContractAsync: mocks.writeContractAsync }),
}))

vi.mock("@/features/status/use-status", () => ({
  useRuntimeValidation: () => ({ data: { ready: true, blockers: [], checkedAt: Date.now() }, refetch: mocks.runtimeRefetch }),
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
}))

vi.mock("@/lib/runtime-validation", () => ({
  refetchRuntimeAttestedWriteReady: mocks.refetchRuntimeWriteReady,
  refetchRuntimeWriteReady: mocks.refetchRuntimeWriteReady,
  runtimeWriteBlocker: mocks.runtimeWriteBlocker,
}))

vi.mock("@/lib/evm/client", () => ({
  basePublicClient: {
    getBlock: mocks.getBlock,
    getTransactionReceipt: mocks.getTransactionReceipt,
  },
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
}))

vi.mock("@/lib/pending-confirmations", () => ({
  readPendingMint: mocks.readPendingMint,
  removePendingMint: mocks.removePendingMint,
  savePendingMint: mocks.savePendingMint,
}))

vi.mock("@/lib/browser-lock", () => ({
  withBrowserLock: (_name: string, action: () => unknown) => action(),
}))

vi.mock("sonner", () => ({ toast: { error: mocks.toastError, success: mocks.toastSuccess } }))

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
  mint_authorization: [{
    deadline: 2_000n,
    recipient: Array(20).fill(3),
    digest: Array(32).fill(0x11),
  }],
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
    deploymentProfile.bridgeAddress = "0x1111111111111111111111111111111111111111"
    mocks.getBlock.mockReset().mockImplementation(({ blockTag, blockNumber }: { blockTag?: string; blockNumber?: bigint }) =>
      Promise.resolve({
        number: blockTag === "finalized" ? 100n : blockNumber,
        hash: finalizedBlockHash,
        timestamp: 1_000n,
      }))
    mocks.getTransactionReceipt.mockReset().mockRejectedValue(new Error("not found"))
    mocks.readPendingMint.mockReset().mockReturnValue(pendingMint)
    mocks.removePendingMint.mockReset().mockResolvedValue(undefined)
    mocks.validateMintAuthorization.mockReset().mockResolvedValue({
      authorization: { depositId: `0x${"11".repeat(32)}` },
      digest: `0x${"11".repeat(32)}`,
      signature: "0xsigned",
      recipient: "0x0303030303030303030303030303030303030303",
    })
    mocks.runtimeRefetch.mockReset()
    mocks.runtimeWriteBlocker.mockReset().mockReturnValue(undefined)
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
    mocks.exactMintReceiptFinalization.mockReset().mockReturnValue("finalized")
    mocks.savePendingMint.mockReset().mockResolvedValue(undefined)
    mocks.toastSuccess.mockReset()
    mocks.toastError.mockReset()
    mocks.heartbeatAgeMs.value = 0
    mocks.heartbeatTimestamp.value = 1_000n
    mocks.authorizationDeadline.value = 2_000n
    mocks.useAccount.mockReset().mockReturnValue({
      address: "0x0000000000000000000000000000000000000001",
    })
  })

  it("does not clear a transaction hash when its receipt is not found", async () => {
    render(<MintAuthorizationAction record={record} />, { wrapper: Wrapper })

    await waitFor(() => expect(mocks.getTransactionReceipt).toHaveBeenCalledWith({ hash: pendingHash }))
    expect(mocks.removePendingMint).not.toHaveBeenCalled()
    expect(screen.queryByText(/Authorization valid for/)).not.toBeInTheDocument()
    expect(screen.getByText(/Waiting for inclusion\./)).toBeInTheDocument()
    expect(screen.getByText("Review saved transaction")).toBeEnabled()
  })

  it("keeps retry recovery out of the compact History action cell", async () => {
    render(<MintAuthorizationAction record={record} compact />, { wrapper: Wrapper })

    expect(await screen.findByText("Base receipt unavailable; checking")).toBeInTheDocument()
    expect(screen.queryByText("Review saved transaction")).not.toBeInTheDocument()
    expect(screen.queryByRole("button")).not.toBeInTheDocument()
  })

  it("keeps the hash when Base revalidation fails", async () => {
    mocks.validateMintAuthorization.mockRejectedValue(new Error("Mint authorization is no longer valid on Base"))
    render(<MintAuthorizationAction record={record} />, { wrapper: Wrapper })

    fireEvent.click(await screen.findByText("Review saved transaction"))

    await waitFor(() => expect(mocks.validateMintAuthorization).toHaveBeenCalledWith(record, expect.any(Object)))
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
    mocks.exactMintReceiptFinalization.mockReturnValue("conflict")
    render(<MintAuthorizationAction record={record} onMintConfirmed={onMintConfirmed} />, { wrapper: Wrapper })

    expect(await screen.findByText("Deposit identity conflict. Do not submit another transaction.")).toBeInTheDocument()
    expect(screen.queryByText("Minted on Base")).not.toBeInTheDocument()
    expect(mocks.removePendingMint).not.toHaveBeenCalled()
    expect(onMintConfirmed).not.toHaveBeenCalled()
  })

  it("keeps_a_mined_transaction_pending_until_its_block_is_finalized", async () => {
    const onMintConfirmed = vi.fn()
    const onProgress = vi.fn()
    mocks.getTransactionReceipt.mockResolvedValue({
      status: "success",
      blockNumber: 101n,
      blockHash: finalizedBlockHash,
      logs: [],
    })

    render(<MintAuthorizationAction record={record} onMintConfirmed={onMintConfirmed} onProgress={onProgress} />, { wrapper: Wrapper })

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
    expect(screen.getByText("Included on Base; awaiting finality")).toBeInTheDocument()
    expect(screen.queryByText("Review saved transaction")).not.toBeInTheDocument()
  })

  it("does not offer a retry for a reverted receipt before its block is finalized", async () => {
    const onProgress = vi.fn()
    mocks.getTransactionReceipt.mockResolvedValue({
      status: "reverted",
      blockNumber: 101n,
      blockHash: finalizedBlockHash,
      logs: [],
    })

    render(<MintAuthorizationAction record={record} onProgress={onProgress} />, { wrapper: Wrapper })

    expect(await screen.findByText("Transaction reverted; awaiting finality")).toBeInTheDocument()
    expect(screen.queryByText("Review saved transaction")).not.toBeInTheDocument()
    expect(mocks.removePendingMint).not.toHaveBeenCalled()
    expect(onProgress).toHaveBeenCalledWith({
      phase: "included",
      transactionHash: pendingHash,
      blockNumber: 101n,
      outcome: "reverted",
    })
  })

  it("reports a saved transaction exactly once after its exact receipt is confirmed", async () => {
    const onMintConfirmed = vi.fn()
    mocks.getTransactionReceipt.mockResolvedValue({
      status: "success",
      blockNumber: 99n,
      blockHash: finalizedBlockHash,
      logs: [],
    })

    render(<MintAuthorizationAction record={record} onMintConfirmed={onMintConfirmed} />, { wrapper: Wrapper })

    await waitFor(() => expect(onMintConfirmed).toHaveBeenCalledOnce())
    expect(onMintConfirmed).toHaveBeenCalledWith({
      transactionHash: pendingHash,
      recipient: pendingExpectation.recipient,
      mintedAmount: 450_000_000n,
    })
    expect(mocks.toastSuccess).not.toHaveBeenCalled()
  })

  it("clears the hash only after successful revalidation and explicit confirmation", async () => {
    render(<MintAuthorizationAction record={record} />, { wrapper: Wrapper })
    fireEvent.click(await screen.findByText("Review saved transaction"))

    expect(await screen.findByText("Clear the saved transaction reference?")).toBeInTheDocument()
    expect(screen.getByText(/original transaction is mined later/)).toBeInTheDocument()
    fireEvent.click(screen.getByText("Cancel"))
    await waitFor(() =>
      expect(screen.queryByText("Clear the saved transaction reference?")).not.toBeInTheDocument()
    )
    expect(mocks.removePendingMint).not.toHaveBeenCalled()

    fireEvent.click(screen.getByText("Review saved transaction"))
    fireEvent.click(await screen.findByText("Clear and retry"))
    await waitFor(() => expect(mocks.removePendingMint).toHaveBeenCalledWith(pendingExpectation))
    await waitFor(() => expect(screen.getByText("Mint on Base")).toBeEnabled())
  })

  it("opens the Base wallet once and does not request an IC success confirmation", async () => {
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
    const view = render(<MintAuthorizationAction record={record} autoPromptOwner="aaaaa-aa" />, { wrapper: Wrapper })

    await waitFor(() => expect(mocks.writeContractAsync).toHaveBeenCalledOnce())
    expect(mocks.refetchRuntimeWriteReady).toHaveBeenCalledOnce()
    expect(await screen.findByText("Minted on Base")).toBeInTheDocument()
    expect(screen.queryByText("Confirm mint on IC")).not.toBeInTheDocument()
    expect(mocks.writeContractAsync).toHaveBeenCalledWith(expect.objectContaining({
      functionName: "mintDepositWithAuthorization",
    }))

    view.unmount()
    render(<MintAuthorizationAction record={record} autoPromptOwner="aaaaa-aa" />, { wrapper: Wrapper })
    expect(mocks.writeContractAsync).toHaveBeenCalledOnce()
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

    render(<MintAuthorizationAction record={record} onMintConfirmed={onMintConfirmed} />, { wrapper: Wrapper })
    const mintButton = await screen.findByRole("button", { name: "Mint on Base" })
    await waitFor(() => expect(mintButton).toBeEnabled())
    fireEvent.click(mintButton)

    await waitFor(() => expect(onMintConfirmed).toHaveBeenCalledOnce())
    expect(onMintConfirmed).toHaveBeenCalledWith({
      transactionHash: pendingHash,
      recipient: pendingExpectation.recipient,
      mintedAmount: 450_000_000n,
    })
    expect(mocks.toastSuccess).not.toHaveBeenCalled()
  })

  it("blocks a new mint while finalized history is unavailable", async () => {
    mocks.readPendingMint.mockReturnValue(undefined)
    render(<MintAuthorizationAction record={record} mintBlockedReason="Finalized Base mint history is unavailable. Refresh before minting." />, { wrapper: Wrapper })

    expect(await screen.findByText("Finalized Base mint history is unavailable. Refresh before minting.")).toBeInTheDocument()
    expect(screen.getByText("Mint on Base")).toBeDisabled()
    fireEvent.click(screen.getByText("Mint on Base"))
    expect(mocks.writeContractAsync).not.toHaveBeenCalled()
  })

  it("does not enable refund from a locally extrapolated timestamp", async () => {
    mocks.readPendingMint.mockReturnValue(undefined)
    mocks.heartbeatAgeMs.value = 2_000_000
    const onRequestRefund = vi.fn()

    render(<MintAuthorizationAction record={record} onRequestRefund={onRequestRefund} />, { wrapper: Wrapper })

    expect(await screen.findByText("Estimated Base time has passed the deadline. A fresh finalized check will decide whether mint or refund is available.")).toBeInTheDocument()
    expect(screen.queryByRole("button", { name: "Claim refund" })).not.toBeInTheDocument()
    expect(screen.getByRole("button", { name: "Mint on Base" })).toBeEnabled()
  })

  it("keeps mint available when finalized time is exactly the deadline", async () => {
    mocks.readPendingMint.mockReturnValue(undefined)
    mocks.heartbeatTimestamp.value = 2_000n

    render(<MintAuthorizationAction record={record} onRequestRefund={vi.fn()} />, { wrapper: Wrapper })

    expect(screen.queryByRole("button", { name: "Claim refund" })).not.toBeInTheDocument()
    expect(await screen.findByRole("button", { name: "Mint on Base" })).toBeEnabled()
  })

  it("enables refund only after the finalized timestamp passes the deadline", async () => {
    mocks.readPendingMint.mockReturnValue(undefined)
    mocks.heartbeatTimestamp.value = 2_001n
    const onRequestRefund = vi.fn()

    render(<MintAuthorizationAction record={record} onRequestRefund={onRequestRefund} />, { wrapper: Wrapper })

    const button = await screen.findByRole("button", { name: "Claim refund" })
    fireEvent.click(button)
    expect(onRequestRefund).toHaveBeenCalledOnce()
  })
})
