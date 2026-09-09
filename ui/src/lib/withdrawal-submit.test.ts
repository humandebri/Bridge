import { bridgeProgressSteps, createBridgeProgress } from "./bridge-progress"
import {
  BlockNotFoundError,
  ContractFunctionRevertedError,
  encodeErrorResult,
  createPublicClient,
  http,
} from "viem"
import { transferErrorMessage } from "./transfer-error"
import { afterEach, describe, expect, it, vi } from "vitest"
import {
  createWithdrawalAfterRevalidation,
  waitForWithdrawalApproval,
  withdrawalAbi,
} from "./withdrawal-submit"

const expectedWallets = {
  address: `0x${"11".repeat(20)}` as const,
  chainId: 8453,
  icAccount: { owner: "aaaaa-aa", subaccount: new Uint8Array(32) },
}

function dependencies() {
  const runtime = {
    ready: true as const,
    blockers: [],
    checkedAt: Date.now(),
    snapshot: { serviceFee: 10n },
  }
  return {
    expectedWallets,
    approval: {
      amount: 100n,
      getBlock: vi.fn().mockResolvedValue({ number: 10n, hash: "0xabc" }),
      readAllowance: vi.fn().mockResolvedValue(100n),
    },
    simulateWithdrawal: vi.fn().mockResolvedValue(undefined),
    runtime,
    refetchRuntime: vi.fn().mockResolvedValue({ data: runtime }),
    currentEvmWallet: vi
      .fn()
      .mockResolvedValue({ address: expectedWallets.address, chainId: expectedWallets.chainId }),
    currentIcAccount: vi.fn().mockResolvedValue(expectedWallets.icAccount),
    refetchFinancials: vi.fn().mockResolvedValue({ serviceFee: 10n, balance: 100n }),
    validateFinancials: vi.fn(),
    createWithdrawal: vi
      .fn<(quote: { serviceFee: bigint; balance: bigint }) => Promise<`0x${string}`>>()
      .mockResolvedValue("0xtx"),
    onBroadcast: vi.fn<(hash: `0x${string}`) => void>(),
  }
}

describe("withdrawal submission action-time checks", () => {
  it("does not call createWithdrawal when the signer or code drifts while approval is pending", async () => {
    const deps = dependencies()
    deps.refetchRuntime.mockResolvedValue({
      data: {
        ready: false,
        blockers: ["Bridge signer differs from the reviewed profile"],
        checkedAt: Date.now(),
      },
    })
    await expect(createWithdrawalAfterRevalidation(deps)).rejects.toThrow(
      "Bridge signer differs from the reviewed profile",
    )
    expect(deps.createWithdrawal).not.toHaveBeenCalled()
  })

  it("does not call createWithdrawal when an account or chain drifts while approval is pending", async () => {
    const deps = dependencies()
    deps.currentEvmWallet.mockResolvedValue({ address: expectedWallets.address, chainId: 1 })
    await expect(createWithdrawalAfterRevalidation(deps)).rejects.toThrow("changed after approval")
    expect(deps.createWithdrawal).not.toHaveBeenCalled()
  })

  it("createWithdrawalAfterRevalidation", async () => {
    const deps = dependencies()
    await expect(createWithdrawalAfterRevalidation(deps)).resolves.toEqual({
      transactionHash: "0xtx",
      pendingSaved: true,
    })
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
    deps.onBroadcast.mockImplementation((hash) => {
      persisted = hash
    })

    const result = await createWithdrawalAfterRevalidation(deps)

    expect(result).toEqual({ transactionHash: "0xtx", pendingSaved: true })
    expect(persisted).toBe(result.transactionHash)
  })

  it("returns the broadcast hash when persistence fails after broadcast", async () => {
    const deps = dependencies()
    deps.onBroadcast.mockImplementation((hash) => {
      throw new Error(`could not save ${hash}`)
    })

    await expect(createWithdrawalAfterRevalidation(deps)).resolves.toEqual({
      transactionHash: "0xtx",
      pendingSaved: false,
    })
    expect(deps.createWithdrawal).toHaveBeenCalledOnce()
  })

  it("does not submit when the final financial validation fails", async () => {
    const deps = dependencies()
    deps.validateFinancials.mockImplementation(() => {
      throw new Error("Amount must exceed the service fee")
    })
    await expect(createWithdrawalAfterRevalidation(deps)).rejects.toThrow("Amount must exceed")
    expect(deps.createWithdrawal).not.toHaveBeenCalled()
  })
})

