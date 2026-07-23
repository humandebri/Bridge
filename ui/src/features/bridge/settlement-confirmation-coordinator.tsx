import { useEffect } from "react"
import { hexToBytes } from "viem"
import { toast } from "sonner"
import { deploymentProfile } from "@/config/profile"
import { useIcWallet } from "@/features/wallet/ic-wallet-provider"
import type { SettlementActionResult } from "@/generated/bridge.did"
import { basePublicClient } from "@/lib/evm/client"
import { createBridgeActor } from "@/lib/ic/bridge"
import { NotifyWithdrawalCallError, SettlementActionCallError, type IcWalletAdapter } from "@/lib/ic/wallet"
import {
  PENDING_CONFIRMATIONS_CHANGED,
  pendingConfirmationKey,
  pendingConfirmationsStorageKey,
  readPendingConfirmations,
  removePendingConfirmation,
  savePendingConfirmation,
  setPendingConfirmationBlocked,
  type PendingConfirmation,
  type PendingConfirmationInput,
} from "@/lib/pending-confirmations"
import { withdrawalNotificationPresentation } from "@/lib/withdrawal-notification"
import { decideWithdrawalFinalization } from "@/lib/withdrawal-confirmation-state"
import { withBrowserLock } from "@/lib/browser-lock"

export const CONFIRMATION_POLL_MS = 15_000

export type ConfirmationOutcome =
  | { status: "retry"; retryAt?: number }
  | { status: "complete" }
  | { status: "reverted" }
  | { status: "blocked" }

export type ConfirmationTrigger = "background" | "userInitiated"

export async function confirmPendingDepositFromUserAction(
  input: Extract<PendingConfirmationInput, { kind: "deposit" }>,
  adapter: IcWalletAdapter,
  ensureWriteReady: () => Promise<void>,
): Promise<ConfirmationOutcome | undefined> {
  const closeWalletSession = await adapter.prepare()
  try {
    await ensureWriteReady()
    const entry: Extract<PendingConfirmation, { kind: "deposit" }> = {
      ...input,
      blocked: input.blocked ?? false,
      bridgeCanisterId: deploymentProfile.bridgeCanisterId ?? "",
      chainId: deploymentProfile.chainId,
      bridgeAddress: deploymentProfile.bridgeAddress?.toLowerCase() ?? "",
    }
    return await runWithConfirmationLock(pendingConfirmationKey(entry), entry, async (lease) => {
      lease.assertCurrent()
      await savePendingConfirmation(input)
      lease.assertCurrent()
      return confirmWhenFinalized(entry, adapter, () => true, lease, "userInitiated")
    })
  } finally {
    await closeWalletSession()
  }
}

export function SettlementConfirmationCoordinator() {
  const ic = useIcWallet()

  useEffect(() => {
    const owner = ic.account?.owner
    if (!ic.adapter || !owner) return

    let active = true
    const running = new Set<string>()
    const retryAt = new Map<string, number>()

    const tick = () => {
      if (!active || document.visibilityState !== "visible") return
      const now = Date.now()
      const entries = readPendingConfirmations().filter((entry) => !entry.blocked && entry.owner === owner)
      for (const entry of entries) {
        const key = pendingConfirmationKey(entry)
        if (running.has(key) || (retryAt.get(key) ?? 0) > now) continue
        running.add(key)
        void runWithConfirmationLock(key, entry, (lease) => confirmWhenFinalized(
          entry,
          ic.adapter!,
          () => active && document.visibilityState === "visible",
          lease,
        ))
          .then((outcome) => {
            if (!active) return
            if (!outcome) {
              retryAt.set(key, Date.now() + CONFIRMATION_POLL_MS)
              return
            }
            if (outcome.status === "retry") retryAt.set(key, outcome.retryAt ?? Date.now() + CONFIRMATION_POLL_MS)
            else retryAt.delete(key)
          })
          .finally(() => {
            running.delete(key)
          })
      }
    }

    const wake = () => {
      retryAt.clear()
      tick()
    }
    const wakeFromStorage = (event: StorageEvent) => {
      if (event.key === pendingConfirmationsStorageKey()) wake()
    }
    window.addEventListener(PENDING_CONFIRMATIONS_CHANGED, wake)
    window.addEventListener("storage", wakeFromStorage)
    document.addEventListener("visibilitychange", wake)
    const interval = ic.adapter.requiresUserGesture ? undefined : window.setInterval(tick, CONFIRMATION_POLL_MS)
    tick()

    return () => {
      active = false
      if (interval !== undefined) window.clearInterval(interval)
      window.removeEventListener(PENDING_CONFIRMATIONS_CHANGED, wake)
      window.removeEventListener("storage", wakeFromStorage)
      document.removeEventListener("visibilitychange", wake)
    }
  }, [ic.account?.owner, ic.adapter])

  return null
}

