import { fireEvent, render, screen } from "@testing-library/react"
import { describe, expect, it, vi } from "vitest"
import { BridgeConfirmationDialog, type BridgeDirection } from "./bridge-page"

function Harness({ direction, onConfirm = vi.fn() }: { direction: BridgeDirection; onConfirm?: () => void }) {
  const sendSymbol = direction === "deposit" ? "TICRC1" : "KINIC"
  const receiveSymbol = direction === "deposit" ? "KINIC" : "TICRC1"
  return <BridgeConfirmationDialog direction={direction} open setOpen={() => undefined} source="source-wallet" destination="destination-wallet" amount="10" receive={9n} sendSymbol={sendSymbol} receiveSymbol={receiveSymbol} pending={false} onConfirm={onConfirm} />
}

describe("BridgeConfirmationDialog", () => {
  it("lets a deposit continue after reviewing its wallets and amount", () => {
    render(<Harness direction="deposit" />)
    expect(screen.getByText("10 TICRC1 / 0.00000009 KINIC")).toBeVisible()
    const confirm = screen.getByRole("button", { name: "Confirm and open wallet" })
    expect(confirm).toBeEnabled()
  })

  it("requires an irreversible burn acknowledgement for a withdrawal", () => {
    render(<Harness direction="withdraw" />)
    expect(screen.getByText("10 KINIC / 0.00000009 TICRC1")).toBeVisible()
    expect(screen.getByRole("heading", { name: "Confirm bridge to IC" })).toBeVisible()
    const confirm = screen.getByRole("button", { name: "Confirm and open wallet" })
    expect(confirm).toBeDisabled()
    fireEvent.click(screen.getByRole("checkbox", { name: "Acknowledge irreversible burn" }))
    expect(confirm).toBeEnabled()
  })
})