describe("withdrawal approval observation", () => {
  afterEach(() => vi.useRealTimers())
  function observation() {
    return {
      receipt: { status: "success" as const, blockNumber: 10n, blockHash: "0xabc" as const },
      amount: 100n,
      getBlock: vi.fn().mockResolvedValue({ number: 10n, hash: "0xabc" }),
      readAllowance: vi.fn().mockResolvedValue(100n),
      simulate: vi.fn(async (blockNumber: bigint) => blockNumber),
    }
  }
  it("waits_for_the_approval_block_and_allowance_before_revalidation", async () => {
    vi.useFakeTimers()
    const progress = createBridgeProgress({
      direction: "withdraw",
      phase: "awaiting-base-approval-reflection",
      source: expectedWallets.address,
      destination: "aaaaa-aa",
      sendAmount: "1",
      receiveAmount: "0.9",
      sendSymbol: "bSNS",
      receiveSymbol: "SNS",
    })
    expect(
      bridgeProgressSteps(progress).find((step) => step.label === "Base token approval"),
    ).toMatchObject({
      status: "current",
      note: "Confirming token approval. No wallet action is needed.",
    })
    const deps = observation()
    deps.getBlock.mockResolvedValueOnce({ number: 9n, hash: "0xold" })
    deps.readAllowance.mockResolvedValueOnce(0n)
    const submit = dependencies()
    const flow = waitForWithdrawalApproval(deps).then(async (blockNumber) => {
      expect(blockNumber).toBe(10n)
      return createWithdrawalAfterRevalidation(submit)
    })
    await vi.advanceTimersByTimeAsync(0)
    expect(deps.readAllowance).not.toHaveBeenCalled()
    expect(submit.createWithdrawal).not.toHaveBeenCalled()
    await vi.advanceTimersByTimeAsync(2_000)
    expect(submit.refetchRuntime).not.toHaveBeenCalled()
    await vi.advanceTimersByTimeAsync(2_000)
    await flow
    expect(deps.readAllowance).toHaveBeenCalledWith(10n)
    expect(submit.refetchRuntime).toHaveBeenCalledOnce()
    expect(submit.createWithdrawal).toHaveBeenCalledOnce()
  })
  it("times_out_without_submitting_even_when_an_rpc_never_returns", async () => {
    vi.useFakeTimers()
    const deps = observation()
    deps.getBlock.mockReturnValue(new Promise(() => {}))
    const result = expect(waitForWithdrawalApproval(deps)).rejects.toThrow(
      "withdrawal was not submitted",
    )
    await vi.advanceTimersByTimeAsync(60_000)
    await result
    expect(deps.readAllowance).not.toHaveBeenCalled()
  })
  it("bounds_unpropagated_allowance_to_sixty_seconds", async () => {
    vi.useFakeTimers()
    const deps = observation()
    deps.readAllowance.mockResolvedValue(0n)
    const result = expect(waitForWithdrawalApproval(deps)).rejects.toThrow("60 seconds")
    await vi.advanceTimersByTimeAsync(60_000)
    await result
    expect(deps.readAllowance).toHaveBeenCalledTimes(30)
  })
  it("retries_missing_blocks_but_stops_on_other_errors", async () => {
    vi.useFakeTimers()
    const deps = observation()
    deps.getBlock.mockRejectedValueOnce(new BlockNotFoundError({ blockNumber: 10n }))
    const result = waitForWithdrawalApproval(deps)
    await vi.advanceTimersByTimeAsync(2_000)
    await expect(result).resolves.toBe(10n)
    deps.readAllowance.mockRejectedValue(new Error("contract rejected"))
    await expect(waitForWithdrawalApproval(deps)).rejects.toThrow("contract rejected")
  })
  it("rejects_reverted_approval_and_changed_block_hash", async () => {
    const deps = observation()
    await expect(
      waitForWithdrawalApproval({ ...deps, receipt: { ...deps.receipt, status: "reverted" } }),
    ).rejects.toThrow("Token approval failed")
    expect(deps.getBlock).not.toHaveBeenCalled()
    deps.getBlock.mockResolvedValue({ number: 10n, hash: "0xchanged" })
    await expect(waitForWithdrawalApproval(deps)).rejects.toThrow("approval block changed")
    expect(deps.readAllowance).not.toHaveBeenCalled()
  })
  it("checks_allowance_at_a_numbered_block_without_a_new_approval", async () => {
    const deps = observation()
    await expect(waitForWithdrawalApproval({ ...deps, receipt: undefined })).resolves.toBe(10n)
    expect(deps.readAllowance).toHaveBeenCalledWith(10n)
  })
  it("decodes_bubbled_token_allowance_errors", () => {
    const data = encodeErrorResult({
      abi: withdrawalAbi,
      errorName: "ERC20InsufficientAllowance",
      args: [expectedWallets.address, 0n, 500000000n],
    })
    expect(data.slice(0, 10)).toBe("0xfb8f41b2")
    const error = new ContractFunctionRevertedError({
      abi: withdrawalAbi,
      data,
      functionName: "createWithdrawal",
    })
    expect(transferErrorMessage(error)).toContain("Allowed: 0; required: 500000000")
    expect(transferErrorMessage(new Error("unknown revert 0x12345678"))).toBe(
      "unknown revert 0x12345678",
    )
  })
})

