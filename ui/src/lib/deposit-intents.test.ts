import { beforeEach, describe, expect, it } from "vitest"
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

  it("round trips bigint values in an account-scoped deployment key", () => {
    saveDepositIntent(intent)
    expect(readDepositIntent(account)).toEqual(intent)
    expect(readDepositIntent({ owner: account.owner })).toBeUndefined()
  })

  it("removes only the matching account intent", () => {
    saveDepositIntent(intent)
    removeDepositIntent(account)
    expect(readDepositIntent(account)).toBeUndefined()
  })
})
