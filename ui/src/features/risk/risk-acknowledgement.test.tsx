import { fireEvent, render, screen } from "@testing-library/react"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"
import {
  persistRiskAcknowledgement,
  RiskAcknowledgementDialog,
  riskAcknowledgementStorageKey,
} from "./risk-acknowledgement"

describe("RiskAcknowledgementDialog", () => {
  beforeEach(() => window.localStorage.clear())
  afterEach(() => vi.restoreAllMocks())

  it("blocks the app until the risk is checked and explicitly acknowledged", () => {
    render(<RiskAcknowledgementDialog />)

    expect(screen.getByRole("heading", { name: "Unaudited bridge" })).toBeVisible()
    expect(screen.queryByRole("button", { name: "Close confirmation" })).not.toBeInTheDocument()
    const continueButton = screen.getByRole("button", { name: "Acknowledge and continue" })
    expect(continueButton).toBeDisabled()
    expect(window.localStorage.getItem(riskAcknowledgementStorageKey())).toBeNull()

    fireEvent.keyDown(document, { key: "Escape" })
    expect(screen.getByRole("dialog")).toBeVisible()

    fireEvent.click(screen.getByRole("checkbox", { name: "Acknowledge unaudited bridge risk" }))
    expect(window.localStorage.getItem(riskAcknowledgementStorageKey())).toBeNull()
    expect(continueButton).toBeEnabled()
    fireEvent.click(continueButton)

    expect(screen.queryByRole("dialog")).not.toBeInTheDocument()
    expect(window.localStorage.getItem(riskAcknowledgementStorageKey())).toBe("acknowledged")
  })

  it("does not show again for the active deployment after acknowledgement", () => {
    window.localStorage.setItem(riskAcknowledgementStorageKey(), "acknowledged")
    render(<RiskAcknowledgementDialog />)
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument()
  })

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
