export const WITHDRAWAL_HISTORY_LIMIT = 20
export const WITHDRAWAL_LOG_CHUNK_SIZE = 5_000n

export interface FinalizedEventLog {
  blockNumber: bigint | null
  transactionHash: `0x${string}` | null
  logIndex: number | null
}

export interface WithdrawalLogScan<T extends FinalizedEventLog> {
  lastFinalizedBlock: bigint
  logs: T[]
}

interface ScanOptions<T extends FinalizedEventLog> {
  deploymentBlock: bigint
  finalizedBlock: bigint
  previous?: WithdrawalLogScan<T>
  fetchLogs: (fromBlock: bigint, toBlock: bigint) => Promise<T[]>
}

export async function scanWithdrawalLogs<T extends FinalizedEventLog>({ deploymentBlock, finalizedBlock, previous, fetchLogs }: ScanOptions<T>): Promise<WithdrawalLogScan<T>> {
  if (finalizedBlock < deploymentBlock) return { lastFinalizedBlock: finalizedBlock, logs: [] }
  if (previous && finalizedBlock === previous.lastFinalizedBlock) return previous

  if (previous && finalizedBlock > previous.lastFinalizedBlock) {
    const additions: T[] = []
    let fromBlock = previous.lastFinalizedBlock + 1n
    while (fromBlock <= finalizedBlock) {
      const toBlock = minBigInt(fromBlock + WITHDRAWAL_LOG_CHUNK_SIZE - 1n, finalizedBlock)
      additions.push(...await fetchLogs(fromBlock, toBlock))
      fromBlock = toBlock + 1n
    }
    return { lastFinalizedBlock: finalizedBlock, logs: newestUnique([...previous.logs, ...additions]) }
  }

  const logs: T[] = []
  let toBlock = finalizedBlock
  while (toBlock >= deploymentBlock && logs.length < WITHDRAWAL_HISTORY_LIMIT) {
    const fromBlock = maxBigInt(deploymentBlock, toBlock - WITHDRAWAL_LOG_CHUNK_SIZE + 1n)
    logs.push(...await fetchLogs(fromBlock, toBlock))
    if (fromBlock === deploymentBlock) break
    toBlock = fromBlock - 1n
  }
  return { lastFinalizedBlock: finalizedBlock, logs: newestUnique(logs) }
}

function newestUnique<T extends FinalizedEventLog>(logs: T[]): T[] {
  const unique = new Map<string, T>()
  for (const log of logs) {
    if (log.blockNumber === null || log.transactionHash === null || log.logIndex === null) throw new Error("Finalized withdrawal log metadata is incomplete")
    unique.set(`${log.transactionHash}:${log.logIndex}`, log)
  }
  return [...unique.values()]
    .sort((left, right) => {
      if (right.blockNumber !== left.blockNumber) return (right.blockNumber as bigint) > (left.blockNumber as bigint) ? 1 : -1
      return (right.logIndex as number) - (left.logIndex as number)
    })
    .slice(0, WITHDRAWAL_HISTORY_LIMIT)
}

function minBigInt(left: bigint, right: bigint) { return left < right ? left : right }
function maxBigInt(left: bigint, right: bigint) { return left > right ? left : right }
