import { describe, expect, it } from "vitest"
import { automaticConfirmationCheckDate, hasScheduledConfirmation, nextAutomaticConfirmationCheck, shouldPollScheduledHistory } from "./confirmation-schedule"

describe("automatic confirmation schedule", () => {
  it("distinguishes scheduled records from manual recovery records", () => {
    expect(nextAutomaticConfirmationCheck([])).toBeUndefined()
    expect(nextAutomaticConfirmationCheck([25n])).toBe(25n)
    expect(hasScheduledConfirmation([[], [25n]])).toBe(true)
    expect(hasScheduledConfirmation([[], []])).toBe(false)
  })

  it("polls only while a scheduled record is visible", () => {
    expect(shouldPollScheduledHistory(true, true)).toBe(true)
    expect(shouldPollScheduledHistory(true, false)).toBe(false)
    expect(shouldPollScheduledHistory(false, true)).toBe(false)
  })

  it("converts nanoseconds to the displayed wall-clock time", () => {
    expect(automaticConfirmationCheckDate(1_500_000_000n).getTime()).toBe(1_500)
  })
})
