import { describe, expect, it } from "vitest"
import {
  bridgeAvailability,
  displayCyclesSufficient,
  STATUS_FRESHNESS_MS,
  statusDataIsFresh,
} from "./bridge-availability"

describe("bridgeAvailability", () => {
  it("marks both asset directions unavailable when the cycles floor is insufficient", () => {
    expect(
      bridgeAvailability({
        observationsAccepted: true,
        baseStatus: { depositsPaused: false, withdrawalsPaused: false },
        icDepositsPaused: false,
        cyclesSufficient: false,
      }),
    ).toEqual({
      status: "Unavailable",
      available: false,
      toBase: "Unavailable",
      toIc: "Unavailable",
    })
  })

  it("keeps withdrawal availability independent from the deposit reserve", () => {
    expect(
      bridgeAvailability({
        observationsAccepted: true,
        baseStatus: { depositsPaused: false, withdrawalsPaused: true },
        icDepositsPaused: false,
        cyclesSufficient: true,
      }),
    ).toEqual({ status: "Available", available: true, toBase: "Available", toIc: "Paused" })
  })

  it("marks the bridge unavailable when neither direction can start", () => {
    expect(
      bridgeAvailability({
        observationsAccepted: true,
        baseStatus: { depositsPaused: true, withdrawalsPaused: true },
        icDepositsPaused: false,
        cyclesSufficient: true,
      }),
    ).toEqual({ status: "Paused", available: false, toBase: "Paused", toIc: "Paused" })
  })

  it("fails closed while runtime or status data is unavailable", () => {
    expect(bridgeAvailability({ observationsAccepted: false })).toEqual({
      status: "Unknown",
      available: false,
      toBase: "Unknown",
      toIc: "Unknown",
    })
  })

  it("pauses deposits when the IC side is paused", () => {
    expect(
      bridgeAvailability({
        observationsAccepted: true,
        baseStatus: { depositsPaused: false, withdrawalsPaused: false },
        icDepositsPaused: true,
        cyclesSufficient: true,
      }),
    ).toEqual({ status: "Available", available: true, toBase: "Paused", toIc: "Available" })
  })

  it("fails closed when any observation is older than 60 seconds", () => {
    const now = 100_000
    expect(statusDataIsFresh({ baseUpdatedAt: now, canisterUpdatedAt: now, now })).toBe(true)
    expect(
      statusDataIsFresh({
        baseUpdatedAt: now - STATUS_FRESHNESS_MS - 1,
        canisterUpdatedAt: now,
        now,
      }),
    ).toBe(false)
    expect(statusDataIsFresh({ baseUpdatedAt: now, canisterUpdatedAt: undefined, now })).toBe(false)
  })

  it("uses only the IC cycles floor for asset-transfer availability", () => {
    const reserve = { cyclesBalance: 30n, requiredCycles: 30n }
    expect(displayCyclesSufficient(reserve)).toBe(true)
    expect(displayCyclesSufficient({ ...reserve, requiredCycles: 31n })).toBe(false)
  })
})
