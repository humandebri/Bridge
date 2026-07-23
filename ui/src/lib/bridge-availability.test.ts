import { describe, expect, it } from "vitest"
import { bridgeAvailability } from "./bridge-availability"

describe("bridgeAvailability", () => {
  it("marks deposits unavailable when the canister reserve is insufficient", () => {
    expect(bridgeAvailability({
      runtimeReady: true,
      baseStatus: { depositsPaused: false, withdrawalsPaused: false },
      reserveSufficient: false,
    })).toEqual({ available: true, toBase: "Unavailable", toIc: "Available" })
  })

  it("keeps withdrawal availability independent from the deposit reserve", () => {
    expect(bridgeAvailability({
      runtimeReady: true,
      baseStatus: { depositsPaused: false, withdrawalsPaused: true },
      reserveSufficient: true,
    })).toEqual({ available: true, toBase: "Available", toIc: "Paused" })
  })

  it("marks the bridge unavailable when neither direction can start", () => {
    expect(bridgeAvailability({
      runtimeReady: true,
      baseStatus: { depositsPaused: true, withdrawalsPaused: true },
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
})
