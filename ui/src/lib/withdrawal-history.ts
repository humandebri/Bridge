import { Principal } from "@dfinity/principal"
import { bytesToHex, hexToBytes, type Hex } from "viem"
import type { NotifyWithdrawalReceipt } from "@/generated/bridge.did"
import { NotifyWithdrawalCallError, notifyWithdrawalWithBrowserIdentity } from "@/lib/ic/withdrawal-notification-client"
import {
  ensurePendingWithdrawalConfirmation,
  markPendingConfirmationNotificationAttempt,
  markPendingConfirmationNotified,
  setPendingConfirmationNotificationFailure,
  type PendingConfirmationInput,
  type PendingNotificationFailure,
} from "@/lib/pending-confirmations"

export const WITHDRAWAL_LOG_CHUNK_SIZE = 2_000n
export const WITHDRAWAL_SCAN_CHUNKS_PER_STEP = 4

export interface WithdrawalDestinationAccount {
  owner: string
  subaccount: Uint8Array
}

export function decodeWithdrawalDestination(owner: Hex, subaccount: Hex): WithdrawalDestinationAccount {
  const ownerBytes = hexToBytes(owner)
  if (ownerBytes.length === 0
    || ownerBytes.length > 29
    || ownerBytes.length === 1 && ownerBytes[0] === 0x04) {
    throw new Error("Finalized withdrawal destination owner is invalid")
  }
  const subaccountBytes = hexToBytes(subaccount)
  if (subaccountBytes.length !== 32) throw new Error("Finalized withdrawal destination subaccount must be 32 bytes")
  return {
    owner: Principal.fromUint8Array(ownerBytes).toText(),
    subaccount: subaccountBytes,
  }
}

interface HistoryWithdrawalNotificationTarget {
  hash: Hex
  destinationAccount: WithdrawalDestinationAccount
}

interface HistoryWithdrawalNotificationDependencies {
  ensurePending: (value: PendingConfirmationInput) => Promise<void>
  notify: (transactionHash: Uint8Array) => Promise<NotifyWithdrawalReceipt>
  markNotified: (value: PendingConfirmationInput, withdrawalId: Hex) => Promise<void>
  markAttempt?: typeof markPendingConfirmationNotificationAttempt
  setFailure?: typeof setPendingConfirmationNotificationFailure
  delay?: (milliseconds: number) => Promise<void>
}

export async function notifyHistoryWithdrawal(
  target: HistoryWithdrawalNotificationTarget,
  dependencies: HistoryWithdrawalNotificationDependencies = {
    ensurePending: ensurePendingWithdrawalConfirmation,
    notify: notifyWithdrawalWithBrowserIdentity,
    markNotified: markPendingConfirmationNotified,
  },
  finalizedBlock = 0n,
): Promise<{ pending: PendingConfirmationInput; receipt: NotifyWithdrawalReceipt; withdrawalId: Uint8Array }> {
  const pending: PendingConfirmationInput = {
    kind: "withdrawal",
    transactionHash: target.hash,
    owner: target.destinationAccount.owner,
  }
  await dependencies.ensurePending(pending)
  const markAttempt = dependencies.markAttempt ?? markPendingConfirmationNotificationAttempt
  const setFailure = dependencies.setFailure ?? setPendingConfirmationNotificationFailure
  const wait = dependencies.delay ?? ((milliseconds: number) => new Promise<void>((resolve) => window.setTimeout(resolve, milliseconds)))
  await markAttempt(pending, "manual", finalizedBlock).catch(() => undefined)
  let receipt: NotifyWithdrawalReceipt
  try {
    receipt = await dependencies.notify(hexToBytes(target.hash))
  } catch (error) {
    if (historyNotificationAllowsShortRetry(error)) {
      await wait(5_000)
      await markAttempt(pending, "short-retry", finalizedBlock).catch(() => undefined)
      try {
        receipt = await dependencies.notify(hexToBytes(target.hash))
      } catch (retryError) {
        await setFailure(pending, historyNotificationFailure(retryError))
        throw retryError
      }
    } else {
      await setFailure(pending, historyNotificationFailure(error))
      throw error
    }
  }
  const withdrawalId = "Duplicate" in receipt ? receipt.Duplicate.withdrawal_id : receipt.Ingested.withdrawal_id
  const withdrawalIdBytes = Uint8Array.from(withdrawalId)
  await dependencies.markNotified(pending, bytesToHex(withdrawalIdBytes))
  return { pending, receipt, withdrawalId: withdrawalIdBytes }
}

function historyNotificationAllowsShortRetry(error: unknown): boolean {
  return error instanceof NotifyWithdrawalCallError ? error.code === "Busy" : true
}

function historyNotificationFailure(error: unknown): PendingNotificationFailure {
  const message = error instanceof Error ? error.message : "The IC notification failed."
  if (!(error instanceof NotifyWithdrawalCallError)) {
    return { code: "TransportError", message, disposition: "manual-retry" }
  }
  if (error.code === "TransactionNotConfirmed") {
    return { code: error.code, message, disposition: "manual-retry" }
  }
  if ([
    "AnonymousCaller",
    "BaseStateMismatch",
    "BridgeSignerMismatch",
    "InvalidTransactionHash",
    "LedgerFeeExceedsServiceFee",
    "TransactionReverted",
    "WithdrawalBeforeAdmissionBoundary",
    "WithdrawalConflict",
  ].includes(error.code)) return { code: error.code, message, disposition: "terminal" }
  return { code: error.code, message, disposition: "manual-retry" }
}

