import { describe, expect, it } from "vitest"
import {
  hasCanonicalMintAuthorizationDeadline,
  mintAuthorizationWindow,
} from "./mint-authorization-window"

describe("mint authorization window", () => {
  it.each([
    { remainingSeconds: 301n, expected: true },
    { remainingSeconds: 300n, expected: true },
    { remainingSeconds: 299n, expected: true },
    { remainingSeconds: 0n, expected: true },
    { remainingSeconds: -1n, expected: false },
  ])("reports $remainingSeconds seconds as $expected", ({ remainingSeconds, expected }) => {
    const deadline = 1_900n
    const result = mintAuthorizationWindow(deadline, deadline - remainingSeconds)

    expect(result).toEqual({
      deadline,
      remainingSeconds,
      isUnexpired: expected,
    })
  })

  it("mint_authorization_window_accepts_deadline_and_rejects_expiry", () => {
    const deadline = 1_900n

    expect(mintAuthorizationWindow(deadline, deadline - 1n).isUnexpired).toBe(true)
    expect(mintAuthorizationWindow(deadline, deadline).isUnexpired).toBe(true)
    expect(mintAuthorizationWindow(deadline, deadline + 1n).isUnexpired).toBe(false)
  })

  it("accepts_only_issued_at_plus_900_seconds_without_nat64_overflow", () => {
    expect(hasCanonicalMintAuthorizationDeadline(1_000n, 1_900n)).toBe(true)
    expect(hasCanonicalMintAuthorizationDeadline(1_001n, 1_900n)).toBe(false)
    expect(hasCanonicalMintAuthorizationDeadline(1_000n, 1_901n)).toBe(false)
    expect(hasCanonicalMintAuthorizationDeadline((1n << 64n) - 1n, (1n << 64n) - 1n)).toBe(false)
  })
})
