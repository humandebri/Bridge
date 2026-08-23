import { describe, expect, it } from "vitest"
import { decideWithdrawalFinalization } from "./withdrawal-confirmation-state"

describe("decideWithdrawalFinalization", () => {
  it.each([
    ["success", 10n, null, "retry"],
    ["reverted", 10n, null, "retry"],
    ["success", 10n, 9n, "retry"],
    ["reverted", 10n, 9n, "retry"],
    ["success", 10n, 10n, "notify"],
    ["reverted", 10n, 10n, "discard-reverted"],
    ["success", 10n, 11n, "notify"],
    ["reverted", 10n, 11n, "discard-reverted"],
  ] as const)("maps %s at receipt %s with finalized %s to %s", (status, receipt, finalized, expected) => {
    expect(decideWithdrawalFinalization(status, receipt, finalized, true)).toBe(expected)
  })

  it("retries a finalized receipt that is not on the canonical chain", () => {
    expect(decideWithdrawalFinalization("success", 10n, 10n, false)).toBe("retry")
    expect(decideWithdrawalFinalization("reverted", 10n, 10n, false)).toBe("retry")
  })
})
