import type { DeploymentProfile } from "@/config/profile"
import type { IcAccount } from "@/lib/ic/wallet"
import { sameIcAccount } from "@/lib/wallet-snapshot"

const STORAGE_KEY = "kinic.bridge.pending-withdrawals.v1"
const HASH = /^0x[0-9a-fA-F]{64}$/
const ADDRESS = /^0x[0-9a-fA-F]{40}$/
const SUBACCOUNT = /^0x[0-9a-fA-F]{64}$/

export interface PendingWithdrawalNotification {
  hash: `0x${string}`
  owner: string
  subaccount: `0x${string}`
  requester: `0x${string}`
  chainId: number
  bridgeAddress: `0x${string}`
}

export interface KeyValueStorage {
  getItem(key: string): string | null
  setItem(key: string, value: string): void
}

export function accountSubaccountHex(account: IcAccount): `0x${string}` {
  const bytes = account.subaccount ?? new Uint8Array(32)
  return `0x${Array.from(bytes, (value) => value.toString(16).padStart(2, "0")).join("")}`
}

export function readPendingWithdrawals(storage: KeyValueStorage = localStorage): PendingWithdrawalNotification[] {
  try {
    const value: unknown = JSON.parse(storage.getItem(STORAGE_KEY) ?? "[]")
    if (!Array.isArray(value)) return []
    return value.filter(isPendingWithdrawal)
  } catch {
    return []
  }
}

export function addPendingWithdrawal(item: PendingWithdrawalNotification, storage: KeyValueStorage = localStorage): void {
  writePendingWithdrawals(
    [...readPendingWithdrawals(storage).filter((current) => current.hash.toLowerCase() !== item.hash.toLowerCase()), item],
    storage,
  )
}

export function removePendingWithdrawal(hash: `0x${string}`, storage: KeyValueStorage = localStorage): void {
  writePendingWithdrawals(readPendingWithdrawals(storage).filter((item) => item.hash.toLowerCase() !== hash.toLowerCase()), storage)
}

export function matchesPendingContext(
  pending: PendingWithdrawalNotification,
  account: IcAccount,
  profile: DeploymentProfile,
): boolean {
  if (!profile.bridgeAddress) return false
  return pending.owner === account.owner
    && pending.subaccount.toLowerCase() === accountSubaccountHex(account).toLowerCase()
    && pending.chainId === profile.chainId
    && pending.bridgeAddress.toLowerCase() === profile.bridgeAddress.toLowerCase()
}

export function assertPendingAccount(pending: PendingWithdrawalNotification, current: IcAccount): void {
  const expected: IcAccount = {
    owner: pending.owner,
    subaccount: hexBytes(pending.subaccount),
  }
  if (!sameIcAccount(current, expected)) {
    throw new Error("The IC wallet account changed; reconnect the original destination before notifying settlement")
  }
}

function writePendingWithdrawals(items: PendingWithdrawalNotification[], storage: KeyValueStorage): void {
  storage.setItem(STORAGE_KEY, JSON.stringify(items))
}

function isPendingWithdrawal(value: unknown): value is PendingWithdrawalNotification {
  if (typeof value !== "object" || value === null) return false
  const item = value as Record<string, unknown>
  return typeof item.hash === "string" && HASH.test(item.hash)
    && typeof item.owner === "string" && item.owner.length > 0
    && typeof item.subaccount === "string" && SUBACCOUNT.test(item.subaccount)
    && typeof item.requester === "string" && ADDRESS.test(item.requester)
    && typeof item.chainId === "number" && Number.isSafeInteger(item.chainId) && item.chainId > 0
    && typeof item.bridgeAddress === "string" && ADDRESS.test(item.bridgeAddress)
}

function hexBytes(value: string): Uint8Array {
  return Uint8Array.from(value.slice(2).match(/../g) ?? [], (byte) => Number.parseInt(byte, 16))
}
