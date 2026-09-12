import { describe, expect, it } from "vitest"
import { encodeAbiParameters, encodeEventTopics } from "viem"
import { bridgeAbi } from "@/generated/abi/bridge.generated"
import {
  depositMintEventMatches,
  exactMintReceiptFinalization,
  receiptContainsExactDepositMint,
  type ExpectedDepositMint,
} from "./deposit-mint-finalization"

const expected: ExpectedDepositMint = {
  depositId: `0x${"11".repeat(32)}`,
  recipient: `0x${"22".repeat(20)}`,
  authorizationDigest: `0x${"33".repeat(32)}`,
  grossAmount: 20_000n,
  serviceFee: 7n,
  mintedAmount: 19_993n,
}

function accepts_only_the_exact_finalized_DepositMinted_payload() {
  expect(depositMintEventMatches(expected, { ...expected })).toBe(true)
  expect(
    depositMintEventMatches(expected, { ...expected, depositId: `0x${"44".repeat(32)}` }),
  ).toBe(false)
  expect(
    depositMintEventMatches(expected, { ...expected, recipient: `0x${"44".repeat(20)}` }),
  ).toBe(false)
  expect(
    depositMintEventMatches(expected, { ...expected, authorizationDigest: `0x${"44".repeat(32)}` }),
  ).toBe(false)
  expect(
    depositMintEventMatches(expected, { ...expected, grossAmount: expected.grossAmount + 1n }),
  ).toBe(false)
  expect(
    depositMintEventMatches(expected, { ...expected, serviceFee: expected.serviceFee + 1n }),
  ).toBe(false)
  expect(
    depositMintEventMatches(expected, { ...expected, mintedAmount: expected.mintedAmount + 1n }),
  ).toBe(false)
}

describe("depositMintEventMatches", () => {
  it(
    "accepts only the exact finalized DepositMinted payload",
    accepts_only_the_exact_finalized_DepositMinted_payload,
  )
})

describe("receiptContainsExactDepositMint", () => {
  const bridgeAddress = `0x${"77".repeat(20)}` as const
  const topics = encodeEventTopics({
    abi: bridgeAbi,
    eventName: "DepositMinted",
    args: {
      depositId: expected.depositId,
      recipient: expected.recipient,
      authorizationDigest: expected.authorizationDigest,
    },
  }) as readonly `0x${string}`[]
  const data = encodeAbiParameters(
    [
      { type: "uint256", name: "grossAmount" },
      { type: "uint256", name: "serviceFee" },
      { type: "uint256", name: "mintedAmount" },
    ],
    [expected.grossAmount, expected.serviceFee, expected.mintedAmount],
  )

  function accepts_only_an_exact_receipt_event_from_configured_bridge() {
    expect(
      receiptContainsExactDepositMint(
        expected,
        [
          {
            address: bridgeAddress,
            topics,
            data,
          },
        ],
        bridgeAddress,
      ),
    ).toBe(true)

    expect(
      receiptContainsExactDepositMint(
        {
          ...expected,
          authorizationDigest: `0x${"44".repeat(32)}`,
        },
        [
          {
            address: bridgeAddress,
            topics,
            data,
          },
        ],
        bridgeAddress,
      ),
    ).toBe(false)

    expect(
      receiptContainsExactDepositMint(
        {
          ...expected,
          mintedAmount: expected.mintedAmount + 1n,
        },
        [
          {
            address: bridgeAddress,
            topics,
            data,
          },
        ],
        bridgeAddress,
      ),
    ).toBe(false)

    expect(
      receiptContainsExactDepositMint(
        expected,
        [
          {
            address: bridgeAddress,
            topics,
            data,
          },
        ],
        `0x${"88".repeat(20)}`,
      ),
    ).toBe(false)
  }

  it(
    "accepts only an exact event emitted by the configured bridge",
    accepts_only_an_exact_receipt_event_from_configured_bridge,
  )

  it("does not accept a successful receipt without DepositMinted", () => {
    expect(receiptContainsExactDepositMint(expected, [], bridgeAddress)).toBe(false)
  })

  function accepts_only_an_exact_canonical_finalized_mint_receipt() {
    const blockHash = `0x${"aa".repeat(32)}` as const
    const receipt = {
      status: "success" as const,
      blockNumber: 99n,
      blockHash,
      logs: [{ address: bridgeAddress, topics, data }],
    }
    const input = {
      expected,
      expectedBridgeAddress: bridgeAddress,
      receipt,
      finalizedBlockNumber: 100n,
      canonicalReceiptBlockHash: blockHash,
    }

    expect(exactMintReceiptFinalization(input)).toBe("finalized")
    expect(
      exactMintReceiptFinalization({
        ...input,
        finalizedBlockNumber: 98n,
      }),
    ).toBe("pending")
    expect(
      exactMintReceiptFinalization({
        ...input,
        canonicalReceiptBlockHash: `0x${"bb".repeat(32)}`,
      }),
    ).toBe("pending")
    expect(
      exactMintReceiptFinalization({
        ...input,
        receipt: { ...receipt, logs: [] },
      }),
    ).toBe("conflict")
    expect(
      exactMintReceiptFinalization({
        ...input,
        receipt: { ...receipt, status: "reverted" },
      }),
    ).toBe("reverted")
  }

  it(
    "accepts only an exact canonical finalized mint receipt",
    accepts_only_an_exact_canonical_finalized_mint_receipt,
  )
})
