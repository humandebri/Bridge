import { describe, expect, it } from "vitest"
import {
  hasCanonicalMintAuthorizationDeadline,
  mintAuthorizationWindow,
} from "./mint-authorization-window"

describe("mint authorization window", () => {
  it("mint_authorization_window_accepts_deadline_and_rejects_expiry", () => {
    const deadline = 1_900n

    for (const [remainingSeconds, isUnexpired] of [
      [1n, true],
      [0n, true],
      [-1n, false],
    ] as const) {
      expect(mintAuthorizationWindow(deadline, deadline - remainingSeconds)).toEqual({
        deadline,
        remainingSeconds,
        isUnexpired,
      })
    }
  })

  it("accepts_only_issued_at_plus_900_seconds_without_nat64_overflow", () => {
    expect(hasCanonicalMintAuthorizationDeadline(1_000n, 1_900n)).toBe(true)
    expect(hasCanonicalMintAuthorizationDeadline(1_001n, 1_900n)).toBe(false)
    expect(hasCanonicalMintAuthorizationDeadline(1_000n, 1_901n)).toBe(false)
    expect(hasCanonicalMintAuthorizationDeadline((1n << 64n) - 1n, (1n << 64n) - 1n)).toBe(false)
  })
})
