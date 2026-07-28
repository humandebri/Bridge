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
}))

vi.mock("wagmi", () => ({
  useAccount: () => ({ address: "0x0000000000000000000000000000000000000001" }),
  useChainId: () => deploymentProfile.chainId,
  useWriteContract: () => ({ isPending: false, writeContractAsync: vi.fn() }),
}))

vi.mock("@/features/status/use-status", () => ({
  useRuntimeValidation: () => ({ refetch: mocks.runtimeRefetch }),
}))

vi.mock("@/features/wallet/ic-wallet-provider", () => ({
  useIcWallet: () => ({ adapter: undefined, account: undefined }),
}))

vi.mock("@/lib/runtime-validation", () => ({
  refetchRuntimeWriteReady: mocks.refetchRuntimeWriteReady,
}))

vi.mock("@/lib/evm/client", () => ({
  basePublicClient: {
    getBlock: mocks.getBlock,
    getTransactionReceipt: mocks.getTransactionReceipt,
    waitForTransactionReceipt: vi.fn(),
  },
}))

vi.mock("@/lib/mint-authorization", () => ({
  contractAuthorization: () => ({ depositId: `0x${"11".repeat(32)}` }),
  readPendingMint: mocks.readPendingMint,
  removePendingMint: mocks.removePendingMint,
  savePendingMint: vi.fn(),
  validateMintAuthorization: mocks.validateMintAuthorization,
}))

vi.mock("sonner", () => ({ toast: { error: vi.fn(), success: vi.fn() } }))

const pendingHash = `0x${"22".repeat(32)}`
const record = {
  state: { AuthorizationAvailable: null },
  mint_authorization: [{
    deadline: 2_000n,
    recipient: Array(20).fill(3),
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
    mocks.validateMintAuthorization.mockReset().mockResolvedValue({ authorization: {} })
    mocks.runtimeRefetch.mockReset()
    mocks.refetchRuntimeWriteReady.mockReset().mockResolvedValue(undefined)
  })

  it("does not clear a transaction hash when its receipt is not found", async () => {
    render(<MintAuthorizationAction record={record} />, { wrapper: Wrapper })

    await waitFor(() => expect(mocks.getTransactionReceipt).toHaveBeenCalledWith({ hash: pendingHash }))
    expect(mocks.removePendingMint).not.toHaveBeenCalled()
    expect(screen.getByText("状態を確認して再試行")).toBeEnabled()
  })

  it("keeps the hash when Base revalidation fails", async () => {
    mocks.validateMintAuthorization.mockRejectedValue(new Error("Mint authorization is no longer valid on Base"))
    render(<MintAuthorizationAction record={record} />, { wrapper: Wrapper })

    fireEvent.click(await screen.findByText("状態を確認して再試行"))

    await waitFor(() => expect(mocks.validateMintAuthorization).toHaveBeenCalledWith(record))
    expect(mocks.removePendingMint).not.toHaveBeenCalled()
    expect(screen.queryByText("保存済みtransactionを解除しますか？")).not.toBeInTheDocument()
  })

  it("clears the hash only after successful revalidation and explicit confirmation", async () => {
    render(<MintAuthorizationAction record={record} />, { wrapper: Wrapper })
    fireEvent.click(await screen.findByText("状態を確認して再試行"))

    expect(await screen.findByText("保存済みtransactionを解除しますか？")).toBeInTheDocument()
    expect(screen.getByText(/元transactionが後から採掘されると/)).toBeInTheDocument()
    fireEvent.click(screen.getByText("Cancel"))
    await waitFor(() =>
      expect(screen.queryByText("保存済みtransactionを解除しますか？")).not.toBeInTheDocument()
    )
    expect(mocks.removePendingMint).not.toHaveBeenCalled()

    fireEvent.click(screen.getByText("状態を確認して再試行"))
    fireEvent.click(await screen.findByText("解除して再試行"))
    await waitFor(() => expect(mocks.removePendingMint).toHaveBeenCalledWith(`0x${"11".repeat(32)}`))
    expect(screen.getByText("Mint on Base")).toBeEnabled()
  })
})
