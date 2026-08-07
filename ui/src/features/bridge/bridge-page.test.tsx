import { cleanup, fireEvent, render, screen } from "@testing-library/react"
import { afterEach, describe, expect, it, vi } from "vitest"
import { BridgeConfirmationDialog, isDepositAuthorizationPending, type BridgeDirection } from "./bridge-page"

afterEach(cleanup)

function Harness({ direction, approvalNeeded, onConfirm = vi.fn() }: { direction: BridgeDirection; approvalNeeded?: boolean; onConfirm?: () => void }) {
  const sendSymbol = direction === "deposit" ? "TICRC1" : "KINIC"
  const receiveSymbol = direction === "deposit" ? "KINIC" : "TICRC1"
  return <BridgeConfirmationDialog
    direction={direction}
    open
    setOpen={() => undefined}
    preflight={{
      runId: 1,
      direction,
      phase: "ready",
      checks: [
        { id: "wallets", label: "Wallets connected", status: "passed" },
        { id: "runtime", label: "Bridge configuration verified", status: "passed" },
        { id: "financials", label: "Balance and fees checked", status: "passed" },
        { id: "availability", label: "Transfer availability checked", status: "passed" },
      ],
    }}
    source="source-wallet"
    destination="destination-wallet"
    amount="10"
    receive={9n}
    fee={1n}
    sendSymbol={sendSymbol}
    receiveSymbol={receiveSymbol}
    approvalNeeded={approvalNeeded}
    pending={false}
    onRetry={vi.fn()}
    onConfirm={onConfirm}
  />
}

describe("BridgeConfirmationDialog", () => {
  it("shows live preflight progress before exposing transaction confirmation", () => {
    render(<BridgeConfirmationDialog
      direction="deposit"
      open
      setOpen={() => undefined}
      preflight={{
        runId: 1,
        direction: "deposit",
        phase: "checking",
        checks: [
          { id: "wallets", label: "Wallets connected", status: "passed" },
          { id: "runtime", label: "Bridge configuration verified", status: "checking" },
          { id: "financials", label: "Balance and fees checked", status: "waiting" },
          { id: "availability", label: "Transfer availability checked", status: "waiting" },
        ],
      }}
      source="source-wallet"
      destination="destination-wallet"
      amount="10"
      receive={9n}
      sendSymbol="TICRC1"
      receiveSymbol="KINIC"
      pending={false}
      onRetry={vi.fn()}
      onConfirm={vi.fn()}
    />)

    expect(screen.getByText("Checking current bridge conditions. No transaction has been sent.")).toBeVisible()
    expect(screen.getByText("Checking your wallets, balance, fees, and bridge availability…")).toBeVisible()
    expect(screen.queryByText("Wallets connected")).not.toBeInTheDocument()
    expect(screen.queryByRole("button", { name: "Continue to IC wallet" })).not.toBeInTheDocument()
  })

  it("keeps a failed check visible and offers a retry", () => {
    const onRetry = vi.fn()
    render(<BridgeConfirmationDialog
      direction="withdraw"
      open
      setOpen={() => undefined}
      preflight={{
        runId: 1,
        direction: "withdraw",
        phase: "failed",
        checks: [
          { id: "wallets", label: "Wallets connected", status: "passed" },
          { id: "runtime", label: "Bridge configuration verified", status: "failed", error: "Bridge signer differs from the reviewed profile" },
          { id: "financials", label: "Balance and fees checked", status: "waiting" },
          { id: "availability", label: "Transfer availability checked", status: "waiting" },
        ],
      }}
      source="source-wallet"
      destination="destination-wallet"
      amount="10"
      receive={9n}
      sendSymbol="KINIC"
      receiveSymbol="TICRC1"
      pending={false}
      onRetry={onRetry}
      onConfirm={vi.fn()}
    />)

    expect(screen.getByText("No transaction was sent.")).toBeVisible()
    expect(screen.getByText("Bridge signer differs from the reviewed profile")).toBeVisible()
    fireEvent.click(screen.getByRole("button", { name: "Try again" }))
    expect(onRetry).toHaveBeenCalledOnce()
    expect(screen.getByRole("button", { name: "Close" })).toBeVisible()
  })

  it("lets a deposit continue after reviewing its wallets and amount", () => {
    render(<Harness direction="deposit" />)
    expect(screen.getByRole("heading", { name: "Review bridge to Base" })).toBeVisible()
    expect(screen.getByText("Review the transfer and the wallet actions that come next.")).toBeVisible()
    expect(screen.getByText("10 TICRC1")).toBeVisible()
    expect(screen.getByText("0.00000009 KINIC")).toBeVisible()
    expect(screen.queryByText("Wallets connected")).not.toBeInTheDocument()
    expect(screen.queryByText(/initial pull Ledger fee is never refunded/)).not.toBeInTheDocument()
    const confirm = screen.getByRole("button", { name: "Continue to IC wallet" })
    expect(confirm).toBeEnabled()
  })

  it("requires an irreversible burn acknowledgement for a withdrawal", () => {
    render(<Harness direction="withdraw" />)
    expect(screen.getByText("10 KINIC")).toBeVisible()
    expect(screen.getByText("0.00000009 TICRC1")).toBeVisible()
    expect(screen.getByRole("heading", { name: "Review bridge to IC" })).toBeVisible()
    expect(screen.getAllByRole("listitem").map((step) => step.textContent)).toEqual([
      "1. Allow the bridge to use KINIC in your Base wallet.",
      "2. Confirm the withdrawal transaction in your Base wallet.",
      "3. After Base finality, the browser automatically notifies the Bridge. No IC wallet confirmation is needed for the ledger payout.",
    ])
    const confirm = screen.getByRole("button", { name: "Continue to Base wallet" })
    expect(confirm).toBeDisabled()
    fireEvent.click(screen.getByRole("checkbox", { name: "Acknowledge irreversible burn" }))
    expect(confirm).toBeEnabled()
  })

  it("omits the token access step when the reviewed allowance is already sufficient", () => {
    render(<Harness direction="deposit" approvalNeeded={false} />)

    expect(screen.queryByText(/Allow the bridge to use TICRC1/)).not.toBeInTheDocument()
    expect(screen.getByText("Confirm the deposit request in your IC wallet.")).toBeVisible()
  })
})

describe("isDepositAuthorizationPending", () => {
  it("keeps the deposit action pending until its Mint Authorization is available", () => {
    expect(isDepositAuthorizationPending({ EscrowedUnquoted: null })).toBe(true)
    expect(isDepositAuthorizationPending({ AuthorizationPending: null })).toBe(true)
    expect(isDepositAuthorizationPending({ AuthorizationAvailable: null })).toBe(false)
  })
})