export async function runWithConfirmationLock<T>(
  key: string,
  entry: PendingConfirmation,
  action: (lease: ConfirmationLease) => Promise<T>,
): Promise<T | undefined> {
  const lockName = `kinic-confirm:${entry.chainId}:${entry.bridgeAddress}:${entry.bridgeCanisterId}:${entry.owner}:${key}`
  if (navigator.locks) {
    return navigator.locks.request(lockName, { ifAvailable: true }, async (lock) => {
      if (!lock) return undefined
      const controller = new AbortController()
      let held = true
      const lease: ConfirmationLease = {
        signal: controller.signal,
        assertCurrent: () => {
          if (!held) throw new ConfirmationLeaseLostError()
        },
      }
      try {
        return await action(lease)
      } finally {
        held = false
        controller.abort()
      }
    })
  }
  return withLocalStorageLease(lockName, action)
}

export interface ConfirmationLease {
  readonly signal: AbortSignal
  assertCurrent(): void
}

interface LocalStorageLeaseRecord {
  ownerId: string
  expiresAt: number
  fencingToken: number
}

interface LeaseChannelMessage {
  key: string
  lease: LocalStorageLeaseRecord
}

const LEASE_TTL_MS = 30_000
const LEASE_RENEW_MS = 10_000
const LEASE_ELECTION_MS = 75
const LEASE_CONFIRM_MS = 25
const LEASE_CHANNEL = "kinic.bridge.confirmation-leases.v2"

class ConfirmationLeaseLostError extends Error {
  constructor() {
    super("Confirmation lease ownership was lost")
    this.name = "ConfirmationLeaseLostError"
  }
}

async function withLocalStorageLease<T>(
  key: string,
  action: (lease: ConfirmationLease) => Promise<T>,
): Promise<T | undefined> {
  const storageKey = `kinic.bridge.confirmation-lease.v2:${key}`
  const ownerId = crypto.randomUUID()
  const now = Date.now()
  const previous = readLease(storageKey)
  if (previous === null) return undefined
  if (previous && previous.expiresAt > now) return undefined
  const lease = {
    ownerId,
    expiresAt: now + LEASE_TTL_MS,
    fencingToken: Math.max(now, (previous?.fencingToken ?? 0) + 1),
  }
  const contenders = new Map<string, LocalStorageLeaseRecord>([[ownerId, lease]])
  const channel = typeof BroadcastChannel === "undefined" ? undefined : new BroadcastChannel(LEASE_CHANNEL)
  const recordContender = (message: unknown) => {
    const candidate = readLeaseMessage(message)
    if (candidate?.key === storageKey && candidate.lease.expiresAt > Date.now()) {
      contenders.set(candidate.lease.ownerId, candidate.lease)
    }
  }
  if (channel) channel.onmessage = (event) => recordContender(event.data)
  window.localStorage.setItem(storageKey, JSON.stringify(lease))
  channel?.postMessage({ key: storageKey, lease } satisfies LeaseChannelMessage)
  await delay(LEASE_ELECTION_MS)
  const electedCurrent = readLease(storageKey)
  if (electedCurrent === null) {
    channel?.close()
    return undefined
  }
  if (electedCurrent) contenders.set(electedCurrent.ownerId, electedCurrent)
  const winner = [...contenders.values()].sort(compareLeasePriority).at(-1)
  if (!winner || winner.ownerId !== ownerId) {
    channel?.close()
    return undefined
  }
  window.localStorage.setItem(storageKey, JSON.stringify(lease))
  channel?.postMessage({ key: storageKey, lease } satisfies LeaseChannelMessage)
  await delay(LEASE_CONFIRM_MS)
  const acquired = readLease(storageKey)
  if (!ownsLease(acquired, lease)) {
    channel?.close()
    return undefined
  }

  const controller = new AbortController()
  const assertCurrent = () => {
    if (controller.signal.aborted || !ownsLease(readLease(storageKey), lease)) {
      controller.abort()
      throw new ConfirmationLeaseLostError()
    }
  }
  const storageListener = (event: StorageEvent) => {
    if (event.key === storageKey && !ownsLease(readLease(storageKey), lease)) controller.abort()
  }
  window.addEventListener("storage", storageListener)
  if (channel) {
    channel.onmessage = (event) => {
      const message = readLeaseMessage(event.data)
      if (message?.key === storageKey && compareLeasePriority(lease, message.lease) < 0) {
        controller.abort()
      }
    }
  }
  const renewal = window.setInterval(() => {
    try {
      assertCurrent()
      lease.expiresAt = Date.now() + LEASE_TTL_MS
      window.localStorage.setItem(storageKey, JSON.stringify(lease))
      channel?.postMessage({ key: storageKey, lease } satisfies LeaseChannelMessage)
    } catch {
      controller.abort()
    }
  }, LEASE_RENEW_MS)
  try {
    assertCurrent()
    return await action({ signal: controller.signal, assertCurrent })
  } catch (error) {
    if (error instanceof ConfirmationLeaseLostError) return undefined
    throw error
  } finally {
    window.clearInterval(renewal)
    window.removeEventListener("storage", storageListener)
    controller.abort()
    channel?.close()
    if (ownsLease(readLease(storageKey), lease)) window.localStorage.removeItem(storageKey)
  }
}

