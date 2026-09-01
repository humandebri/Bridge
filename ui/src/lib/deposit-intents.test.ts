import { beforeEach, describe, expect, it, vi } from "vitest"
import { readDepositIntent, removeDepositIntent, saveDepositIntent } from "./deposit-intents"

const account = { owner: "aaaaa-aa", subaccount: new Uint8Array(32).fill(7) }
const intent = {
  account,
  recipient: `0x${"11".repeat(20)}` as const,
  call: {
    ownerSequence: 9n,
    baseRecipient: new Uint8Array(20).fill(0x11),
    grossAmount: 100n,
    maxServiceFee: 10n,
  },
  state: "submitted" as const,
}

describe("durable deposit intents", () => {
  beforeEach(() => window.localStorage.clear())

  it("round trips bigint values in an account-scoped deployment key", async () => {
    await saveDepositIntent(intent)
    expect(readDepositIntent(account)).toEqual(intent)
    expect(readDepositIntent({ owner: account.owner })).toBeUndefined()
  })

  it("removes only the matching account intent", async () => {
    await saveDepositIntent(intent)
    await removeDepositIntent(account)
    expect(readDepositIntent(account)).toBeUndefined()
  })

  it("keeps a failed durable deletion removed for the current session", async () => {
    await saveDepositIntent(intent)
    const originalRemoveItem = window.localStorage.removeItem.bind(window.localStorage)
    const removeItem = vi.spyOn(Storage.prototype, "removeItem").mockImplementation((key) => {
      if (key.startsWith("kinic.bridge.deposit-intent")) throw new Error("storage unavailable")
      return originalRemoveItem(key)
    })

    await removeDepositIntent(account)
    expect(readDepositIntent(account)).toBeUndefined()

    removeItem.mockRestore()
    await removeDepositIntent(account)
  })

  it("prefers a newer session intent when durable replacement fails", async () => {
    await saveDepositIntent(intent)
    const replacement = {
      ...intent,
      recipient: `0x${"22".repeat(20)}` as const,
      state: "accepted" as const,
    }
    const originalSetItem = window.localStorage.setItem.bind(window.localStorage)
    const setItem = vi.spyOn(Storage.prototype, "setItem").mockImplementation((key, value) => {
      if (key.startsWith("kinic.bridge.deposit-intent")) throw new Error("storage unavailable")
      return originalSetItem(key, value)
    })

    await saveDepositIntent(replacement)
    expect(readDepositIntent(account)).toEqual(replacement)

    setItem.mockRestore()
    await removeDepositIntent(account)
  })
})
