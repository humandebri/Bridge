import { render, screen } from "@testing-library/react"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"
import {
  persistRiskAcknowledgement,
  RiskAcknowledgementDialog,
  riskAcknowledgementStorageKey,
} from "./risk-acknowledgement"

describe("RiskAcknowledgementDialog", () => {
  beforeEach(() => window.localStorage.clear())
  afterEach(() => vi.restoreAllMocks())

  it.each([
    ["another deployment", `${riskAcknowledgementStorageKey()}:other`, "acknowledged"],
    [
      "an older copy version",
      riskAcknowledgementStorageKey().replace(".v1:", ".v0:"),
      "acknowledged",
    ],
    ["an invalid active value", riskAcknowledgementStorageKey(), "true"],
  ])("shows for %s", (_case, key, value) => {
    window.localStorage.setItem(key, value)
    render(<RiskAcknowledgementDialog />)
    expect(screen.getByRole("dialog")).toBeVisible()
  })

  it("fails closed when browser storage cannot be read", () => {
    vi.spyOn(Storage.prototype, "getItem").mockImplementation(() => {
      throw new Error("storage unavailable")
    })
    render(<RiskAcknowledgementDialog />)
    expect(screen.getByRole("dialog")).toBeVisible()
  })

  it("does not fail when browser storage cannot be written", () => {
    expect(() =>
      persistRiskAcknowledgement({
        setItem: () => {
          throw new Error("storage unavailable")
        },
      }),
    ).not.toThrow()
  })

  it("does not fail when browser storage cannot be accessed", () => {
    vi.spyOn(window, "localStorage", "get").mockImplementation(() => {
      throw new Error("storage unavailable")
    })
    expect(() => persistRiskAcknowledgement()).not.toThrow()
  })
})
