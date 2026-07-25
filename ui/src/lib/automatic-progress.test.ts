import { describe, expect, it } from "vitest"
import { automaticProgressInfo, relativeTime } from "@/routes/history"
import type { AutomaticProgressView } from "@/generated/bridge.did"

describe("automatic settlement progress", () => {
  it("formats activity timestamps relative to the browser clock", () => {
    const nowMs = 1_000_000

    expect(relativeTime(BigInt(nowMs - 5 * 60_000) * 1_000_000n, nowMs)).toBe("5 minutes ago")
  })

  it("shows an explicitly claimed confirmation as verification work", () => {
    const value: [AutomaticProgressView] = [{ phase: { Confirmation: null }, state: { Running: { lease_until_ns: 100n } } }]
    expect(automaticProgressInfo(value, 99n)).toMatchObject({ label: "Verifying confirmation", retryAllowed: false, running: true })
    expect(automaticProgressInfo(value, 100n)).toMatchObject({ retryAllowed: true })
  })

  it("shows settlement work separately and permits recovery after lease expiry", () => {
    const value: [AutomaticProgressView] = [{ phase: { Settlement: null }, state: { Running: { lease_until_ns: 200n } } }]
    expect(automaticProgressInfo(value, 199n)).toMatchObject({ label: "Completing automatically", retryAllowed: false, running: true })
    expect(automaticProgressInfo(value, 200n)).toMatchObject({ retryAllowed: true })
  })
})
