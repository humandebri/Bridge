import { describe, expect, it } from "vitest"
import { classifyDepositRecoverySequence } from "./deposit-recovery"

describe("deposit recovery sequence", () => {
  it("unlocks only when the attempted sequence is still next", () => {
    expect(classifyDepositRecoverySequence(7n, 7n)).toBe("not-accepted")
  })

  it("keeps recovery locked when the owner sequence advanced", () => {
    expect(classifyDepositRecoverySequence(7n, 8n)).toBe("accepted-or-conflicted")
    expect(classifyDepositRecoverySequence(7n, 10n)).toBe("accepted-or-conflicted")
  })

  it("fails closed when the observed sequence moves backwards or is unavailable", () => {
    expect(classifyDepositRecoverySequence(7n, 6n)).toBe("invalid")
    expect(classifyDepositRecoverySequence(7n, undefined)).toBe("invalid")
  })
})
