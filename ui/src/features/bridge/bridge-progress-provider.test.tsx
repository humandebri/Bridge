import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react"
import { useState } from "react"
import { afterEach, beforeEach, describe, expect, it } from "vitest"
import { browserLocalStorage } from "@/lib/browser-lock"
import { createBridgeProgress, saveLatestBridgeProgress } from "@/lib/bridge-progress"
import { BridgeProgressProvider, useBridgeProgress } from "./bridge-progress-provider"

beforeEach(() => browserLocalStorage().clear())
afterEach(() => {
  cleanup()
  browserLocalStorage().clear()
})

function Harness() {
  const progress = useBridgeProgress()
  const [startError, setStartError] = useState<string>()
  return <div>
    <button type="button" onClick={() => progress.start({
      direction: "deposit",
      phase: "awaiting-ic-deposit",
      source: "aaaaa-aa",
      destination: "0x0000000000000000000000000000000000000002",
      sendAmount: "2",
      receiveAmount: "1.5",
      sendSymbol: "TICRC1",
      receiveSymbol: "KINIC",
      deposit: { owner: "aaaaa-aa", ownerSequence: "3" },
    })}>Start</button>
    <button type="button" onClick={() => {
      try {
        progress.start({
          direction: "withdraw",
          phase: "awaiting-base-withdrawal",
          source: "0x0000000000000000000000000000000000000002",
          destination: "aaaaa-aa",
          sendAmount: "2",
          receiveAmount: "1.5",
          sendSymbol: "KINIC",
          receiveSymbol: "TICRC1",
          withdrawal: { owner: "aaaaa-aa" },
        })
      } catch (error) {
        setStartError(error instanceof Error ? error.message : "blocked")
      }
    }}>Start another</button>
    <button type="button" onClick={() => progress.progress && progress.update(progress.progress.id, { phase: "complete" })}>Complete</button>
    <button type="button" onClick={() => progress.progress && progress.update(progress.progress.id, {
      phase: "base-mint-included",
      transactionHash: `0x${"22".repeat(32)}`,
      receiptBlockNumber: "123",
      baseTransactionOutcome: "success",
    })}>Base included</button>
    <button type="button" onClick={() => progress.progress && progress.setAction(progress.progress.id, { label: "Retry transfer", run: () => undefined })}>Register action</button>
    {startError && <p>{startError}</p>}
  </div>
}

describe("BridgeProgressProvider", () => {
  it("keeps an active transfer modal open, allows minimizing, and only exposes close after completion", async () => {
    render(<BridgeProgressProvider><Harness /></BridgeProgressProvider>)
    fireEvent.click(screen.getByRole("button", { name: "Start" }))

    expect(screen.getByRole("dialog", { name: "Bridge to Base" })).toBeVisible()
    expect(screen.queryByText("Keep this window open, or minimize it while the transfer continues.")).not.toBeInTheDocument()
    expect(screen.queryByRole("button", { name: "Close confirmation" })).not.toBeInTheDocument()
    fireEvent.keyDown(document, { key: "Escape" })
    expect(screen.getByRole("dialog", { name: "Bridge to Base" })).toBeVisible()

    fireEvent.click(screen.getByRole("button", { name: "Minimize" }))
    const bar = screen.getByRole("button", { name: /Open transfer progress/ })
    expect(bar).toHaveTextContent("Confirm the deposit in your IC wallet")
    fireEvent.click(bar)
    expect(screen.getByRole("dialog", { name: "Bridge to Base" })).toBeVisible()

    await new Promise((resolve) => window.setTimeout(resolve, 0))
    fireEvent.pointerDown(document.querySelector('[data-state="open"][aria-hidden="true"]')!)
    await waitFor(() => expect(screen.queryByRole("dialog", { name: "Bridge to Base" })).not.toBeInTheDocument())
    fireEvent.click(screen.getByRole("button", { name: /Open transfer progress/ }))

    fireEvent.click(screen.getByRole("button", { name: "Complete", hidden: true }))
    expect(screen.getByText("Base transaction")).toBeVisible()
    expect(screen.queryByText("Base finality")).not.toBeInTheDocument()
    expect(screen.getByRole("button", { name: "Close" })).toBeEnabled()
  })

  it("dismisses the Deposit modal from the backdrop after a successful Base receipt and preserves pending recovery state", async () => {
    const storage = browserLocalStorage()
    storage.setItem("kinic.bridge.pending-mint.v2:test", "saved pending mint")
    render(<BridgeProgressProvider><Harness /></BridgeProgressProvider>)
    fireEvent.click(screen.getByRole("button", { name: "Start" }))

    fireEvent.click(screen.getByRole("button", { name: "Base included", hidden: true }))

    expect(screen.queryByRole("status")).not.toBeInTheDocument()
    expect(screen.queryByText("Finality will be reflected in History.")).not.toBeInTheDocument()
    expect(screen.getAllByRole("listitem")).toHaveLength(4)
    expect(screen.getByRole("listitem", { name: "Base transaction complete" })).toBeVisible()
    expect(screen.queryByText("Base finality")).not.toBeInTheDocument()
    expect(screen.queryByText("Complete", { selector: "li *" })).not.toBeInTheDocument()

    await new Promise((resolve) => window.setTimeout(resolve, 0))
    fireEvent.pointerDown(document.querySelector('[data-state="open"][aria-hidden="true"]')!)
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument())
    expect(storage.getItem("kinic.bridge.pending-mint.v2:test")).toBe("saved pending mint")
    fireEvent.click(screen.getByRole("button", { name: "Start another" }))
    expect(screen.getByRole("dialog", { name: "Bridge to IC" })).toBeVisible()
    expect(screen.queryByText("Complete or close the current transfer before starting another one")).not.toBeInTheDocument()
  })

  it("restores an incomplete latest transfer as a minimized global bar", async () => {
    saveLatestBridgeProgress(createBridgeProgress({
      direction: "withdraw",
      phase: "base-withdrawal-finalizing",
      source: "0x0000000000000000000000000000000000000002",
      destination: "aaaaa-aa",
      sendAmount: "2",
      receiveAmount: "1.5",
      sendSymbol: "KINIC",
      receiveSymbol: "TICRC1",
      transactionHash: `0x${"33".repeat(32)}`,
      withdrawal: { owner: "aaaaa-aa" },
    }))

    render(<BridgeProgressProvider><div>Route content</div></BridgeProgressProvider>)

    const bar = await screen.findByRole("button", { name: /Open transfer progress/ })
    expect(bar).toHaveTextContent("Waiting for the Base transaction")
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument()
  })

  it("does not replace an active transfer with a newly started one", () => {
    render(<BridgeProgressProvider><Harness /></BridgeProgressProvider>)
    fireEvent.click(screen.getByRole("button", { name: "Start" }))

    fireEvent.click(screen.getByRole("button", { name: "Start another", hidden: true }))

    expect(screen.getByText("Complete or close the current transfer before starting another one")).toBeVisible()
    expect(screen.getByRole("dialog", { name: "Bridge to Base" })).toBeVisible()
  })

  it("clears the transfer-owned action when progress becomes terminal", () => {
    render(<BridgeProgressProvider><Harness /></BridgeProgressProvider>)
    fireEvent.click(screen.getByRole("button", { name: "Start" }))
    fireEvent.click(screen.getByRole("button", { name: "Register action", hidden: true }))
    expect(screen.getByRole("button", { name: "Retry transfer" })).toBeVisible()

    fireEvent.click(screen.getByRole("button", { name: "Complete", hidden: true }))

    expect(screen.queryByRole("button", { name: "Retry transfer" })).not.toBeInTheDocument()
  })
})
