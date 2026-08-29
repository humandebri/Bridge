import { describe, expect, it } from "vitest"
import {
  MINIMUM_MINT_AUTHORIZATION_REMAINING_SECONDS,
  hasCanonicalMintAuthorizationDeadline,
  mintAuthorizationWindow,
} from "./mint-authorization-window"

describe("mint authorization window", () => {
  it.each([
    { remainingSeconds: 301n, expected: true },
    { remainingSeconds: 300n, expected: true },
    { remainingSeconds: 299n, expected: false },
    { remainingSeconds: 0n, expected: false },
    { remainingSeconds: -1n, expected: false },
  ])("reports $remainingSeconds seconds as $expected", ({ remainingSeconds, expected }) => {
    const deadline = 1_600n
    const result = mintAuthorizationWindow(deadline, deadline - remainingSeconds)

    expect(result).toEqual({
      deadline,
      remainingSeconds,
      hasMinimumRemainingTime: expected,
    })
  })

  it("mint_authorization_window_accepts_300_seconds_and_rejects_299_seconds", () => {
    const deadline = 1_600n

    expect(MINIMUM_MINT_AUTHORIZATION_REMAINING_SECONDS).toBe(300n)
    expect(mintAuthorizationWindow(deadline, deadline - 300n).hasMinimumRemainingTime).toBe(true)
    expect(mintAuthorizationWindow(deadline, deadline - 299n).hasMinimumRemainingTime).toBe(false)
  })

  it("accepts_only_issued_at_plus_600_seconds_without_nat64_overflow", () => {
    expect(hasCanonicalMintAuthorizationDeadline(1_000n, 1_600n)).toBe(true)
    expect(hasCanonicalMintAuthorizationDeadline(1_001n, 1_600n)).toBe(false)
    expect(hasCanonicalMintAuthorizationDeadline(1_000n, 1_601n)).toBe(false)
    expect(hasCanonicalMintAuthorizationDeadline((1n << 64n) - 1n, (1n << 64n) - 1n)).toBe(false)
  })
})
