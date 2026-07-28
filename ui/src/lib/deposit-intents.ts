import { deploymentProfile } from "@/config/profile"
import type { DepositCall, IcAccount } from "@/lib/ic/wallet"
import { withBrowserLock } from "@/lib/browser-lock"

export type DurableDepositIntentState = "prepared" | "submitted" | "accepted"

export interface DurableDepositIntent {
  account: IcAccount
  recipient: `0x${string}`
  call: DepositCall
  state: DurableDepositIntentState
}

const PREFIX = "kinic.bridge.deposit-intent.v2"
const sessionIntents = new Map<string, DurableDepositIntent>()
const removedSessionIntents = new Set<string>()

export function readDepositIntent(account: IcAccount): DurableDepositIntent | undefined {
  if (typeof window === "undefined") return undefined
  const key = storageKey(account)
  if (removedSessionIntents.has(key)) return undefined
  const sessionIntent = sessionIntents.get(key)
  if (sessionIntent) return cloneIntent(sessionIntent)
  try {
    const value: unknown = JSON.parse(window.localStorage.getItem(key) ?? "null")
    return isStoredIntent(value) ? fromStored(value) : undefined
  } catch {
    return undefined
  }
}

export async function saveDepositIntent(intent: DurableDepositIntent): Promise<void> {
  const key = storageKey(intent.account)
  removedSessionIntents.delete(key)
  sessionIntents.set(key, cloneIntent(intent))
  await withBrowserLock(`kinic-storage:${key}`, () => {
    try {
      window.localStorage.setItem(key, JSON.stringify({
        owner: intent.account.owner,
        subaccount: hex(intent.account.subaccount ?? new Uint8Array()),
        recipient: intent.recipient,
        ownerSequence: intent.call.ownerSequence.toString(),
        baseRecipient: hex(intent.call.baseRecipient),
        grossAmount: intent.call.grossAmount.toString(),
        maxServiceFee: intent.call.maxServiceFee.toString(),
        state: intent.state,
      }))
      sessionIntents.delete(key)
    } catch { /* Session memory still prevents an automatic duplicate prompt. */ }
  })
}

export async function removeDepositIntent(account: IcAccount): Promise<void> {
  const key = storageKey(account)
  sessionIntents.delete(key)
  removedSessionIntents.add(key)
  await withBrowserLock(`kinic-storage:${key}`, () => {
    try {
      window.localStorage.removeItem(key)
      removedSessionIntents.delete(key)
    } catch { /* Already removed from session memory. */ }
  })
}

function storageKey(account: IcAccount): string {
  return [
    PREFIX,
    deploymentProfile.chainId,
    deploymentProfile.bridgeAddress?.toLowerCase() ?? "",
    deploymentProfile.bridgeCanisterId ?? "",
    account.owner,
    hex(account.subaccount ?? new Uint8Array()),
  ].join(":")
}

interface StoredIntent {
  owner: string
  subaccount: string
  recipient: `0x${string}`
  ownerSequence: string
  baseRecipient: string
  grossAmount: string
  maxServiceFee: string
  state: DurableDepositIntentState
}

function isStoredIntent(value: unknown): value is StoredIntent {
  if (typeof value !== "object" || value === null) return false
  const item = value as Record<string, unknown>
  return typeof item.owner === "string"
    && typeof item.subaccount === "string" && /^0x(?:[0-9a-fA-F]{64})?$/.test(item.subaccount)
    && typeof item.recipient === "string" && /^0x[0-9a-fA-F]{40}$/.test(item.recipient)
    && typeof item.ownerSequence === "string" && /^\d+$/.test(item.ownerSequence)
    && typeof item.baseRecipient === "string" && /^0x[0-9a-fA-F]{40}$/.test(item.baseRecipient)
    && typeof item.grossAmount === "string" && /^\d+$/.test(item.grossAmount)
    && typeof item.maxServiceFee === "string" && /^\d+$/.test(item.maxServiceFee)
    && ["prepared", "submitted", "accepted"].includes(String(item.state))
}

function fromStored(value: StoredIntent): DurableDepositIntent {
  return {
    account: {
      owner: value.owner,
      subaccount: value.subaccount === "0x" ? undefined : bytes(value.subaccount),
    },
    recipient: value.recipient,
    call: {
      ownerSequence: BigInt(value.ownerSequence),
      baseRecipient: bytes(value.baseRecipient),
      grossAmount: BigInt(value.grossAmount),
      maxServiceFee: BigInt(value.maxServiceFee),
    },
    state: value.state,
  }
}

function hex(value: Uint8Array | number[]): string {
  return `0x${Array.from(value, (byte) => Number(byte).toString(16).padStart(2, "0")).join("")}`
}

function bytes(value: string): Uint8Array {
  return Uint8Array.from(value.slice(2).match(/.{2}/g) ?? [], (byte) => Number.parseInt(byte, 16))
}

function cloneIntent(intent: DurableDepositIntent): DurableDepositIntent {
  return {
    ...intent,
    account: { ...intent.account, subaccount: intent.account.subaccount?.slice() },
    call: { ...intent.call, baseRecipient: intent.call.baseRecipient.slice() },
  }
}
