import { describe, expect, it, vi } from "vitest"
import { createWithdrawalAfterRevalidation } from "./withdrawal-submit"

const expectedWallets = {
  address: `0x${"11".repeat(20)}` as const,
  chainId: 8453,
  icAccount: { owner: "aaaaa-aa", subaccount: new Uint8Array(32) },
}

function dependencies() {
  return {
    expectedWallets,
    refetchRuntime: vi.fn().mockResolvedValue({ data: { ready: true, blockers: [], checkedAt: Date.now() } }),
    currentEvmWallet: vi.fn().mockResolvedValue({ address: expectedWallets.address, chainId: expectedWallets.chainId }),
    currentIcAccount: vi.fn().mockResolvedValue(expectedWallets.icAccount),
    createWithdrawal: vi.fn().mockResolvedValue("0xtx"),
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
    await expect(createWithdrawalAfterRevalidation(deps)).resolves.toBe("0xtx")
    expect(deps.refetchRuntime).toHaveBeenCalledOnce()
    expect(deps.createWithdrawal).toHaveBeenCalledOnce()
  })
})