function readLease(key: string): LocalStorageLeaseRecord | undefined | null {
  const raw = window.localStorage.getItem(key)
  if (raw === null) return undefined
  try {
    const value: unknown = JSON.parse(raw)
    if (typeof value !== "object" || value === null) return null
    const lease = value as Record<string, unknown>
    return typeof lease.ownerId === "string"
      && typeof lease.expiresAt === "number"
      && typeof lease.fencingToken === "number"
      && Number.isSafeInteger(lease.expiresAt)
      && Number.isSafeInteger(lease.fencingToken)
      ? { ownerId: lease.ownerId, expiresAt: lease.expiresAt, fencingToken: lease.fencingToken }
      : null
  } catch {
    return null
  }
}

function readLeaseMessage(value: unknown): LeaseChannelMessage | undefined {
  if (typeof value !== "object" || value === null) return undefined
  const message = value as { key?: unknown; lease?: unknown }
  if (typeof message.key !== "string" || typeof message.lease !== "object" || message.lease === null) return undefined
  const lease = message.lease as Record<string, unknown>
  return typeof lease.ownerId === "string"
    && typeof lease.expiresAt === "number"
    && typeof lease.fencingToken === "number"
    && Number.isSafeInteger(lease.expiresAt)
    && Number.isSafeInteger(lease.fencingToken)
    ? { key: message.key, lease: { ownerId: lease.ownerId, expiresAt: lease.expiresAt, fencingToken: lease.fencingToken } }
    : undefined
}

function compareLeasePriority(left: LocalStorageLeaseRecord, right: LocalStorageLeaseRecord): number {
  return left.fencingToken - right.fencingToken || left.ownerId.localeCompare(right.ownerId)
}

function ownsLease(
  current: LocalStorageLeaseRecord | undefined | null,
  expected: LocalStorageLeaseRecord,
): current is LocalStorageLeaseRecord {
  return current !== null
    && current !== undefined
    && current.ownerId === expected.ownerId
    && current.fencingToken === expected.fencingToken
    && current.expiresAt > Date.now()
}

function delay(ms: number): Promise<void> {
  return new Promise((resolve) => window.setTimeout(resolve, ms))
}

const UNFENCED_CONFIRMATION_LEASE: ConfirmationLease = {
  signal: new AbortController().signal,
  assertCurrent: () => undefined,
}

