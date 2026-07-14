import { describe, expect, it } from "vitest"
import {
  accountSubaccountHex,
  addPendingWithdrawal,
  assertPendingAccount,
  matchesPendingContext,
  readPendingWithdrawals,
  removePendingWithdrawal,
  type KeyValueStorage,
  type PendingWithdrawalNotification,
} from "@/lib/withdrawal-notifications"
import type { DeploymentProfile } from "@/config/profile"

class MemoryStorage implements KeyValueStorage {
  values = new Map<string, string>()
  getItem(key: string) { return this.values.get(key) ?? null }
  setItem(key: string, value: string) { this.values.set(key, value) }
}

const account = { owner: "aaaaa-aa", subaccount: new Uint8Array(32).fill(7) }
const profile = {
  chainId: 31337,
  bridgeAddress: "0x1111111111111111111111111111111111111111",
} as unknown as DeploymentProfile
const pending: PendingWithdrawalNotification = {
  hash: `0x${"22".repeat(32)}`,
  owner: account.owner,
  subaccount: accountSubaccountHex(account),
  requester: "0x3333333333333333333333333333333333333333",
  chainId: profile.chainId,
  bridgeAddress: profile.bridgeAddress!,
}

describe("pending withdrawal notifications", () => {
  it("persists only valid v1 records", () => {
    const storage = new MemoryStorage()
    addPendingWithdrawal(pending, storage)
    expect(storage.getItem("kinic.bridge.pending-withdrawals.v1")).toBe(JSON.stringify([pending]))
    expect(readPendingWithdrawals(storage)).toEqual([pending])
    removePendingWithdrawal(pending.hash, storage)
    expect(readPendingWithdrawals(storage)).toEqual([])
  })

  it("rejects malformed JSON and incomplete records", () => {
    const storage = new MemoryStorage()
    storage.setItem("kinic.bridge.pending-withdrawals.v1", "{")
    expect(readPendingWithdrawals(storage)).toEqual([])
    storage.setItem("kinic.bridge.pending-withdrawals.v1", JSON.stringify([{ hash: pending.hash, owner: pending.owner }]))
    expect(readPendingWithdrawals(storage)).toEqual([])
  })

  it("rejects a different owner, subaccount, chain, or bridge", () => {
    expect(matchesPendingContext(pending, account, profile)).toBe(true)
    expect(matchesPendingContext(pending, { ...account, owner: "2vxsx-fae" }, profile)).toBe(false)
    expect(matchesPendingContext(pending, { ...account, subaccount: new Uint8Array(32) }, profile)).toBe(false)
    expect(matchesPendingContext(pending, account, { ...profile, chainId: 8453 })).toBe(false)
    expect(matchesPendingContext(pending, account, { ...profile, bridgeAddress: "0x4444444444444444444444444444444444444444" })).toBe(false)
  })

  it("requires the active IC account immediately before notification", () => {
    expect(() => assertPendingAccount(pending, account)).not.toThrow()
    expect(() => assertPendingAccount(pending, { ...account, subaccount: new Uint8Array(32) })).toThrow(/account changed/)
  })
})
