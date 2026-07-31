import type { FinalizedEventLog } from "./withdrawal-history"
import { decodeEventLog, type Hex } from "viem"
import { bridgeAbi } from "@/generated/abi/bridge.generated"

export const DEPOSIT_MINT_LOG_CHUNK_SIZE = 2_000n
export const DEPOSIT_MINT_SCAN_CHUNKS_PER_STEP = 4

export interface ExpectedDepositMint {
  depositId: `0x${string}`
  recipient: `0x${string}`
  authorizationDigest: `0x${string}`
  grossAmount: bigint
  serviceFee: bigint
  mintedAmount: bigint
}

export interface ObservedDepositMint {
  depositId: `0x${string}`
  recipient: `0x${string}`
  authorizationDigest: `0x${string}`
  grossAmount: bigint
  serviceFee: bigint
  mintedAmount: bigint
}

export interface FinalizedDepositMintLog extends FinalizedEventLog {
  args: ObservedDepositMint
}

export interface DepositMintLogScan {
  lastFinalizedBlock: bigint
  lastFinalizedBlockHash: `0x${string}`
  observedFinalizedBlock: bigint
  olderCursor: bigint | null
  reachedDeploymentBlock: boolean
  logs: FinalizedDepositMintLog[]
}
export type DepositMintFinalizationStatus = "checking" | "minted" | "absent" | "unavailable"

export function depositMintEventMatches(
  expected: ExpectedDepositMint,
  observed: ObservedDepositMint,
): boolean {
  return observed.depositId.toLowerCase() === expected.depositId.toLowerCase()
    && observed.recipient.toLowerCase() === expected.recipient.toLowerCase()
    && observed.authorizationDigest.toLowerCase() === expected.authorizationDigest.toLowerCase()
    && observed.grossAmount === expected.grossAmount
    && observed.serviceFee === expected.serviceFee
    && observed.mintedAmount === expected.mintedAmount
}

export function receiptContainsExactDepositMint(
  expected: ExpectedDepositMint,
  logs: readonly { address: Hex; data: Hex; topics: readonly Hex[] }[],
  expectedBridgeAddress: Hex,
): boolean {
  return logs.some((log) => {
    if (log.address.toLowerCase() !== expectedBridgeAddress.toLowerCase()) return false
    try {
      const decoded = decodeEventLog({
        abi: bridgeAbi,
        eventName: "DepositMinted",
        data: log.data,
        topics: log.topics as [signature: Hex, ...args: Hex[]],
        strict: true,
      })
      const args = decoded.args
      return depositMintEventMatches(expected, {
        depositId: args.depositId,
        recipient: args.recipient,
        authorizationDigest: args.authorizationDigest,
        grossAmount: args.grossAmount,
        serviceFee: args.serviceFee,
        mintedAmount: args.mintedAmount,
      })
    } catch {
      return false
    }
  })
}

export function depositMintFinalizationStatus({
  expected,
  authorizationBlock,
  scan,
  queryState,
}: {
  expected: ExpectedDepositMint
  authorizationBlock: bigint
  scan?: DepositMintLogScan
  queryState: "ready" | "checking" | "unavailable"
}): DepositMintFinalizationStatus {
  if (scan?.logs.some((log) => depositMintEventMatches(expected, log.args))) return "minted"
  if (queryState === "unavailable") return "unavailable"
  if (queryState === "checking" || !scan || scan.lastFinalizedBlock < scan.observedFinalizedBlock) {
    return "checking"
  }
  return scan.olderCursor === null || authorizationBlock > scan.olderCursor ? "absent" : "checking"
}

export async function scanDepositMintLogs({
  deploymentBlock,
  finalizedBlock,
  finalizedBlockHash,
  previous,
  fetchLogs,
  fetchBlockHash,
}: {
  deploymentBlock: bigint
  finalizedBlock: bigint
  finalizedBlockHash: `0x${string}`
  previous?: DepositMintLogScan
  fetchLogs: (fromBlock: bigint, toBlock: bigint) => Promise<FinalizedDepositMintLog[]>
  fetchBlockHash: (blockNumber: bigint) => Promise<`0x${string}`>
}): Promise<DepositMintLogScan> {
  if (finalizedBlock < deploymentBlock) {
    return {
      lastFinalizedBlock: finalizedBlock,
      lastFinalizedBlockHash: finalizedBlockHash,
      observedFinalizedBlock: finalizedBlock,
      olderCursor: null,
      reachedDeploymentBlock: true,
      logs: [],
    }
  }

  let calls = 0
  let checkpoint = previous?.lastFinalizedBlock ?? finalizedBlock
  let checkpointHash = previous?.lastFinalizedBlockHash ?? finalizedBlockHash
  let olderCursor: bigint | null = previous ? previous.olderCursor : finalizedBlock
  let reachedDeploymentBlock = previous?.reachedDeploymentBlock ?? false
  const logs = [...(previous?.logs ?? [])]

  if (previous && checkpoint < finalizedBlock) {
    let fromBlock = checkpoint + 1n
    while (fromBlock <= finalizedBlock && calls < DEPOSIT_MINT_SCAN_CHUNKS_PER_STEP) {
      const toBlock = minBigInt(fromBlock + DEPOSIT_MINT_LOG_CHUNK_SIZE - 1n, finalizedBlock)
      logs.push(...await fetchLogs(fromBlock, toBlock))
      checkpoint = toBlock
      fromBlock = toBlock + 1n
      calls += 1
    }
    checkpointHash = checkpoint === finalizedBlock
      ? finalizedBlockHash
      : await fetchBlockHash(checkpoint)
  }

  while (olderCursor !== null
    && olderCursor >= deploymentBlock
    && calls < DEPOSIT_MINT_SCAN_CHUNKS_PER_STEP) {
    const fromBlock = maxBigInt(
      deploymentBlock,
      olderCursor - DEPOSIT_MINT_LOG_CHUNK_SIZE + 1n,
    )
    logs.push(...await fetchLogs(fromBlock, olderCursor))
    calls += 1
    if (fromBlock === deploymentBlock) {
      reachedDeploymentBlock = true
      olderCursor = null
    } else {
      olderCursor = fromBlock - 1n
    }
  }

  return {
    lastFinalizedBlock: checkpoint,
    lastFinalizedBlockHash: checkpointHash,
    observedFinalizedBlock: finalizedBlock,
    olderCursor,
    reachedDeploymentBlock,
    logs: newestUnique(logs),
  }
}

function newestUnique(logs: FinalizedDepositMintLog[]): FinalizedDepositMintLog[] {
  const unique = new Map<string, FinalizedDepositMintLog>()
  for (const log of logs) {
    if (log.blockNumber === null || log.transactionHash === null || log.logIndex === null) {
      throw new Error("Finalized deposit mint log metadata is incomplete")
    }
    unique.set(`${log.transactionHash}:${log.logIndex}`, log)
  }
  return [...unique.values()].sort((left, right) => {
    if (right.blockNumber !== left.blockNumber) {
      return (right.blockNumber as bigint) > (left.blockNumber as bigint) ? 1 : -1
    }
    return (right.logIndex as number) - (left.logIndex as number)
  })
}

function minBigInt(left: bigint, right: bigint): bigint {
  return left < right ? left : right
}

function maxBigInt(left: bigint, right: bigint): bigint {
  return left > right ? left : right
}