export async function confirmWhenFinalized(
  entry: PendingConfirmation,
  adapter: IcWalletAdapter,
  isActive: () => boolean = () => true,
  lease: ConfirmationLease = UNFENCED_CONFIRMATION_LEASE,
  trigger: ConfirmationTrigger = "background",
): Promise<ConfirmationOutcome> {
  if (entry.kind === "deposit") {
    try {
      lease.assertCurrent()
      const actor = await createBridgeActor(deploymentProfile.icHost, entry.bridgeCanisterId)
      lease.assertCurrent()
      const [record] = await actor.get_deposit(hexToBytes(entry.settlementId))
      lease.assertCurrent()
      const confirmation = record?.base_confirmation[0]
      if (confirmation && "Submitted" in confirmation) {
        const canonicalHash = bytesHex(confirmation.Submitted.transaction_hash)
        if (canonicalHash.toLowerCase() !== entry.transactionHash.toLowerCase()) {
          lease.assertCurrent()
          await savePendingConfirmation({ ...entry, transactionHash: canonicalHash })
          return { status: "retry" }
        }
      }
    } catch (error) {
      if (error instanceof ConfirmationLeaseLostError) throw error
      return { status: "retry" }
    }
  }
  let receipt: Awaited<ReturnType<typeof basePublicClient.getTransactionReceipt>>
  let finalized: Awaited<ReturnType<typeof basePublicClient.getBlock>>
  try {
    lease.assertCurrent()
    receipt = await basePublicClient.getTransactionReceipt({ hash: entry.transactionHash })
    lease.assertCurrent()
    finalized = await basePublicClient.getBlock({ blockTag: "finalized" })
    lease.assertCurrent()
  } catch (error) {
    if (error instanceof ConfirmationLeaseLostError) throw error
    return { status: "retry" }
  }
  if (!isActive()) return { status: "retry" }
  if (entry.kind === "withdrawal") {
    const decision = decideWithdrawalFinalization(receipt.status, receipt.blockNumber, finalized.number)
    if (decision === "retry") return { status: "retry" }
    if (decision === "discard-reverted") {
      lease.assertCurrent()
      await removePendingConfirmation(entry)
      toast.warning("The Base withdrawal transaction reverted, so no withdrawal was created. You can try again.")
      return { status: "reverted" }
    }
  }
  if (finalized.number === null || finalized.number < receipt.blockNumber) return { status: "retry" }
  const finalizedBlockNumber = finalized.number
  if (adapter.requiresUserGesture && trigger === "background") return { status: "retry" }

  try {
    lease.assertCurrent()
    toast.info("Base transaction is finalized. Review the IC wallet confirmation.")
    if (entry.kind === "withdrawal") {
      const receipt = await withIcOwnerPrompt(entry.owner, adapter, () => adapter.notifyWithdrawal(hexToBytes(entry.transactionHash)))
      lease.assertCurrent()
      await removePendingConfirmation(entry)
      toastWithdrawalNotification(receipt)
      return { status: "complete" }
    }
    const result = await withIcOwnerPrompt(entry.owner, adapter, () => adapter.confirmDeposit({
      settlementId: hexToBytes(entry.settlementId),
      transactionHash: hexToBytes(entry.transactionHash),
      receiptBlockNumber: receipt.blockNumber,
      observedFinalizedBlockNumber: finalizedBlockNumber,
    }))
    lease.assertCurrent()
    return await handleDepositResult(entry, result)
  } catch (error) {
    if (error instanceof ConfirmationLeaseLostError) throw error
    if (!isActive()) return { status: "retry" }
    if (isRetryableConfirmationError(error)) {
      return { status: "retry", retryAt: error instanceof SettlementActionCallError ? error.retryAt : undefined }
    }
    await setPendingConfirmationBlocked(entry, true)
    toast.warning("Confirmation was not completed. Resume it from History.")
    return { status: "blocked" }
  }
}

export function isRetryableConfirmationError(error: unknown): boolean {
  if (error instanceof SettlementActionCallError) {
    return ["Busy", "AutomaticProgressPending", "RateLimited", "StorageFailure"].includes(error.code)
  }
  if (error instanceof NotifyWithdrawalCallError) {
    return ["Busy", "RpcUnavailable", "RpcInconsistent", "TransactionNotFound", "TransactionNotConfirmed", "StorageFailure", "RateLimited", "InsufficientCycles"].includes(error.code)
  }
  if (!(error instanceof Error)) return true
  return !/reject|declin|denied|cancel|account changed|does not own|reverted|payload already|hash is invalid|state does not match|signer does not match|connect (?:an ic wallet|oisy|plug)|not connected|not installed|reconnect|unauthorized|invalid .*reply|wallet reply.*invalid|response .*mismatch|certifi/i.test(error.message)
}

function toastWithdrawalNotification(receipt: Awaited<ReturnType<IcWalletAdapter["notifyWithdrawal"]>>): void {
  const presentation = withdrawalNotificationPresentation(receipt)
  if (presentation.tone === "success") toast.success(presentation.message)
  else if (presentation.tone === "warning") toast.warning(presentation.message)
  else toast.info(presentation.message)
}

async function handleDepositResult(entry: Extract<PendingConfirmation, { kind: "deposit" }>, result: SettlementActionResult): Promise<ConfirmationOutcome> {
  if ("Submitted" in result) {
    await savePendingConfirmation({ ...entry, transactionHash: bytesHex(result.Submitted.transaction_hash), blocked: false })
    toast.success("The next Base transaction was submitted. Check History after finalization if it has not completed.")
    return { status: "retry" }
  }
  if ("WaitingForConfirmation" in result) return { status: "retry" }
  await removePendingConfirmation(entry)
  if ("Stopped" in result) toast.warning("Settlement stopped and needs attention in History.")
  else toast.success("Base confirmation was verified by the bridge.")
  return { status: "complete" }
}

function bytesHex(bytes: Uint8Array | number[]): `0x${string}` {
  return `0x${Array.from(bytes, (value) => Number(value).toString(16).padStart(2, "0")).join("")}`
}

async function withIcOwnerPrompt<T>(owner: string, adapter: IcWalletAdapter, prompt: () => Promise<T>): Promise<T> {
  return withBrowserLock(`kinic-wallet-prompt:ic:${owner}`, async () => {
    if ((await adapter.getAccount()).owner !== owner) throw new Error("The connected IC account changed before the wallet prompt")
    const result = await prompt()
    if ((await adapter.getAccount()).owner !== owner) throw new Error("The connected IC account changed during the wallet prompt")
    return result
  })
}
