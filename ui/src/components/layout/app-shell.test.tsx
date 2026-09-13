import { cleanup, render, screen } from "@testing-library/react"
import { afterEach, describe, expect, it, vi } from "vitest"

vi.mock("@/features/bridge/mint-confirmation-coordinator", () => ({
  MintConfirmationCoordinator: () => null,
}))
vi.mock("@tanstack/react-router", () => ({
  Link: ({ children }: { children: React.ReactNode }) => <a>{children}</a>,
  Outlet: () => null,
}))
vi.mock("@/features/wallet/wallet-controls", () => ({
  WalletCenter: () => null,
  WalletDialogProvider: ({ children }: { children: React.ReactNode }) => <>{children}</>,
}))
vi.mock("@/features/bridge/settlement-confirmation-coordinator", () => ({
  SettlementConfirmationCoordinator: () => null,
}))
vi.mock("@/features/bridge/deposit-progress-coordinator", () => ({
  DepositProgressCoordinator: () => null,
}))
vi.mock("@/features/risk/risk-acknowledgement", () => ({ RiskAcknowledgementDialog: () => null }))
vi.mock("@/config/profile", () => ({
  deploymentProfile: {
    testOnly: true,
    environmentMode: "short-delay-test-only",
  },
}))

import { AppShell } from "./app-shell"

afterEach(cleanup)

describe("AppShell test deployment banner", () => {
  it("always identifies a test-only deployment", () => {
    render(<AppShell />)
    expect(screen.getByRole("status", { name: "Test deployment" })).toHaveTextContent(
      "IC MAINNET × BASE SEPOLIA TEST",
    )
    expect(screen.getByRole("status", { name: "Test deployment" })).toHaveTextContent(
      "5-MINUTE TIMELOCK",
    )
  })
})

describe("AppShell footer", () => {
  it("links to the official KINIC resources in a new tab", () => {
    render(<AppShell />)

    expect(screen.getByRole("link", { name: "Wiki" })).toHaveAttribute(
      "href",
      "https://wiki.kinic.xyz/",
    )
    const xLink = screen.getByRole("link", { name: "KINIC on X" })
    expect(xLink).toHaveAttribute("href", "https://x.com/kinic_app")
    const openChatLink = screen.getByRole("link", { name: "KINIC OpenChat community" })
    expect(openChatLink).toHaveAttribute(
      "href",
      "https://oc.app/community/rqdzm-qaaaa-aaaar-ar3na-cai/channel/3004043573",
    )
    for (const link of screen.getAllByRole("link").filter((element) => element.closest("footer"))) {
      expect(link).toHaveAttribute("target", "_blank")
      expect(link).toHaveAttribute("rel", "noopener noreferrer")
    }
  })
})
