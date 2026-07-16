import { deploymentProfile } from "@/config/profile"

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

const STORAGE_KEY = "kinic.bridge.pending-confirmations.v2"
export const PENDING_CONFIRMATIONS_CHANGED = "kinic-pending-confirmations-changed"

export function pendingConfirmationKey(value: PendingConfirmation | PendingConfirmationInput): string {
  return value.kind === "deposit"
    ? `deposit:${value.settlementId.toLowerCase()}`
    : `withdrawal:${value.transactionHash.toLowerCase()}`
}

export function readPendingConfirmations(): PendingConfirmation[] {
  if (typeof window === "undefined") return []
  try {
    const value: unknown = JSON.parse(window.localStorage.getItem(STORAGE_KEY) ?? "[]")
    if (!Array.isArray(value)) return []
    return value.filter(isPendingConfirmation).filter(matchesActiveDeployment)
  } catch {
    return []
  }
}

export function savePendingConfirmation(value: PendingConfirmationInput): void {
  const next = readPendingConfirmations()
  const entry = {
    ...value,
    blocked: value.blocked ?? false,
    ...activeDeployment(),
  }
  const key = pendingConfirmationKey(entry)
  const index = next.findIndex((item) => pendingConfirmationKey(item) === key)
  if (index === -1) next.push(entry)
  else next[index] = entry
  write(next)
}

export function restorePendingConfirmation(value: PendingConfirmationInput): void {
  const key = pendingConfirmationKey(value)
  if (readPendingConfirmations().some((item) => pendingConfirmationKey(item) === key)) return
  savePendingConfirmation(value)
}

export function removePendingConfirmation(value: PendingConfirmation | PendingConfirmationInput): void {
  const key = pendingConfirmationKey(value)
  write(readPendingConfirmations().filter((item) => pendingConfirmationKey(item) !== key))
}

export function setPendingConfirmationBlocked(
  value: PendingConfirmation | PendingConfirmationInput,
  blocked: boolean,
): void {
  const key = pendingConfirmationKey(value)
  write(readPendingConfirmations().map((item) => pendingConfirmationKey(item) === key ? { ...item, blocked } : item))
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

function write(values: PendingConfirmation[]): void {
  window.localStorage.setItem(STORAGE_KEY, JSON.stringify(values))
  window.dispatchEvent(new Event(PENDING_CONFIRMATIONS_CHANGED))
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
