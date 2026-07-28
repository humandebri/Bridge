import { fireEvent, render, screen } from "@testing-library/react"
import { useState } from "react"
import { describe, expect, it } from "vitest"
import { Dialog, DialogContent, DialogDescription, DialogTitle } from "./dialog"

function DismissibleDialog() {
  const [open, setOpen] = useState(true)
  return <Dialog open={open} onOpenChange={setOpen}><DialogContent>
    <DialogTitle>Existing dialog</DialogTitle>
    <DialogDescription>Existing dialogs remain dismissible.</DialogDescription>
  </DialogContent></Dialog>
}

describe("DialogContent", () => {
  it("remains dismissible by default", () => {
    render(<DismissibleDialog />)
    fireEvent.click(screen.getByRole("button", { name: "Close confirmation" }))
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument()
  })
})
