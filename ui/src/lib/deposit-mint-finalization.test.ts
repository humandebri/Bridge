import { describe, expect, it } from "vitest"
import { encodeAbiParameters, encodeEventTopics } from "viem"
import { bridgeAbi } from "@/generated/abi/bridge.generated"
import {
  DEPOSIT_MINT_LOG_CHUNK_SIZE,
  DEPOSIT_MINT_SCAN_CHUNKS_PER_STEP,
  depositMintEventMatches,
  depositMintFinalizationStatus,
  receiptContainsExactDepositMint,
  scanDepositMintLogs,
  type DepositMintLogScan,
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
  expect(depositMintEventMatches(expected, { ...expected, depositId: `0x${"44".repeat(32)}` })).toBe(false)
  expect(depositMintEventMatches(expected, { ...expected, recipient: `0x${"44".repeat(20)}` })).toBe(false)
  expect(depositMintEventMatches(expected, { ...expected, authorizationDigest: `0x${"44".repeat(32)}` })).toBe(false)
  expect(depositMintEventMatches(expected, { ...expected, grossAmount: expected.grossAmount + 1n })).toBe(false)
  expect(depositMintEventMatches(expected, { ...expected, serviceFee: expected.serviceFee + 1n })).toBe(false)
  expect(depositMintEventMatches(expected, { ...expected, mintedAmount: expected.mintedAmount + 1n })).toBe(false)
}

describe("depositMintEventMatches", () => {
  it("accepts only the exact finalized DepositMinted payload", accepts_only_the_exact_finalized_DepositMinted_payload)
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
    expect(receiptContainsExactDepositMint(expected, [{
      address: bridgeAddress,
      topics,
      data,
    }], bridgeAddress)).toBe(true)

    expect(receiptContainsExactDepositMint({
      ...expected,
      authorizationDigest: `0x${"44".repeat(32)}`,
    }, [{
      address: bridgeAddress,
      topics,
      data,
    }], bridgeAddress)).toBe(false)

    expect(receiptContainsExactDepositMint({
      ...expected,
      mintedAmount: expected.mintedAmount + 1n,
    }, [{
      address: bridgeAddress,
      topics,
      data,
    }], bridgeAddress)).toBe(false)

    expect(receiptContainsExactDepositMint(expected, [{
      address: bridgeAddress,
      topics,
      data,
    }], `0x${"88".repeat(20)}`)).toBe(false)
  }

  it("accepts only an exact event emitted by the configured bridge", accepts_only_an_exact_receipt_event_from_configured_bridge)

  it("does not accept a successful receipt without DepositMinted", () => {
    expect(receiptContainsExactDepositMint(expected, [], bridgeAddress)).toBe(false)
  })
})

describe("depositMintFinalizationStatus", () => {
  const scan = (olderCursor: bigint | null, logs: DepositMintLogScan["logs"] = []): DepositMintLogScan => ({
    lastFinalizedBlock: 30_000n,
    lastFinalizedBlockHash: `0x${"aa".repeat(32)}`,
    observedFinalizedBlock: 30_000n,
    olderCursor,
    reachedDeploymentBlock: olderCursor === null,
    logs,
  })

  it("retains an exact finalized mint even while a refresh fails", () => {
    expect(depositMintFinalizationStatus({
      expected,
      authorizationBlock: 10_000n,
      scan: scan(20_000n, [{
        blockNumber: 29_000n,
        transactionHash: `0x${"bb".repeat(32)}`,
        logIndex: 0,
        args: expected,
      }]),
      queryState: "unavailable",
    })).toBe("minted")
  })

  it("only reports absence after the authorization origin is covered", () => {
    expect(depositMintFinalizationStatus({
      expected,
      authorizationBlock: 20_001n,
      scan: scan(20_000n),
      queryState: "ready",
    })).toBe("absent")
    expect(depositMintFinalizationStatus({
      expected,
      authorizationBlock: 20_000n,
      scan: scan(20_000n),
      queryState: "ready",
    })).toBe("checking")
  })

  it("does not reuse absence while refreshing or after an RPC failure", () => {
    const complete = scan(null)
    expect(depositMintFinalizationStatus({
      expected,
      authorizationBlock: 10_000n,
      scan: complete,
      queryState: "checking",
    })).toBe("checking")
    expect(depositMintFinalizationStatus({
      expected,
      authorizationBlock: 10_000n,
      scan: complete,
      queryState: "unavailable",
    })).toBe("unavailable")
  })
})

describe("scanDepositMintLogs", () => {
  it("uses one four-chunk budget for head catch-up and backward coverage", async () => {
    const ranges: Array<[bigint, bigint]> = []
    const previous: DepositMintLogScan = {
      lastFinalizedBlock: 30_000n,
      lastFinalizedBlockHash: `0x${"aa".repeat(32)}`,
      observedFinalizedBlock: 30_000n,
      olderCursor: 10_000n,
      reachedDeploymentBlock: false,
      logs: [],
    }
    const result = await scanDepositMintLogs({
      deploymentBlock: 1n,
      finalizedBlock: 31_000n,
      finalizedBlockHash: `0x${"bb".repeat(32)}`,
      previous,
      fetchLogs: (fromBlock, toBlock) => {
        ranges.push([fromBlock, toBlock])
        return Promise.resolve([])
      },
      fetchBlockHash: (): Promise<`0x${string}`> =>
        Promise.resolve<`0x${string}`>(`0x${"cc".repeat(32)}`),
    })

    expect(ranges).toEqual([
      [30_001n, 31_000n],
      [8_001n, 10_000n],
      [6_001n, 8_000n],
      [4_001n, 6_000n],
    ])
    expect(result.lastFinalizedBlock).toBe(31_000n)
    expect(result.olderCursor).toBe(4_000n)
  })

  it("keeps absence in checking state while a large finalized gap is catching up", async () => {
    const result = await scanDepositMintLogs({
      deploymentBlock: 1n,
      finalizedBlock: 60_000n,
      finalizedBlockHash: `0x${"bb".repeat(32)}`,
      previous: {
        lastFinalizedBlock: 30_000n,
        lastFinalizedBlockHash: `0x${"aa".repeat(32)}`,
        observedFinalizedBlock: 30_000n,
        olderCursor: null,
        reachedDeploymentBlock: true,
        logs: [],
      },
      fetchLogs: () => Promise.resolve([]),
      fetchBlockHash: (): Promise<`0x${string}`> =>
        Promise.resolve<`0x${string}`>(`0x${"cc".repeat(32)}`),
    })
    expect(result.lastFinalizedBlock).toBe(30_000n + DEPOSIT_MINT_LOG_CHUNK_SIZE * BigInt(DEPOSIT_MINT_SCAN_CHUNKS_PER_STEP))
    expect(depositMintFinalizationStatus({
      expected,
      authorizationBlock: 1n,
      scan: result,
      queryState: "ready",
    })).toBe("checking")
  })
})
