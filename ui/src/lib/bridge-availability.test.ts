import { describe, expect, it } from "vitest"
import { bridgeAvailability, displayReserveSufficient, STATUS_FRESHNESS_MS, statusDataIsFresh } from "./bridge-availability"

describe("bridgeAvailability", () => {
  it("marks deposits unavailable when the canister reserve is insufficient", () => {
    expect(bridgeAvailability({
      runtimeReady: true,
      baseStatus: { depositsPaused: false, withdrawalsPaused: false },
      icDepositsPaused: false,
      reserveSufficient: false,
    })).toEqual({ available: true, toBase: "Unavailable", toIc: "Available" })
  })

  it("keeps withdrawal availability independent from the deposit reserve", () => {
    expect(bridgeAvailability({
      runtimeReady: true,
      baseStatus: { depositsPaused: false, withdrawalsPaused: true },
      icDepositsPaused: false,
      reserveSufficient: true,
    })).toEqual({ available: true, toBase: "Available", toIc: "Paused" })
  })

  it("marks the bridge unavailable when neither direction can start", () => {
    expect(bridgeAvailability({
      runtimeReady: true,
      baseStatus: { depositsPaused: true, withdrawalsPaused: true },
      icDepositsPaused: false,
      reserveSufficient: true,
    })).toEqual({ available: false, toBase: "Paused", toIc: "Paused" })
  })

  it("fails closed while runtime or status data is unavailable", () => {
    expect(bridgeAvailability({ runtimeReady: false })).toEqual({
      available: false,
      toBase: "Unavailable",
      toIc: "Unavailable",
    })
  })

  it("pauses deposits when the IC side is paused", () => {
    expect(bridgeAvailability({
      runtimeReady: true,
      baseStatus: { depositsPaused: false, withdrawalsPaused: false },
      icDepositsPaused: true,
      reserveSufficient: true,
    })).toEqual({ available: true, toBase: "Paused", toIc: "Available" })
  })

  it("fails closed when any observation is older than 60 seconds", () => {
    const now = 100_000
    expect(statusDataIsFresh({ runtimeCheckedAt: now, baseUpdatedAt: now, canisterUpdatedAt: now, now })).toBe(true)
    expect(statusDataIsFresh({ runtimeCheckedAt: now, baseUpdatedAt: now - STATUS_FRESHNESS_MS - 1, canisterUpdatedAt: now, now })).toBe(false)
    expect(statusDataIsFresh({ runtimeCheckedAt: now, baseUpdatedAt: now, canisterUpdatedAt: undefined, now })).toBe(false)
  })

  it("uses the smaller confirmed ETH balance and the IC cycles floor", () => {
    const reserve = { finalizedSignerBalance: 20n, safeSignerBalance: 10n, requiredEthWei: 10n, cyclesBalance: 30n, requiredCycles: 30n }
    expect(displayReserveSufficient(reserve)).toBe(true)
    expect(displayReserveSufficient({ ...reserve, requiredEthWei: 11n })).toBe(false)
    expect(displayReserveSufficient({ ...reserve, requiredCycles: 31n })).toBe(false)
  })
})
