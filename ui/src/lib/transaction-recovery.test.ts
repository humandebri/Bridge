import { beforeEach, describe, expect, it, vi } from "vitest"
import { encodeAbiParameters, encodeEventTopics, encodeFunctionData, type Hex } from "viem"
import { bridgeAbi } from "@/generated/abi/bridge.generated"
import { recoverTransaction, withdrawalReceiptDetails } from "./transaction-recovery"
const mocks = vi.hoisted(() => ({
  receipt: vi.fn(),
  transaction: vi.fn(),
  pending: vi.fn(),
  mint: vi.fn(),
}))
const bridge = `0x${"44".repeat(20)}` as Hex
const sender = `0x${"55".repeat(20)}` as Hex
const hash = `0x${"66".repeat(32)}` as Hex
const subaccount = `0x${"00".repeat(32)}` as Hex
vi.mock("@/config/profile", () => ({
  deploymentProfile: { bridgeAddress: `0x${"44".repeat(20)}` },
}))
vi.mock("@/lib/evm/client", () => ({ basePublicClient: { getTransaction: mocks.transaction } }))
vi.mock("@/lib/base-transaction-observation", () => ({
  readBaseReceipt: mocks.receipt,
  sharedBaseRead: (_key: string, read: () => unknown) => read(),
}))
vi.mock("@/lib/ic/bridge", () => ({ createBridgeActor: vi.fn() }))
vi.mock("@/lib/pending-confirmations", () => ({
  ensurePendingWithdrawalConfirmation: mocks.pending,
  savePendingMint: mocks.mint,
}))
beforeEach(() => {
  vi.resetAllMocks()
  mocks.receipt.mockResolvedValue({
    status: "success",
    blockNumber: 42n,
    logs: [
      {
        address: bridge,
        logIndex: 0,
        topics: encodeEventTopics({
          abi: bridgeAbi,
          eventName: "WithdrawalCommitted",
          args: { withdrawalId: 1n, requester: sender },
        }),
        data: encodeAbiParameters(
          [
            { type: "uint256" },
            { type: "uint256" },
            { type: "uint256" },
            { type: "uint256" },
            { type: "bytes" },
            { type: "bytes32" },
          ],
          [100n, 20n, 10n, 90n, "0x01", subaccount],
        ),
      },
    ],
  })
  mocks.transaction.mockResolvedValue({
    to: bridge,
    from: sender,
    input: encodeFunctionData({
      abi: bridgeAbi,
      functionName: "createWithdrawal",
      args: [100n, 20n, "0x01", subaccount],
    }),
  })
})
describe("transaction hash recovery", () => {
  it("recovers_only_a_matching_withdrawal_for_its_sender_or_ic_recipient", async () => {
    const details = await withdrawalReceiptDetails(hash)
    await expect(recoverTransaction(hash, { evm: sender })).resolves.toContain("restored")
    expect(mocks.pending).toHaveBeenCalledWith({
      kind: "withdrawal",
      transactionHash: hash,
      owner: details.destinationAccount.owner,
    })
    await expect(
      recoverTransaction(hash, { ic: details.destinationAccount.owner }),
    ).resolves.toContain("restored")
    const writes = mocks.pending.mock.calls.length
    await expect(recoverTransaction(hash, { evm: bridge })).rejects.toThrow("Connect the sender")
    expect(mocks.pending).toHaveBeenCalledTimes(writes)
    expect(mocks.mint).not.toHaveBeenCalled()
  })
  it("rejects_wrong_contract_call_amount_and_destination_without_queuing", async () => {
    mocks.transaction.mockResolvedValue({
      to: bridge,
      from: sender,
      input: encodeFunctionData({
        abi: bridgeAbi,
        functionName: "createWithdrawal",
        args: [101n, 20n, "0x01", subaccount],
      }),
    })
    await expect(recoverTransaction(hash, { evm: sender })).rejects.toThrow("does not match")
    await expect(withdrawalReceiptDetails(hash, "aaaaa-aa")).rejects.toThrow("destination")
    mocks.transaction.mockResolvedValue({ to: sender })
    await expect(recoverTransaction(hash, { evm: sender })).rejects.toThrow("another contract")
    expect(mocks.pending).not.toHaveBeenCalled()
  })
})
