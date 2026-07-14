import { describe, expect, it, vi } from "vitest"
import { scanWithdrawalLogs, WITHDRAWAL_LOG_CHUNK_SIZE } from "./withdrawal-history"

interface TestLog {
  blockNumber: bigint
  transactionHash: `0x${string}`
  logIndex: number
  id: number
}

const log = (id: number, blockNumber: bigint): TestLog => ({ id, blockNumber, transactionHash: `0x${id.toString(16)}`, logIndex: 0 })

describe("withdrawal log scanning", () => {
  it("scans finalized blocks backwards in bounded chunks and stops after 20 events", async () => {
    const fetchLogs = vi.fn((fromBlock: bigint, toBlock: bigint) => Promise.resolve(fromBlock === 5_002n
      ? Array.from({ length: 20 }, (_, index) => log(index + 1, toBlock - BigInt(index)))
      : []))

    const result = await scanWithdrawalLogs({ deploymentBlock: 1n, finalizedBlock: 10_001n, fetchLogs })

    expect(fetchLogs.mock.calls).toEqual([[5_002n, 10_001n]])
    expect(result.logs).toHaveLength(20)
    expect(result.lastFinalizedBlock).toBe(10_001n)
  })

  it("uses only newly finalized ranges on refresh and deduplicates events", async () => {
    const existing = log(1, 100n)
    const duplicate = { ...existing }
    const added = log(2, 102n)
    const fetchLogs = vi.fn(() => Promise.resolve([duplicate, added]))

    const result = await scanWithdrawalLogs({
      deploymentBlock: 1n,
      finalizedBlock: 102n,
      previous: { lastFinalizedBlock: 100n, logs: [existing] },
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
      previous: { lastFinalizedBlock: 1n, logs: [] as TestLog[] },
      fetchLogs,
    })

    expect(fetchLogs.mock.calls).toEqual([
      [2n, 5_001n],
      [5_002n, 10_001n],
    ])
  })
})
