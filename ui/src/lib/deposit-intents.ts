import { deploymentProfile } from "@/config/profile"
import type { DepositCall, IcAccount } from "@/lib/ic/wallet"

export type DurableDepositIntentState = "prepared" | "submitted" | "accepted"

export interface DurableDepositIntent {
  account: IcAccount
  recipient: `0x${string}`
  call: DepositCall
  state: DurableDepositIntentState
}

const PREFIX = "kinic.bridge.deposit-intent.v1"

export function readDepositIntent(account: IcAccount): DurableDepositIntent | undefined {
  if (typeof window === "undefined") return undefined
  try {
    const value: unknown = JSON.parse(window.localStorage.getItem(storageKey(account)) ?? "null")
    return isStoredIntent(value) ? fromStored(value) : undefined
  } catch {
    return undefined
  }
}

export function saveDepositIntent(intent: DurableDepositIntent): void {
  window.localStorage.setItem(storageKey(intent.account), JSON.stringify({
    owner: intent.account.owner,
    subaccount: hex(intent.account.subaccount ?? new Uint8Array()),
    recipient: intent.recipient,
    ownerSequence: intent.call.ownerSequence.toString(),
    baseRecipient: hex(intent.call.baseRecipient),
    grossAmount: intent.call.grossAmount.toString(),
    maxServiceFee: intent.call.maxServiceFee.toString(),
    state: intent.state,
  }))
}

export function removeDepositIntent(account: IcAccount): void {
  window.localStorage.removeItem(storageKey(account))
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
