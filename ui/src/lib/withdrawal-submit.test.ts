import { describe, expect, it, vi } from "vitest"
import { createWithdrawalAfterRevalidation } from "./withdrawal-submit"

const expectedWallets = {
  address: `0x${"11".repeat(20)}` as const,
  chainId: 8453,
  icAccount: { owner: "aaaaa-aa", subaccount: new Uint8Array(32) },
}

function dependencies() {
  const runtime = { ready: true as const, blockers: [], checkedAt: Date.now(), snapshot: { serviceFee: 10n } }
  return {
    expectedWallets,
    runtime,
    refetchRuntime: vi.fn().mockResolvedValue({ data: runtime }),
    currentEvmWallet: vi.fn().mockResolvedValue({ address: expectedWallets.address, chainId: expectedWallets.chainId }),
    currentIcAccount: vi.fn().mockResolvedValue(expectedWallets.icAccount),
    refetchFinancials: vi.fn().mockResolvedValue({ serviceFee: 10n, balance: 100n }),
    validateFinancials: vi.fn(),
    createWithdrawal: vi.fn<(quote: { serviceFee: bigint; balance: bigint }) => Promise<`0x${string}`>>().mockResolvedValue("0xtx"),
    onBroadcast: vi.fn<(hash: `0x${string}`) => void>(),
  }
}

describe("createWithdrawalAfterRevalidation", () => {
  it("does not call createWithdrawal when the signer or code drifts while approval is pending", async () => {
    const deps = dependencies()
    deps.refetchRuntime.mockResolvedValue({ data: { ready: false, blockers: ["Bridge signer differs from the reviewed profile"], checkedAt: Date.now() } })
    await expect(createWithdrawalAfterRevalidation(deps)).rejects.toThrow("Bridge signer differs from the reviewed profile")
    expect(deps.createWithdrawal).not.toHaveBeenCalled()
  })

  it("does not call createWithdrawal when an account or chain drifts while approval is pending", async () => {
    const deps = dependencies()
    deps.currentEvmWallet.mockResolvedValue({ address: expectedWallets.address, chainId: 1 })
    await expect(createWithdrawalAfterRevalidation(deps)).rejects.toThrow("changed after approval")
    expect(deps.createWithdrawal).not.toHaveBeenCalled()
  })

  it("submits only after both action-time checks pass", async () => {
    const deps = dependencies()
    await expect(createWithdrawalAfterRevalidation(deps)).resolves.toEqual({ transactionHash: "0xtx", pendingSaved: true })
    expect(deps.refetchRuntime).toHaveBeenCalledOnce()
    expect(deps.refetchFinancials).toHaveBeenCalledOnce()
    expect(deps.refetchFinancials).toHaveBeenCalledWith(deps.runtime)
    expect(deps.validateFinancials).toHaveBeenCalledWith({ serviceFee: 10n, balance: 100n })
    expect(deps.createWithdrawal).toHaveBeenCalledWith({ serviceFee: 10n, balance: 100n })
    expect(deps.onBroadcast).toHaveBeenCalledWith("0xtx")
  })

  it("persists the broadcast result before returning control to the caller", async () => {
    const deps = dependencies()
    let persisted: string | undefined
    deps.onBroadcast.mockImplementation((hash) => { persisted = hash })

    const result = await createWithdrawalAfterRevalidation(deps)

    expect(result).toEqual({ transactionHash: "0xtx", pendingSaved: true })
    expect(persisted).toBe(result.transactionHash)
  })

  it("returns the broadcast hash when persistence fails after broadcast", async () => {
    const deps = dependencies()
    deps.onBroadcast.mockImplementation((hash) => { throw new Error(`could not save ${hash}`) })

    await expect(createWithdrawalAfterRevalidation(deps)).resolves.toEqual({ transactionHash: "0xtx", pendingSaved: false })
    expect(deps.createWithdrawal).toHaveBeenCalledOnce()
  })

  it("does not submit when the final financial validation fails", async () => {
    const deps = dependencies()
    deps.validateFinancials.mockImplementation(() => { throw new Error("Amount must exceed the service fee") })
    await expect(createWithdrawalAfterRevalidation(deps)).rejects.toThrow("Amount must exceed")
    expect(deps.createWithdrawal).not.toHaveBeenCalled()
  })
})
