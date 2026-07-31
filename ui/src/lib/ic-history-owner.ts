import { Principal } from "@dfinity/principal"
import { deploymentProfile } from "@/config/profile"
import type { IcAccount, IcWalletProvider } from "@/lib/ic/wallet"

const STORAGE_VERSION = 1

interface StoredIcHistoryOwner {
  version: typeof STORAGE_VERSION
  owner: string
  subaccount: string | null
  provider: IcWalletProvider
}

export interface IcHistoryOwner {
  account: IcAccount
  provider: IcWalletProvider
}

export function icHistoryOwnerStorageKey(): string {
  return [
    "kinic.bridge.ic-history-owner.v1",
    deploymentProfile.environment,
    deploymentProfile.chainId,
    deploymentProfile.bridgeCanisterId ?? "",
  ].join(":")
}

export function loadIcHistoryOwner(storage?: Pick<Storage, "getItem">): IcHistoryOwner | undefined {
  try {
    const target = storage ?? window.localStorage
    const raw = target.getItem(icHistoryOwnerStorageKey())
    if (raw === null) return undefined
    const value: unknown = JSON.parse(raw)
    if (!isStoredOwner(value)) return undefined
    const owner = Principal.fromText(value.owner).toText()
    if (owner !== value.owner) return undefined
    return {
      account: {
        owner,
        subaccount: value.subaccount === null ? undefined : hexBytes(value.subaccount),
      },
      provider: value.provider,
    }
  } catch {
    return undefined
  }
}

export function saveIcHistoryOwner(
  owner: IcHistoryOwner,
  storage?: Pick<Storage, "setItem">,
): void {
  try {
    const target = storage ?? window.localStorage
    const value: StoredIcHistoryOwner = {
      version: STORAGE_VERSION,
      owner: Principal.fromText(owner.account.owner).toText(),
      subaccount: owner.account.subaccount ? bytesHex(owner.account.subaccount) : null,
      provider: owner.provider,
    }
    target.setItem(icHistoryOwnerStorageKey(), JSON.stringify(value))
  } catch {
    // Read-only history persistence must never break a successful wallet connection.
  }
}

export function clearIcHistoryOwner(storage?: Pick<Storage, "removeItem">): void {
  try {
    const target = storage ?? window.localStorage
    target.removeItem(icHistoryOwnerStorageKey())
  } catch {
    // A storage failure must not prevent the active wallet session from disconnecting.
  }
}

export function sameIcAccount(left: IcAccount | undefined, right: IcAccount | undefined): boolean {
  if (!left || !right || left.owner !== right.owner) return false
  const leftSubaccount = left.subaccount ?? new Uint8Array()
  const rightSubaccount = right.subaccount ?? new Uint8Array()
  return leftSubaccount.length === rightSubaccount.length
    && leftSubaccount.every((value, index) => value === rightSubaccount[index])
}

function isStoredOwner(value: unknown): value is StoredIcHistoryOwner {
  if (!value || typeof value !== "object") return false
  const item = value as Partial<StoredIcHistoryOwner>
  return item.version === STORAGE_VERSION
    && typeof item.owner === "string"
    && (item.provider === "oisy" || item.provider === "plug")
    && (item.subaccount === null || (typeof item.subaccount === "string" && /^[0-9a-f]{64}$/.test(item.subaccount)))
}

function bytesHex(value: Uint8Array): string {
  return [...value].map((byte) => byte.toString(16).padStart(2, "0")).join("")
}

function hexBytes(value: string): Uint8Array {
  return Uint8Array.from(value.match(/.{2}/g)?.map((byte) => Number.parseInt(byte, 16)) ?? [])
}
