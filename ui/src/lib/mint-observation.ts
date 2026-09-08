import { hexToBytes, type Hex } from "viem"
import { deploymentProfile } from "@/config/profile"
import type { DepositView } from "@/generated/bridge.did"
import { bridgeAbi } from "@/generated/abi/bridge.generated"
import { basePublicClient } from "@/lib/evm/client"
import { readBaseBlock, readBaseReceipt, sharedBaseRead } from "@/lib/base-transaction-observation"
import {
  receiptContainsExactDepositMint,
  exactMintReceiptFinalization,
  type ExpectedDepositMint,
} from "@/lib/deposit-mint-finalization"
import { createBridgeActor } from "@/lib/ic/bridge"
import { getWithdrawalNotificationIdentity } from "@/lib/ic/withdrawal-notification-client"
import { withBrowserLock } from "@/lib/browser-lock"
import { readPendingMint, type PendingMint } from "@/lib/pending-confirmations"

export interface MintObservation {
  status: "submitted" | "success" | "reverted" | "conflict" | "processed" | "unsubmitted"
  transactionHash?: Hex
  blockNumber?: bigint
  finalized: boolean
  recorded: boolean
  finalizedTimestamp?: bigint
  unavailable?: boolean
  notificationError?: string
}
interface ObservationEntry {
  fingerprint?: string
  observation: MintObservation
  finalityRunning?: boolean
}
const observations = new Map<string, ObservationEntry>()
const completedNotifications = new Set<string>()
const bytesHex = (bytes: Uint8Array | number[]): Hex =>
  `0x${Array.from(bytes, (n) => n.toString(16).padStart(2, "0")).join("")}`

export async function observeMint(pending: PendingMint): Promise<MintObservation> {
  const key = [
    deploymentProfile.chainId,
    deploymentProfile.bridgeAddress,
    deploymentProfile.deploymentInstanceId,
    pending.depositId,
    pending.transactionHash,
    pending.recipient,
    pending.authorizationDigest,
    pending.grossAmount,
    pending.chargedServiceFee,
    pending.mintedAmount,
  ]
    .join(":")
    .toLowerCase()
  return sharedBaseRead(`mint:${key}`, async () => {
    const expected = {
      depositId: pending.depositId,
      recipient: pending.recipient,
      authorizationDigest: pending.authorizationDigest,
      grossAmount: BigInt(pending.grossAmount),
      serviceFee: BigInt(pending.chargedServiceFee),
      mintedAmount: BigInt(pending.mintedAmount),
    }
    let entry = observations.get(key) ?? {
      observation: {
        status: "submitted" as const,
        transactionHash: pending.transactionHash,
        finalized: false,
        recorded: false,
      },
    }
    try {
      const receipt = await readBaseReceipt(pending.transactionHash)
      const included =
        receipt.status === "success" &&
        receiptContainsExactDepositMint(
          expected,
          receipt.logs,
          deploymentProfile.bridgeAddress as Hex,
        )
      const fingerprint = `${receipt.blockHash}:${receipt.blockNumber}:${receipt.status}:${included}`
      if (entry.fingerprint !== fingerprint) {
        entry = {
          fingerprint,
          observation: {
            status: receipt.status === "reverted" ? "reverted" : included ? "success" : "conflict",
            transactionHash: pending.transactionHash,
            blockNumber: receipt.blockNumber,
            finalized: false,
            recorded: false,
          },
        }
      }
      observations.set(key, entry)
      // Publish inclusion before any finalized-head request can delay the caller.
      // The next poll takes a new snapshot of the background result.
      const snapshot = { ...entry.observation }
      if (!entry.finalityRunning) {
        entry.finalityRunning = true
        void observeMintFinality(key, entry, pending, expected, receipt).finally(() => {
          entry.finalityRunning = false
        })
      }
      return snapshot
    } catch (error) {
      const missing =
        error &&
        typeof error === "object" &&
        "name" in error &&
        error.name === "TransactionReceiptNotFoundError"
      if (missing) {
        // Replacing the entry also invalidates work for the disappeared receipt.
        entry = {
          observation: {
            status: "submitted",
            transactionHash: pending.transactionHash,
            finalized: false,
            recorded: false,
          },
        }
      } else {
        entry.observation = { ...entry.observation, unavailable: true }
      }
      observations.set(key, entry)
      return { ...entry.observation }
    }
  })
}

