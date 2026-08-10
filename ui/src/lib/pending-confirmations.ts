import { deploymentProfile } from "@/config/profile"
import { withBrowserLock } from "@/lib/browser-lock"
import type { Hex } from "viem"

interface PendingConfirmationBase {
  transactionHash: `0x${string}`
  owner: string
  blocked: boolean
  bridgeCanisterId: string
  chainId: number
  bridgeAddress: string
}

export interface PendingWithdrawalConfirmation extends PendingConfirmationBase {
  kind: "withdrawal"
  notification:
    | { status: "awaiting-notification" }
    | { status: "notified"; withdrawalId: Hex }
}

export type PendingConfirmation = PendingWithdrawalConfirmation
export type PendingConfirmationInput =
  Omit<PendingWithdrawalConfirmation, "blocked" | "bridgeCanisterId" | "chainId" | "bridgeAddress" | "notification"> & { blocked?: boolean }

const STORAGE_PREFIX = "kinic.bridge.pending-confirmations.v6"
let sessionQueue: PendingConfirmation[] | undefined
export const PENDING_CONFIRMATIONS_CHANGED = "kinic-pending-confirmations-changed"

export interface PendingMintExpectation {
  depositId: Hex
  authorizationDigest: Hex
  recipient: Hex
  grossAmount: string
  chargedServiceFee: string
  mintedAmount: string
}

export interface PendingMint extends PendingMintExpectation {
  transactionHash: Hex
}

function pendingMintKey(expected: PendingMintExpectation): string {
  return [
    "kinic.bridge.pending-mint.v2",
    deploymentProfile.chainId,
    String(deploymentProfile.bridgeAddress).toLowerCase(),
    deploymentProfile.bridgeCanisterId ?? "",
    deploymentProfile.deploymentInstanceId?.toLowerCase() ?? "",
    expected.depositId.toLowerCase(),
    expected.authorizationDigest.toLowerCase(),
  ].join(":")
}

const sessionPendingMints = new Map<string, PendingMint>()
const removedSessionPendingMints = new Set<string>()

export async function savePendingMint(value: PendingMint): Promise<void> {
  const key = pendingMintKey(value)
  removedSessionPendingMints.delete(key)
  sessionPendingMints.set(key, value)
  await withBrowserLock(`kinic-storage:${key}`, () => {
    try {
      window.localStorage.setItem(key, JSON.stringify(value))
      sessionPendingMints.delete(key)
    } catch { /* The session copy still preserves recovery after a successful wallet broadcast. */ }
  })
}

export function readPendingMint(expected: PendingMintExpectation): PendingMint | undefined {
  if (typeof window === "undefined") return undefined
  const key = pendingMintKey(expected)
  if (removedSessionPendingMints.has(key)) return undefined
  const sessionValue = sessionPendingMints.get(key)
  if (sessionValue) return sessionValue
  try {
    const value: unknown = JSON.parse(window.localStorage.getItem(key) ?? "null")
    return pendingMintMatches(value, expected) ? value : undefined
  } catch {
    return undefined
  }
}

export async function removePendingMint(expected: PendingMintExpectation): Promise<void> {
  const key = pendingMintKey(expected)
  sessionPendingMints.delete(key)
  removedSessionPendingMints.add(key)
  await withBrowserLock(`kinic-storage:${key}`, () => {
    try {
      window.localStorage.removeItem(key)
      removedSessionPendingMints.delete(key)
    } catch { /* The session tombstone prevents a reverted transaction from reappearing. */ }
  })
}

function pendingMintMatches(value: unknown, expected: PendingMintExpectation): value is PendingMint {
  if (!value || typeof value !== "object") return false
  const candidate = value as Partial<PendingMint>
  return /^0x[0-9a-fA-F]{64}$/.test(candidate.transactionHash ?? "")
    && candidate.depositId?.toLowerCase() === expected.depositId.toLowerCase()
    && candidate.authorizationDigest?.toLowerCase() === expected.authorizationDigest.toLowerCase()
    && candidate.recipient?.toLowerCase() === expected.recipient.toLowerCase()
    && candidate.grossAmount === expected.grossAmount
    && candidate.chargedServiceFee === expected.chargedServiceFee
    && candidate.mintedAmount === expected.mintedAmount
}

export function pendingConfirmationKey(value: PendingConfirmation | PendingConfirmationInput): string {
  return `withdrawal:${value.transactionHash.toLowerCase()}`
}

export function readPendingConfirmations(): PendingConfirmation[] {
  if (typeof window === "undefined") return []
  if (sessionQueue !== undefined) return sessionQueue.filter(matchesActiveDeployment)
  try {
    const value: unknown = JSON.parse(window.localStorage.getItem(pendingConfirmationsStorageKey()) ?? "null")
    if (!isStoredQueue(value)) return []
    return value.entries.filter(isPendingConfirmation).filter(matchesActiveDeployment)
  } catch {
    return []
  }
}

export async function savePendingConfirmation(value: PendingConfirmationInput): Promise<void> {
  await update((next) => {
    const entry: PendingConfirmation = {
      ...value,
      blocked: value.blocked ?? false,
      notification: { status: "awaiting-notification" },
      ...activeDeployment(),
    }
    return upsertPendingConfirmation(next, entry, false)
  })
}

