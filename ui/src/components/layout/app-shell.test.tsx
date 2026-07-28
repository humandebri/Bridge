import { cleanup, render, screen, within } from "@testing-library/react"
import { afterEach, describe, expect, it, vi } from "vitest"

vi.mock("@tanstack/react-router", () => ({
  Link: ({ children }: { children: React.ReactNode }) => <a>{children}</a>,
  Outlet: () => null,
}))
vi.mock("@/features/wallet/wallet-controls", () => ({
  WalletCenter: () => null,
  WalletDialogProvider: ({ children }: { children: React.ReactNode }) => <>{children}</>,
}))
vi.mock("@/features/bridge/settlement-confirmation-coordinator", () => ({ SettlementConfirmationCoordinator: () => null }))
vi.mock("@/features/risk/risk-acknowledgement", () => ({ RiskAcknowledgementDialog: () => null }))

import { AppShell } from "./app-shell"

afterEach(cleanup)

describe("AppShell test deployment banner", () => {
  it("always identifies a test-only deployment", () => {
    render(<AppShell />)
    expect(screen.getByRole("status", { name: "Test deployment" })).toHaveTextContent("IC MAINNET × BASE SEPOLIA TEST")
  })
})

describe("AppShell footer", () => {
  it("uses the available viewport height without duplicate bottom spacing", () => {
    render(<AppShell />)

    const main = screen.getByRole("main")
    expect(main.parentElement).toHaveClass("flex", "min-h-screen", "flex-col")
    expect(main).toHaveClass("w-full", "flex-1")
    expect(main).not.toHaveClass("pb-20")
    expect(screen.getByRole("banner")).toHaveClass("w-full")
    expect(screen.getByRole("contentinfo")).toHaveClass("w-full")
  })

  it("links to the official KINIC resources in a new tab", () => {
    render(<AppShell />)

    expect(screen.getByRole("link", { name: "Wiki" })).toHaveAttribute("href", "https://wiki.kinic.xyz/")
    const xLink = screen.getByRole("link", { name: "KINIC on X" })
    expect(xLink).toHaveAttribute("href", "https://x.com/kinic_app")
    expect(xLink.querySelector("svg")).toHaveAttribute("fill", "#000000")
    const openChatLink = screen.getByRole("link", { name: "KINIC OpenChat community" })
    expect(openChatLink).toHaveAttribute(
      "href",
      "https://oc.app/community/rqdzm-qaaaa-aaaar-ar3na-cai/channel/3004043573",
    )
    const openChatLogo = openChatLink.querySelector("img")
    expect(openChatLogo).toHaveAttribute("src", expect.stringContaining("data:image/svg+xml"))
    expect(openChatLogo?.getAttribute("src")).toContain("%23FBB03B")
    expect(openChatLogo?.getAttribute("src")).toContain("%23ED1E79")

    for (const link of screen.getAllByRole("link").filter((element) => element.closest("footer"))) {
      expect(link).toHaveAttribute("target", "_blank")
      expect(link).toHaveAttribute("rel", "noopener noreferrer")
    }
  })

  it("removes the generic bridge and wallet reminders", () => {
    render(<AppShell />)

    expect(within(screen.getByRole("contentinfo")).queryByText("KINIC Bridge")).not.toBeInTheDocument()
    expect(screen.queryByText("KINIC moves 1:1 across IC and Base.")).not.toBeInTheDocument()
    expect(screen.queryByText("Verify every account, amount, and wallet prompt.")).not.toBeInTheDocument()
  })
})
