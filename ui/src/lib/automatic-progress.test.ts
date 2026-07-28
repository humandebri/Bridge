import { describe, expect, it } from "vitest"
import { automaticProgressInfo, relativeTime } from "@/routes/history"
import type { AutomaticProgressView } from "@/generated/bridge.did"

describe("automatic settlement progress", () => {
  it("formats activity timestamps relative to the browser clock", () => {
    const nowMs = 1_000_000

    expect(relativeTime(BigInt(nowMs - 5 * 60_000) * 1_000_000n, nowMs)).toBe("5 minutes ago")
  })

  it("shows running settlement work and permits recovery after lease expiry", () => {
    const value: [AutomaticProgressView] = [{ state: { Running: { lease_until_ns: 100n } } }]
    expect(automaticProgressInfo(value, 99n)).toMatchObject({ label: "Completing automatically", retryAllowed: false, running: true })
    expect(automaticProgressInfo(value, 100n)).toMatchObject({ retryAllowed: true })
  })

  it("keeps scheduled work automatic until its recovery grace period", () => {
    const value: [AutomaticProgressView] = [{ state: { Scheduled: { next_run_at_ns: 200n } } }]
    expect(automaticProgressInfo(value, 300_000_000_199n)).toMatchObject({ label: "Completing automatically", retryAllowed: false, running: false })
    expect(automaticProgressInfo(value, 300_000_000_200n)).toMatchObject({ retryAllowed: true })
  })
})