async function callError(message: string, code = -32000, data?: string) {
  const client = createPublicClient({
    transport: http("https://rpc.invalid", {
      retryCount: 0,
      fetchFn: async () =>
        new Response(
          JSON.stringify({
            jsonrpc: "2.0",
            id: 1,
            error: { code, message, ...(data === undefined ? {} : { data }) },
          }),
          { headers: { "Content-Type": "application/json" } },
        ),
    }),
  })
  return client
    .simulateContract({
      account: expectedWallets.address,
      address: expectedWallets.address,
      abi: withdrawalAbi,
      functionName: "createWithdrawal",
      args: [100n, 10n, "0x01", `0x${"00".repeat(32)}`],
      blockNumber: 10n,
    })
    .then(
      () => {
        throw new Error("Expected an RPC failure")
      },
      (error: unknown) => error,
    )
}

describe("withdrawal eth_call propagation", () => {
  afterEach(() => vi.useRealTimers())
  it("retries_missing_eth_call_blocks_and_revalidates_before_one_broadcast", async () => {
    for (const stage of ["allowance", "simulation"]) {
      const missing = await callError("header not found")
      vi.useFakeTimers()
      const deps = dependencies()
      if (stage === "allowance") deps.approval.readAllowance.mockRejectedValueOnce(missing)
      else deps.simulateWithdrawal.mockRejectedValueOnce(missing)
      const result = createWithdrawalAfterRevalidation(deps)
      await vi.advanceTimersByTimeAsync(0)
      expect(deps.createWithdrawal).not.toHaveBeenCalled()
      await vi.advanceTimersByTimeAsync(2_000)
      await expect(result).resolves.toMatchObject({ transactionHash: "0xtx" })
      expect(deps.refetchRuntime).toHaveBeenCalledTimes(stage === "allowance" ? 1 : 2)
      expect(deps.approval.readAllowance).toHaveBeenCalledTimes(2)
      expect(deps.simulateWithdrawal).toHaveBeenLastCalledWith(
        { serviceFee: 10n, balance: 100n },
        10n,
      )
      expect(deps.createWithdrawal).toHaveBeenCalledOnce()
      vi.useRealTimers()
    }
  })
  it("does_not_retry_unrelated_rpc_errors_or_contract_reverts", async () => {
    const revert = encodeErrorResult({
      abi: withdrawalAbi,
      errorName: "ERC20InsufficientAllowance",
      args: [expectedWallets.address, 0n, 100n],
    })
    for (const [message, code, data] of [
      ["invalid argument 0", -32000, undefined],
      ["header not found", -32603, undefined],
      ["execution reverted", -32000, revert],
      ["header not found", -32000, revert],
      ["unauthorized", -32000, undefined],
    ] as const) {
      const error = await callError(message, code, data)
      const deps = dependencies()
      deps.simulateWithdrawal.mockRejectedValue(error)
      await expect(createWithdrawalAfterRevalidation(deps)).rejects.toBe(error)
      expect(deps.simulateWithdrawal).toHaveBeenCalledOnce()
      expect(deps.createWithdrawal).not.toHaveBeenCalled()
    }
  })
  it("shares_the_deadline_with_simulation_and_ignores_late_success", async () => {
    const missing = await callError("header not found")
    vi.useFakeTimers()
    const deps = dependencies()
    for (let attempt = 0; attempt < 29; attempt++)
      deps.approval.readAllowance.mockRejectedValueOnce(missing)
    let finish!: () => void
    deps.simulateWithdrawal.mockReturnValue(
      new Promise<void>((resolve) => {
        finish = resolve
      }),
    )
    const result = expect(createWithdrawalAfterRevalidation(deps)).rejects.toThrow("60 seconds")
    await vi.advanceTimersByTimeAsync(58_000)
    expect(deps.simulateWithdrawal).toHaveBeenCalledOnce()
    await vi.advanceTimersByTimeAsync(2_000)
    await result
    finish()
    await vi.advanceTimersByTimeAsync(0)
    expect(deps.createWithdrawal).not.toHaveBeenCalled()
    expect(deps.onBroadcast).not.toHaveBeenCalled()
  })
  it("stops_when_wallet_changes_during_simulation_retry", async () => {
    const missing = await callError("header not found")
    vi.useFakeTimers()
    const deps = dependencies()
    deps.simulateWithdrawal.mockRejectedValueOnce(missing)
    const result = expect(createWithdrawalAfterRevalidation(deps)).rejects.toThrow(
      "changed after approval",
    )
    await vi.advanceTimersByTimeAsync(0)
    deps.currentEvmWallet.mockResolvedValue({ address: expectedWallets.address, chainId: 1 })
    await vi.advanceTimersByTimeAsync(2_000)
    await result
    expect(deps.simulateWithdrawal).toHaveBeenCalledOnce()
    expect(deps.createWithdrawal).not.toHaveBeenCalled()
  })
})
