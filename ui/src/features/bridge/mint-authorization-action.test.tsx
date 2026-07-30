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
  writeContractAsync: vi.fn(),
  waitForTransactionReceipt: vi.fn(),
  savePendingMint: vi.fn(),
  useAccount: vi.fn(),
}))

vi.mock("wagmi", () => ({
  useAccount: mocks.useAccount,
  useChainId: () => deploymentProfile.chainId,
  useWriteContract: () => ({ isPending: false, writeContractAsync: mocks.writeContractAsync }),
}))

vi.mock("@/features/status/use-status", () => ({
  useRuntimeValidation: () => ({ refetch: mocks.runtimeRefetch }),
}))

vi.mock("@/lib/runtime-validation", () => ({
  refetchRuntimeWriteReady: mocks.refetchRuntimeWriteReady,
}))

vi.mock("@/lib/evm/client", () => ({
  basePublicClient: {
    getBlock: mocks.getBlock,
    getTransactionReceipt: mocks.getTransactionReceipt,
    waitForTransactionReceipt: mocks.waitForTransactionReceipt,
  },
}))

vi.mock("@/lib/mint-authorization", () => ({
  contractAuthorization: () => ({ depositId: `0x${"11".repeat(32)}` }),
  validateMintAuthorization: mocks.validateMintAuthorization,
}))

vi.mock("@/lib/pending-confirmations", () => ({
  readPendingMint: mocks.readPendingMint,
  removePendingMint: mocks.removePendingMint,
  savePendingMint: mocks.savePendingMint,
}))

vi.mock("@/lib/browser-lock", () => ({
  withBrowserLock: (_name: string, action: () => unknown) => action(),
}))

vi.mock("sonner", () => ({ toast: { error: vi.fn(), success: vi.fn() } }))

const pendingHash = `0x${"22".repeat(32)}`
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
  afterEach(cleanup)

  beforeEach(() => {
    mocks.getBlock.mockReset().mockResolvedValue({ timestamp: 1_000n })
    mocks.getTransactionReceipt.mockReset().mockRejectedValue(new Error("not found"))
    mocks.readPendingMint.mockReset().mockReturnValue(pendingHash)
    mocks.removePendingMint.mockReset().mockResolvedValue(undefined)
    mocks.validateMintAuthorization.mockReset().mockResolvedValue({
      authorization: { depositId: `0x${"11".repeat(32)}` },
      signature: "0xsigned",
      recipient: "0x0303030303030303030303030303030303030303",
    })
    mocks.runtimeRefetch.mockReset()
    mocks.refetchRuntimeWriteReady.mockReset().mockResolvedValue(undefined)
    mocks.writeContractAsync.mockReset().mockResolvedValue(pendingHash)
    mocks.waitForTransactionReceipt.mockReset().mockResolvedValue({ status: "success" })
    mocks.savePendingMint.mockReset().mockResolvedValue(undefined)
    mocks.useAccount.mockReset().mockReturnValue({
      address: "0x0000000000000000000000000000000000000001",
    })
  })

  it("does not clear a transaction hash when its receipt is not found", async () => {
    render(<MintAuthorizationAction record={record} />, { wrapper: Wrapper })

    await waitFor(() => expect(mocks.getTransactionReceipt).toHaveBeenCalledWith({ hash: pendingHash }))
    expect(mocks.removePendingMint).not.toHaveBeenCalled()
    expect(screen.getByText("Check status and retry")).toBeEnabled()
  })

  it("keeps the hash when Base revalidation fails", async () => {
    mocks.validateMintAuthorization.mockRejectedValue(new Error("Mint authorization is no longer valid on Base"))
    render(<MintAuthorizationAction record={record} />, { wrapper: Wrapper })

    fireEvent.click(await screen.findByText("Check status and retry"))

    await waitFor(() => expect(mocks.validateMintAuthorization).toHaveBeenCalledWith(record))
    expect(mocks.removePendingMint).not.toHaveBeenCalled()
    expect(screen.queryByText("Clear the saved transaction?")).not.toBeInTheDocument()
  })

  it("clears the hash only after successful revalidation and explicit confirmation", async () => {
    render(<MintAuthorizationAction record={record} />, { wrapper: Wrapper })
    fireEvent.click(await screen.findByText("Check status and retry"))

    expect(await screen.findByText("Clear the saved transaction?")).toBeInTheDocument()
    expect(screen.getByText(/original transaction is mined later/)).toBeInTheDocument()
    fireEvent.click(screen.getByText("Cancel"))
    await waitFor(() =>
      expect(screen.queryByText("Clear the saved transaction?")).not.toBeInTheDocument()
    )
    expect(mocks.removePendingMint).not.toHaveBeenCalled()

    fireEvent.click(screen.getByText("Check status and retry"))
    fireEvent.click(await screen.findByText("Clear and retry"))
    await waitFor(() => expect(mocks.removePendingMint).toHaveBeenCalledWith(`0x${"11".repeat(32)}`))
    await waitFor(() => expect(screen.getByText("Mint on Base")).toBeEnabled())
  })

  it("opens the Base wallet once and does not request an IC success confirmation", async () => {
    mocks.readPendingMint.mockReturnValue(undefined)
    mocks.useAccount.mockReturnValue({
      address: "0x0303030303030303030303030303030303030303",
    })
    const view = render(<MintAuthorizationAction record={record} autoPromptOwner="aaaaa-aa" />, { wrapper: Wrapper })

    await waitFor(() => expect(mocks.writeContractAsync).toHaveBeenCalledOnce())
    expect(await screen.findByText("Minted on Base")).toBeInTheDocument()
    expect(screen.queryByText("Confirm mint on IC")).not.toBeInTheDocument()
    expect(mocks.writeContractAsync).toHaveBeenCalledWith(expect.objectContaining({
      functionName: "mintDepositWithAuthorization",
    }))

    view.unmount()
    render(<MintAuthorizationAction record={record} autoPromptOwner="aaaaa-aa" />, { wrapper: Wrapper })
    expect(mocks.writeContractAsync).toHaveBeenCalledOnce()
  })

  it("blocks a new mint while finalized history is unavailable", async () => {
    mocks.readPendingMint.mockReturnValue(undefined)
    render(<MintAuthorizationAction record={record} mintBlockedReason="Finalized Base mint history is unavailable. Refresh before minting." />, { wrapper: Wrapper })

    expect(await screen.findByText("Finalized Base mint history is unavailable. Refresh before minting.")).toBeInTheDocument()
    expect(screen.getByText("Mint on Base")).toBeDisabled()
    fireEvent.click(screen.getByText("Mint on Base"))
    expect(mocks.writeContractAsync).not.toHaveBeenCalled()
  })
})