async function observeMintFinality(
  key: string,
  entry: ObservationEntry,
  pending: PendingMint,
  expected: ExpectedDepositMint,
  receipt: Awaited<ReturnType<typeof readBaseReceipt>>,
): Promise<void> {
  const isCurrent = () => observations.get(key) === entry
  const publish = (patch: Partial<MintObservation>) => {
    if (isCurrent()) entry.observation = { ...entry.observation, ...patch }
  }
  try {
    const finalized = await readBaseBlock("finalized")
    if (!isCurrent()) return
    publish({ finalizedTimestamp: finalized.timestamp, unavailable: false })
    if (finalized.number === null || finalized.number < receipt.blockNumber) return
    const canonical = await readBaseBlock(receipt.blockNumber)
    if (!isCurrent()) return
    const result = exactMintReceiptFinalization({
      expected,
      expectedBridgeAddress: deploymentProfile.bridgeAddress as Hex,
      receipt,
      finalizedBlockNumber: finalized.number,
      canonicalReceiptBlockHash: canonical.hash,
    })
    if (canonical.hash?.toLowerCase() !== receipt.blockHash.toLowerCase()) {
      // Keep this receipt invalidated until canonical evidence changes, even if
      // the RPC keeps returning the same orphaned receipt on subsequent polls.
      observations.set(key, {
        fingerprint: entry.fingerprint,
        observation: {
          ...entry.observation,
          status: "submitted",
          finalized: false,
          recorded: false,
        },
      })
      return
    }
    publish({
      status: result === "finalized" ? "success" : result === "reverted" ? "reverted" : "conflict",
      finalized: result === "finalized" || result === "reverted",
    })
    if (result !== "finalized") return
    void notifyFinalizedMint(key, entry, pending)
  } catch {
    publish({ unavailable: true })
  }
}

async function notifyFinalizedMint(
  key: string,
  entry: ObservationEntry,
  pending: PendingMint,
): Promise<void> {
  const isCurrent = () => observations.get(key) === entry
  const publish = (patch: Partial<MintObservation>) => {
    if (isCurrent()) entry.observation = { ...entry.observation, ...patch }
  }
  const notificationKey = `${key}:${entry.fingerprint}`
  if (!completedNotifications.has(notificationKey)) {
    try {
      const recorded = await sharedBaseRead(`mint-notification:${notificationKey}`, () =>
        withBrowserLock(
          `kinic-mint-notify:${deploymentProfile.deploymentInstanceId}:${pending.depositId}`,
          async () => {
            if (!isCurrent()) return false
            const identity = await getWithdrawalNotificationIdentity(deploymentProfile)
            if (!isCurrent()) return false
            const actor = await createBridgeActor(
              deploymentProfile.icHost,
              deploymentProfile.bridgeCanisterId as string,
              identity,
            )
            if (!isCurrent()) return false
            const result = await actor.notify_deposit_mint({
              deposit_id: hexToBytes(pending.depositId),
              transaction_hash: hexToBytes(pending.transactionHash),
            })
            if (!isCurrent()) return false
            if ("Err" in result) throw new Error(Object.keys(result.Err)[0])
            const value = "Recorded" in result.Ok ? result.Ok.Recorded : result.Ok.Duplicate
            if (bytesHex(value.deposit_id).toLowerCase() !== pending.depositId.toLowerCase())
              throw new Error("IdentityConflict")
            return true
          },
        ),
      )
      if (!isCurrent() || !recorded) return
      completedNotifications.add(notificationKey)
    } catch (error) {
      publish({
        notificationError:
          error instanceof Error && /^[A-Za-z]+$/.test(error.message)
            ? error.message
            : "NotificationUnavailable",
      })
      return
    }
  }
  publish({ recorded: true, notificationError: undefined })
}

export async function observeDeposit(record: DepositView): Promise<MintObservation> {
  const recorded = record.mint_receipt[0]
  if (recorded)
    return {
      status: "success",
      transactionHash: bytesHex(recorded.transaction_hash),
      blockNumber: recorded.receipt_block_number,
      finalized: true,
      recorded: true,
    }
  const authorization = record.mint_authorization[0]
  if (!authorization) return { status: "unsubmitted", finalized: false, recorded: false }
  const expected = {
    depositId: bytesHex(record.deposit_id),
    authorizationDigest: bytesHex(authorization.digest),
    recipient: bytesHex(authorization.recipient),
    grossAmount: authorization.gross_amount.toString(),
    chargedServiceFee: authorization.charged_service_fee.toString(),
    mintedAmount: (authorization.gross_amount - authorization.charged_service_fee).toString(),
  }
  const pending = readPendingMint(expected)
  if (pending) return observeMint(pending)
  try {
    const processed = await sharedBaseRead(`processed:${expected.depositId}`, () =>
      basePublicClient.readContract({
        address: deploymentProfile.bridgeAddress as Hex,
        abi: bridgeAbi,
        functionName: "isDepositProcessed",
        args: [expected.depositId],
      }),
    )
    return { status: processed ? "processed" : "unsubmitted", finalized: false, recorded: false }
  } catch {
    return { status: "unsubmitted", finalized: false, recorded: false, unavailable: true }
  }
}

export function clearMintObservations() {
  observations.clear()
  completedNotifications.clear()
}
