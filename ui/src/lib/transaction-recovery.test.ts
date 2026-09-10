import { beforeEach, describe, expect, it, vi } from "vitest"
import { encodeAbiParameters, encodeEventTopics, encodeFunctionData, type Hex } from "viem"
import { bridgeAbi } from "@/generated/abi/bridge.generated"
import { withdrawalReceiptDetails } from "./transaction-recovery"
const mocks = vi.hoisted(() => ({
  receipt: vi.fn(),
  transaction: vi.fn(),
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
describe("withdrawal receipt validation", () => {
  it("reads_matching_withdrawal_receipt_details", async () => {
    await expect(withdrawalReceiptDetails(hash)).resolves.toMatchObject({
      hash,
      requester: sender,
      amount: 100n,
      amountOut: 90n,
    })
  })
  it("rejects_wrong_contract_call_amount_and_destination", async () => {
    mocks.transaction.mockResolvedValue({
      to: bridge,
      from: sender,
      input: encodeFunctionData({
        abi: bridgeAbi,
        functionName: "createWithdrawal",
        args: [101n, 20n, "0x01", subaccount],
      }),
    })
    await expect(withdrawalReceiptDetails(hash)).rejects.toThrow("does not match")
    await expect(withdrawalReceiptDetails(hash, "aaaaa-aa")).rejects.toThrow("destination")
    mocks.transaction.mockResolvedValue({ to: sender })
    await expect(withdrawalReceiptDetails(hash)).rejects.toThrow("another contract")
  })
})
