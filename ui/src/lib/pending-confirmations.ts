import { deploymentProfile } from "@/config/profile"
import { withBrowserLock } from "@/lib/browser-lock"

export type PendingConfirmationKind = "deposit" | "withdrawal"

interface PendingConfirmationBase {
  transactionHash: `0x${string}`
  owner: string
  blocked: boolean
  bridgeCanisterId: string
  chainId: number
  bridgeAddress: string
}

export interface PendingDepositConfirmation extends PendingConfirmationBase {
  kind: "deposit"
  settlementId: `0x${string}`
}

export interface PendingWithdrawalConfirmation extends PendingConfirmationBase {
  kind: "withdrawal"
}

export type PendingConfirmation = PendingDepositConfirmation | PendingWithdrawalConfirmation
export type PendingConfirmationInput =
  | (Omit<PendingDepositConfirmation, "blocked" | "bridgeCanisterId" | "chainId" | "bridgeAddress"> & { blocked?: boolean })
  | (Omit<PendingWithdrawalConfirmation, "blocked" | "bridgeCanisterId" | "chainId" | "bridgeAddress"> & { blocked?: boolean })

const STORAGE_PREFIX = "kinic.bridge.pending-confirmations.v4"
let sessionQueue: PendingConfirmation[] | undefined
export const PENDING_CONFIRMATIONS_CHANGED = "kinic-pending-confirmations-changed"

export function pendingConfirmationKey(value: PendingConfirmation | PendingConfirmationInput): string {
  return value.kind === "deposit"
    ? `deposit:${value.settlementId.toLowerCase()}`
    : `withdrawal:${value.transactionHash.toLowerCase()}`
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
      window.localStorage.setItem(key, JSON.stringify({ version: 4, entries: values }))
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
  if (!common) return false
  if (item.kind === "withdrawal") return true
  return item.kind === "deposit"
    && typeof item.settlementId === "string"
    && /^0x[0-9a-fA-F]{64}$/.test(item.settlementId)
}

function isStoredQueue(value: unknown): value is { version: 4; entries: unknown[] } {
  if (typeof value !== "object" || value === null) return false
  const queue = value as Record<string, unknown>
  return queue.version === 4 && Array.isArray(queue.entries)
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