export async function restorePendingConfirmation(value: PendingConfirmationInput): Promise<void> {
  await update((next) => {
    const entry: PendingConfirmation = {
      ...value,
      blocked: value.blocked ?? false,
      notification: { status: "awaiting-notification" },
      ...activeDeployment(),
    }
    return upsertPendingConfirmation(next, entry, true)
  })
}

export async function ensurePendingWithdrawalConfirmation(value: PendingConfirmationInput): Promise<void> {
  await update((next) => {
    const key = pendingConfirmationKey(value)
    const existing = next.find((item) => pendingConfirmationKey(item) === key)
    if (existing) {
      if (existing.owner !== value.owner) throw new Error("Pending withdrawal destination owner conflict")
      return next
    }
    return [...next, {
      ...value,
      blocked: value.blocked ?? false,
      notification: { status: "awaiting-notification" },
      ...activeDeployment(),
    }]
  })
}

export async function removePendingConfirmation(value: PendingConfirmation | PendingConfirmationInput): Promise<void> {
  const key = pendingConfirmationKey(value)
  await update((next) => next.filter((item) => pendingConfirmationKey(item) !== key))
}

export async function setPendingConfirmationBlocked(
  value: PendingConfirmation | PendingConfirmationInput,
  blocked: boolean,
): Promise<void> {
  const key = pendingConfirmationKey(value)
  await update((next) => next.map((item) => pendingConfirmationKey(item) === key ? { ...item, blocked } : item))
}

export async function markPendingConfirmationNotified(
  value: PendingConfirmation | PendingConfirmationInput,
  withdrawalId: Hex,
): Promise<void> {
  const key = pendingConfirmationKey(value)
  await update((next) => {
    let found = false
    const values = next.map((item) => {
      if (pendingConfirmationKey(item) !== key) return item
      found = true
      if (item.notification.status === "notified"
        && item.notification.withdrawalId.toLowerCase() !== withdrawalId.toLowerCase()) {
        throw new Error("Pending withdrawal notification ID conflict")
      }
      return { ...item, notification: { status: "notified" as const, withdrawalId } }
    })
    if (!found) throw new Error("Pending withdrawal notification record is missing")
    return values
  })
}

function activeDeployment() {
  return {
    bridgeCanisterId: deploymentProfile.bridgeCanisterId ?? "",
    chainId: deploymentProfile.chainId,
    bridgeAddress: deploymentProfile.bridgeAddress?.toLowerCase() ?? "",
  }
}

function matchesActiveDeployment(value: PendingConfirmation): boolean {
  const active = activeDeployment()
  return value.bridgeCanisterId === active.bridgeCanisterId
    && value.chainId === active.chainId
    && value.bridgeAddress === active.bridgeAddress
}

async function update(change: (values: PendingConfirmation[]) => PendingConfirmation[]): Promise<void> {
  const key = pendingConfirmationsStorageKey()
  await withBrowserLock(`kinic-storage:${key}`, () => {
    const values = change(readPendingConfirmations())
    sessionQueue = values
    try {
      window.localStorage.setItem(key, JSON.stringify({ version: 6, entries: values }))
      sessionQueue = undefined
    } finally {
      window.dispatchEvent(new Event(PENDING_CONFIRMATIONS_CHANGED))
    }
  })
}

export function pendingConfirmationsStorageKey(): string {
  const active = activeDeployment()
  return `${STORAGE_PREFIX}:${active.chainId}:${active.bridgeAddress}:${active.bridgeCanisterId}`
}

function isPendingConfirmation(value: unknown): value is PendingConfirmation {
  if (typeof value !== "object" || value === null) return false
  const item = value as Record<string, unknown>
  const common = typeof item.transactionHash === "string" && /^0x[0-9a-fA-F]{64}$/.test(item.transactionHash)
    && typeof item.owner === "string"
    && typeof item.blocked === "boolean"
    && typeof item.bridgeCanisterId === "string"
    && typeof item.chainId === "number"
    && typeof item.bridgeAddress === "string"
  if (!common || item.kind !== "withdrawal" || !item.notification || typeof item.notification !== "object") return false
  const notification = item.notification as Record<string, unknown>
  return notification.status === "awaiting-notification"
    || notification.status === "notified"
      && typeof notification.withdrawalId === "string"
      && /^0x[0-9a-fA-F]{64}$/.test(notification.withdrawalId)
}

function isStoredQueue(value: unknown): value is { version: 6; entries: unknown[] } {
  if (typeof value !== "object" || value === null) return false
  const queue = value as Record<string, unknown>
  return queue.version === 6 && Array.isArray(queue.entries)
}

export function upsertPendingConfirmation(
  values: PendingConfirmation[],
  entry: PendingConfirmation,
  preserveExistingBlocked: boolean,
): PendingConfirmation[] {
  const next = [...values]
  const key = pendingConfirmationKey(entry)
  const index = next.findIndex((item) => pendingConfirmationKey(item) === key)
  if (index === -1) next.push(entry)
  else {
    const existing = next[index]!
    next[index] = {
      ...entry,
      blocked: preserveExistingBlocked ? existing.blocked : entry.blocked,
      notification: existing.notification.status === "notified"
        ? existing.notification
        : entry.notification,
    }
  }
  return next
}
