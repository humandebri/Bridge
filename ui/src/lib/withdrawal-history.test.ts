import { describe, expect, it, vi } from "vitest"
import { fetchInBatches, scanWithdrawalLogs, WITHDRAWAL_LOG_CHUNK_SIZE, WITHDRAWAL_SCAN_CHUNKS_PER_STEP } from "./withdrawal-history"

interface TestLog {
  blockNumber: bigint
  transactionHash: `0x${string}`
  logIndex: number
  id: number
}

const log = (id: number, blockNumber: bigint): TestLog => ({ id, blockNumber, transactionHash: `0x${id.toString(16)}`, logIndex: 0 })
const blockHash = (block: bigint): Promise<`0x${string}`> => {
  const hash: `0x${string}` = `0x${block.toString(16)}`
  return Promise.resolve(hash)
}

describe("withdrawal log scanning", () => {
  it("loads more than 20 canister views in ordered batches", async () => {
    const fetchBatch = vi.fn((batch: number[]) => Promise.resolve(batch.map((value) => `view-${value}`)))

    const result = await fetchInBatches(Array.from({ length: 21 }, (_, index) => index), 20, fetchBatch)

    expect(fetchBatch.mock.calls.map(([batch]) => batch.length)).toEqual([20, 1])
    expect(result).toEqual(Array.from({ length: 21 }, (_, index) => `view-${index}`))
  })

  it("keeps the older cursor when a bounded scan finds 20 events", async () => {
    const fetchLogs = vi.fn((fromBlock: bigint, toBlock: bigint) => Promise.resolve(fromBlock === 45_002n
      ? Array.from({ length: 20 }, (_, index) => log(index + 1, toBlock - BigInt(index)))
      : []))

    const result = await scanWithdrawalLogs({ deploymentBlock: 1n, finalizedBlock: 50_001n, finalizedBlockHash: "0x5001", fetchLogs })

    expect(fetchLogs).toHaveBeenCalledTimes(WITHDRAWAL_SCAN_CHUNKS_PER_STEP)
    expect(result.logs).toHaveLength(20)
    expect(result.lastFinalizedBlock).toBe(50_001n)
    expect(result.olderCursor).toBe(30_001n)
  })

  it("uses only newly confirmed ranges on refresh and deduplicates events", async () => {
    const existing = log(1, 100n)
    const duplicate = { ...existing }
    const added = log(2, 102n)
    const fetchLogs = vi.fn(() => Promise.resolve([duplicate, added]))

    const result = await scanWithdrawalLogs({
      deploymentBlock: 1n,
      finalizedBlock: 102n,
      finalizedBlockHash: "0x0102",
      previous: { lastFinalizedBlock: 100n, lastFinalizedBlockHash: "0x0100", olderCursor: null, reachedDeploymentBlock: true, logs: [existing] },
      fetchLogs,
    })

    expect(fetchLogs).toHaveBeenCalledWith(101n, 102n)
    expect(result.logs.map((entry) => entry.id)).toEqual([2, 1])
  })

  it("splits a large incremental range into 5,000-block RPC requests", async () => {
    const fetchLogs = vi.fn(() => Promise.resolve([] as TestLog[]))

    await scanWithdrawalLogs({
      deploymentBlock: 1n,
      finalizedBlock: WITHDRAWAL_LOG_CHUNK_SIZE * 2n + 1n,
      finalizedBlockHash: "0x1001",
      previous: { lastFinalizedBlock: 1n, lastFinalizedBlockHash: "0x0001", olderCursor: null, reachedDeploymentBlock: true, logs: [] as TestLog[] },
      fetchBlockHash: blockHash,
      fetchLogs,
    })

    expect(fetchLogs.mock.calls).toEqual([
      [2n, 5_001n],
      [5_002n, 10_001n],
    ])
  })

  it("caps each scan step and resumes older ranges from its cursor", async () => {
    const fetchLogs = vi.fn(() => Promise.resolve([] as TestLog[]))
    const finalizedBlock = WITHDRAWAL_LOG_CHUNK_SIZE * 10n

    const first = await scanWithdrawalLogs({ deploymentBlock: 1n, finalizedBlock, finalizedBlockHash: "0x50000", fetchLogs })
    expect(fetchLogs).toHaveBeenCalledTimes(WITHDRAWAL_SCAN_CHUNKS_PER_STEP)
    expect(first.olderCursor).toBe(finalizedBlock - WITHDRAWAL_LOG_CHUNK_SIZE * BigInt(WITHDRAWAL_SCAN_CHUNKS_PER_STEP))
    expect(first.reachedDeploymentBlock).toBe(false)

    fetchLogs.mockClear()
    const second = await scanWithdrawalLogs({ deploymentBlock: 1n, finalizedBlock, finalizedBlockHash: "0x50000", previous: first, mode: "older", fetchLogs })
    expect(fetchLogs).toHaveBeenCalledTimes(WITHDRAWAL_SCAN_CHUNKS_PER_STEP)
    expect(second.olderCursor).toBe(first.olderCursor! - WITHDRAWAL_LOG_CHUNK_SIZE * BigInt(WITHDRAWAL_SCAN_CHUNKS_PER_STEP))
  })

  it("caps a large finalized catch-up and resumes it on the next refresh", async () => {
    const fetchLogs = vi.fn(() => Promise.resolve([] as TestLog[]))
    const previous = { lastFinalizedBlock: 1n, lastFinalizedBlockHash: "0x0001" as const, olderCursor: null, reachedDeploymentBlock: true, logs: [] as TestLog[] }
    const finalizedBlock = WITHDRAWAL_LOG_CHUNK_SIZE * 10n

    const first = await scanWithdrawalLogs({ deploymentBlock: 1n, finalizedBlock, finalizedBlockHash: "0x50000", previous, fetchLogs, fetchBlockHash: blockHash })
    expect(fetchLogs).toHaveBeenCalledTimes(WITHDRAWAL_SCAN_CHUNKS_PER_STEP)
    expect(first.lastFinalizedBlock).toBe(1n + WITHDRAWAL_LOG_CHUNK_SIZE * BigInt(WITHDRAWAL_SCAN_CHUNKS_PER_STEP))

    fetchLogs.mockClear()
    const second = await scanWithdrawalLogs({ deploymentBlock: 1n, finalizedBlock, finalizedBlockHash: "0x50000", previous: first, fetchLogs, fetchBlockHash: blockHash })
    expect(fetchLogs).toHaveBeenCalledTimes(WITHDRAWAL_SCAN_CHUNKS_PER_STEP)
    expect(second.lastFinalizedBlock).toBe(first.lastFinalizedBlock + WITHDRAWAL_LOG_CHUNK_SIZE * BigInt(WITHDRAWAL_SCAN_CHUNKS_PER_STEP))
  })

  it("preserves the older cursor when a finalized refresh fills the visible history", async () => {
    const previous = { lastFinalizedBlock: 100n, lastFinalizedBlockHash: "0x0100" as const, olderCursor: 50n, reachedDeploymentBlock: false, logs: [] as TestLog[] }
    const result = await scanWithdrawalLogs({
      deploymentBlock: 1n,
      finalizedBlock: 101n,
      finalizedBlockHash: "0x0101",
      previous,
      fetchLogs: () => Promise.resolve(Array.from({ length: 20 }, (_, index) => log(index + 1, 101n))),
    })

    expect(result.logs).toHaveLength(20)
    expect(result.olderCursor).toBe(50n)
  })

  it("discards cached logs and rescans from the finalized head when its hash changes", async () => {
    const stale = log(1, 100n)
    const replacement = log(2, 100n)
    const fetchLogs = vi.fn(() => Promise.resolve([replacement]))

    const result = await scanWithdrawalLogs({
      deploymentBlock: 1n,
      finalizedBlock: 100n,
      finalizedBlockHash: "0xnew",
      previous: { lastFinalizedBlock: 100n, lastFinalizedBlockHash: "0xold", olderCursor: null, reachedDeploymentBlock: true, logs: [stale] },
      fetchLogs,
    })

    expect(fetchLogs).toHaveBeenCalledWith(1n, 100n)
    expect(result.logs).toEqual([replacement])
    expect(result.lastFinalizedBlockHash).toBe("0xnew")
  })

  it("restarts from the finalized head when an older scan loses its reorged checkpoint", async () => {
    const replacement = log(2, 100n)
    const fetchLogs = vi.fn(() => Promise.resolve([replacement]))

    const result = await scanWithdrawalLogs({
      deploymentBlock: 1n,
      finalizedBlock: 100n,
      finalizedBlockHash: "0xnew",
      previous: undefined,
      mode: "older",
      fetchLogs,
    })

    expect(fetchLogs).toHaveBeenCalledWith(1n, 100n)
    expect(result.logs).toEqual([replacement])
    expect(result.lastFinalizedBlockHash).toBe("0xnew")
  })

  it("continues scanning after more than 20 events have already been found", async () => {
    const previous = {
      lastFinalizedBlock: 50_001n,
      lastFinalizedBlockHash: "0x5001" as const,
      olderCursor: 30_001n,
      reachedDeploymentBlock: false,
      logs: Array.from({ length: 20 }, (_, index) => log(index + 1, 50_001n - BigInt(index))),
    }
    const fetchLogs = vi.fn(() => Promise.resolve([log(21, 30_000n)]))

    const result = await scanWithdrawalLogs({ deploymentBlock: 1n, finalizedBlock: 50_001n, finalizedBlockHash: "0x5001", previous, mode: "older", fetchLogs })

    expect(fetchLogs).toHaveBeenCalledTimes(WITHDRAWAL_SCAN_CHUNKS_PER_STEP)
    expect(result.logs).toHaveLength(21)
    expect(result.olderCursor).toBe(10_001n)
  })
})
