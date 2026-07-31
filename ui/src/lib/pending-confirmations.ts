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
}

export type PendingConfirmation = PendingWithdrawalConfirmation
export type PendingConfirmationInput =
  Omit<PendingWithdrawalConfirmation, "blocked" | "bridgeCanisterId" | "chainId" | "bridgeAddress"> & { blocked?: boolean }

const STORAGE_PREFIX = "kinic.bridge.pending-confirmations.v5"
let sessionQueue: PendingConfirmation[] | undefined
export const PENDING_CONFIRMATIONS_CHANGED = "kinic-pending-confirmations-changed"

function pendingMintKey(depositId: Hex): string {
  return [
    "kinic.bridge.pending-mint.v1",
    deploymentProfile.chainId,
    String(deploymentProfile.bridgeAddress).toLowerCase(),
    deploymentProfile.bridgeCanisterId ?? "",
    depositId.toLowerCase(),
  ].join(":")
}

const sessionPendingMints = new Map<string, Hex>()
const removedSessionPendingMints = new Set<string>()

export async function savePendingMint(depositId: Hex, transactionHash: Hex): Promise<void> {
  const key = pendingMintKey(depositId)
  removedSessionPendingMints.delete(key)
  sessionPendingMints.set(key, transactionHash)
  await withBrowserLock(`kinic-storage:${key}`, () => {
    try {
      window.localStorage.setItem(key, transactionHash)
      sessionPendingMints.delete(key)
    } catch { /* The session copy still preserves recovery after a successful wallet broadcast. */ }
  })
}

export function readPendingMint(depositId: Hex): Hex | undefined {
  if (typeof window === "undefined") return undefined
  const key = pendingMintKey(depositId)
  if (removedSessionPendingMints.has(key)) return undefined
  const sessionValue = sessionPendingMints.get(key)
  if (sessionValue) return sessionValue
  try {
    const value = window.localStorage.getItem(key)
    return value && /^0x[0-9a-fA-F]{64}$/.test(value) ? value as Hex : undefined
  } catch {
    return undefined
  }
}

export async function removePendingMint(depositId: Hex): Promise<void> {
  const key = pendingMintKey(depositId)
  sessionPendingMints.delete(key)
  removedSessionPendingMints.add(key)
  await withBrowserLock(`kinic-storage:${key}`, () => {
    try {
      window.localStorage.removeItem(key)
      removedSessionPendingMints.delete(key)
    } catch { /* The session tombstone prevents a reverted transaction from reappearing. */ }
  })
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
    const entry = { ...value, blocked: value.blocked ?? false, ...activeDeployment() }
    return upsertPendingConfirmation(next, entry, false)
  })
}

export async function restorePendingConfirmation(value: PendingConfirmationInput): Promise<void> {
  await update((next) => {
    const entry = { ...value, blocked: value.blocked ?? false, ...activeDeployment() }
    return upsertPendingConfirmation(next, entry, true)
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
      window.localStorage.setItem(key, JSON.stringify({ version: 5, entries: values }))
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
  return common && item.kind === "withdrawal"
}

function isStoredQueue(value: unknown): value is { version: 5; entries: unknown[] } {
  if (typeof value !== "object" || value === null) return false
  const queue = value as Record<string, unknown>
  return queue.version === 5 && Array.isArray(queue.entries)
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
  else next[index] = preserveExistingBlocked ? { ...entry, blocked: next[index]!.blocked } : entry
  return next
}
