import { render, screen } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { useState } from "react"
import { describe, expect, it, vi } from "vitest"
import { BridgeConfirmationDialog, type BridgeDirection } from "./bridge-page"

function Harness({ direction, onConfirm = vi.fn() }: { direction: BridgeDirection; onConfirm?: () => void }) {
  const [disclosed, setDisclosed] = useState(false)
  return <BridgeConfirmationDialog direction={direction} open setOpen={() => undefined} disclosed={disclosed} setDisclosed={setDisclosed} source="source-wallet" destination="destination-wallet" amount="10" receive={9n} pending={false} onConfirm={onConfirm} />
}

describe("BridgeConfirmationDialog", () => {
  it("requires the Base governance disclosure before a deposit can continue", async () => {
    render(<Harness direction="deposit" />)
    const confirm = screen.getByRole("button", { name: "Confirm and open wallet" })
    expect(confirm).toBeDisabled()
    await userEvent.click(screen.getByRole("checkbox"))
    expect(confirm).toBeEnabled()
  })

  it("shows the irreversible burn warning before a withdrawal", () => {
    render(<Harness direction="withdraw" />)
    expect(screen.getByText(/Burning KINIC on Base is irreversible/)).toBeVisible()
    expect(screen.getByRole("button", { name: "Confirm burn" })).toBeEnabled()
  })
})
