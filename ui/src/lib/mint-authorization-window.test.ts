import { describe, expect, it } from "vitest"
import {
  MINIMUM_MINT_AUTHORIZATION_REMAINING_SECONDS,
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
    const finalizedBlockTimestamp = 1_000n
    const deadline = finalizedBlockTimestamp + 900n
    const result = mintAuthorizationWindow(finalizedBlockTimestamp, deadline - remainingSeconds)

    expect(result).toEqual({
      deadline,
      remainingSeconds,
      hasMinimumRemainingTime: expected,
    })
  })

  it("mint_authorization_window_accepts_300_seconds_and_rejects_299_seconds", () => {
    const finalizedBlockTimestamp = 1_000n
    const deadline = finalizedBlockTimestamp + 900n

    expect(MINIMUM_MINT_AUTHORIZATION_REMAINING_SECONDS).toBe(300n)
    expect(mintAuthorizationWindow(finalizedBlockTimestamp, deadline - 300n).hasMinimumRemainingTime).toBe(true)
    expect(mintAuthorizationWindow(finalizedBlockTimestamp, deadline - 299n).hasMinimumRemainingTime).toBe(false)
  })
})