export interface FinalizedEventLog {
  blockNumber: bigint | null
  transactionHash: `0x${string}` | null
  logIndex: number | null
}

export interface WithdrawalLogScan<T extends FinalizedEventLog> {
  lastFinalizedBlock: bigint
  lastFinalizedBlockHash: `0x${string}`
  olderCursor: bigint | null
  reachedDeploymentBlock: boolean
  logs: T[]
}

export async function fetchInBatches<T, U>(items: T[], batchSize: number, fetchBatch: (batch: T[]) => Promise<U[]>): Promise<U[]> {
  if (!Number.isSafeInteger(batchSize) || batchSize <= 0) throw new Error("Batch size must be a positive integer")
  const results: U[] = []
  for (let offset = 0; offset < items.length; offset += batchSize) {
    const batch = items.slice(offset, offset + batchSize)
    const fetched = await fetchBatch(batch)
    if (fetched.length !== batch.length) throw new Error("Withdrawal batch response length does not match the request")
    results.push(...fetched)
  }
  return results
}

export async function fetchUniqueBlockTimestamps(blockNumbers: bigint[], fetchTimestamp: (blockNumber: bigint) => Promise<bigint>): Promise<Map<bigint, bigint>> {
  const unique = [...new Set(blockNumbers)]
  return new Map(await Promise.all(unique.map(async (blockNumber) => [blockNumber, await fetchTimestamp(blockNumber)] as const)))
}

interface ScanOptions<T extends FinalizedEventLog> {
  deploymentBlock: bigint
  finalizedBlock: bigint
  finalizedBlockHash: `0x${string}`
  previous?: WithdrawalLogScan<T>
  mode?: "refresh" | "older"
  fetchLogs: (fromBlock: bigint, toBlock: bigint) => Promise<T[]>
  fetchBlockHash?: (blockNumber: bigint) => Promise<`0x${string}`>
}

export async function scanWithdrawalLogs<T extends FinalizedEventLog>({ deploymentBlock, finalizedBlock, finalizedBlockHash, previous, mode = "refresh", fetchLogs, fetchBlockHash }: ScanOptions<T>): Promise<WithdrawalLogScan<T>> {
  if (finalizedBlock < deploymentBlock) return { lastFinalizedBlock: finalizedBlock, lastFinalizedBlockHash: finalizedBlockHash, olderCursor: null, reachedDeploymentBlock: true, logs: [] }

  if (mode === "older" && previous) {
    if (previous.olderCursor === null) return previous
    const scanned = await scanBackwards(previous.olderCursor, deploymentBlock, previous.logs, fetchLogs)
    return { ...previous, ...scanned }
  }

  if (previous && finalizedBlock === previous.lastFinalizedBlock && finalizedBlockHash === previous.lastFinalizedBlockHash) return previous

  if (previous && finalizedBlock > previous.lastFinalizedBlock) {
    const additions: T[] = []
    let fromBlock = previous.lastFinalizedBlock + 1n
    let calls = 0
    while (fromBlock <= finalizedBlock && calls < WITHDRAWAL_SCAN_CHUNKS_PER_STEP) {
      const toBlock = minBigInt(fromBlock + WITHDRAWAL_LOG_CHUNK_SIZE - 1n, finalizedBlock)
      additions.push(...await fetchLogs(fromBlock, toBlock))
      fromBlock = toBlock + 1n
      calls += 1
    }
    const logs = newestUnique([...previous.logs, ...additions])
    const checkpoint = fromBlock - 1n
    return {
      ...previous,
      lastFinalizedBlock: checkpoint,
      lastFinalizedBlockHash: checkpoint === finalizedBlock ? finalizedBlockHash : await fetchBlockHash?.(checkpoint) ?? previous.lastFinalizedBlockHash,
      olderCursor: previous.olderCursor,
      logs,
    }
  }

  const scanned = await scanBackwards(finalizedBlock, deploymentBlock, [], fetchLogs)
  return { lastFinalizedBlock: finalizedBlock, lastFinalizedBlockHash: finalizedBlockHash, ...scanned }
}

async function scanBackwards<T extends FinalizedEventLog>(startBlock: bigint, deploymentBlock: bigint, existing: T[], fetchLogs: ScanOptions<T>["fetchLogs"]) {
  const logs = [...existing]
  let toBlock = startBlock
  let calls = 0
  let reachedDeploymentBlock = false
  while (toBlock >= deploymentBlock && calls < WITHDRAWAL_SCAN_CHUNKS_PER_STEP) {
    const fromBlock = maxBigInt(deploymentBlock, toBlock - WITHDRAWAL_LOG_CHUNK_SIZE + 1n)
    logs.push(...await fetchLogs(fromBlock, toBlock))
    calls += 1
    if (fromBlock === deploymentBlock) {
      reachedDeploymentBlock = true
      break
    }
    toBlock = fromBlock - 1n
  }
  const merged = newestUnique(logs)
  const olderCursor = reachedDeploymentBlock ? null : toBlock
  return { olderCursor, reachedDeploymentBlock, logs: merged }
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
}

function minBigInt(left: bigint, right: bigint) { return left < right ? left : right }
function maxBigInt(left: bigint, right: bigint) { return left > right ? left : right }
