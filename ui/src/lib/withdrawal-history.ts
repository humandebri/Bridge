import { Principal } from "@icp-sdk/core/principal"
import { bytesToHex, hexToBytes, type Hex } from "viem"
import type { NotifyWithdrawalReceipt } from "@/generated/bridge.did"
import {
  NotifyWithdrawalCallError,
  notifyWithdrawalWithBrowserIdentity,
} from "@/lib/ic/withdrawal-notification-client"
import {
  ensurePendingWithdrawalConfirmation,
  markPendingConfirmationNotificationAttempt,
  markPendingConfirmationNotified,
  setPendingConfirmationNotificationFailure,
  type PendingConfirmationInput,
  type PendingNotificationFailure,
} from "@/lib/pending-confirmations"

export interface WithdrawalDestinationAccount {
  owner: string
  subaccount: Uint8Array
}

export function decodeWithdrawalDestination(
  owner: Hex,
  subaccount: Hex,
): WithdrawalDestinationAccount {
  const ownerBytes = hexToBytes(owner)
  if (
    ownerBytes.length === 0 ||
    ownerBytes.length > 29 ||
    (ownerBytes.length === 1 && ownerBytes[0] === 0x04)
  ) {
    throw new Error("Finalized withdrawal destination owner is invalid")
  }
  const subaccountBytes = hexToBytes(subaccount)
  if (subaccountBytes.length !== 32)
    throw new Error("Finalized withdrawal destination subaccount must be 32 bytes")
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
): Promise<{
  pending: PendingConfirmationInput
  receipt: NotifyWithdrawalReceipt
  withdrawalId: Uint8Array
}> {
  const pending: PendingConfirmationInput = {
    kind: "withdrawal",
    transactionHash: target.hash,
    owner: target.destinationAccount.owner,
  }
  await dependencies.ensurePending(pending)
  const markAttempt = dependencies.markAttempt ?? markPendingConfirmationNotificationAttempt
  const setFailure = dependencies.setFailure ?? setPendingConfirmationNotificationFailure
  const wait =
    dependencies.delay ??
    ((milliseconds: number) =>
      new Promise<void>((resolve) => window.setTimeout(resolve, milliseconds)))
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
  const withdrawalId =
    "Duplicate" in receipt ? receipt.Duplicate.withdrawal_id : receipt.Ingested.withdrawal_id
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
  if (
    [
      "AnonymousCaller",
      "BaseStateMismatch",
      "BridgeSignerMismatch",
      "InvalidTransactionHash",
      "LedgerFeeExceedsServiceFee",
      "TransactionReverted",
      "WithdrawalBeforeAdmissionBoundary",
      "WithdrawalConflict",
    ].includes(error.code)
  )
    return { code: error.code, message, disposition: "terminal" }
  return { code: error.code, message, disposition: "manual-retry" }
}

export async function fetchInBatches<T, U>(
  items: T[],
  batchSize: number,
  fetchBatch: (batch: T[]) => Promise<U[]>,
): Promise<U[]> {
  if (!Number.isSafeInteger(batchSize) || batchSize <= 0)
    throw new Error("Batch size must be a positive integer")
  const results: U[] = []
  for (let offset = 0; offset < items.length; offset += batchSize) {
    const batch = items.slice(offset, offset + batchSize)
    const fetched = await fetchBatch(batch)
    if (fetched.length !== batch.length)
      throw new Error("Withdrawal batch response length does not match the request")
    results.push(...fetched)
  }
  return results
}
