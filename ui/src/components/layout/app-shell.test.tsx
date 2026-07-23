import { render, screen } from "@testing-library/react"
import { describe, expect, it, vi } from "vitest"

vi.mock("@tanstack/react-router", () => ({
  Link: ({ children }: { children: React.ReactNode }) => <a>{children}</a>,
  Outlet: () => null,
}))
vi.mock("@/features/wallet/wallet-controls", () => ({
  WalletCenter: () => null,
  WalletDialogProvider: ({ children }: { children: React.ReactNode }) => <>{children}</>,
}))
vi.mock("@/features/bridge/settlement-confirmation-coordinator", () => ({ SettlementConfirmationCoordinator: () => null }))

import { AppShell } from "./app-shell"

describe("AppShell test deployment banner", () => {
  it("always identifies a test-only deployment", () => {
    render(<AppShell />)
    expect(screen.getByRole("status", { name: "Test deployment" })).toHaveTextContent("IC MAINNET × BASE SEPOLIA TEST")
  })
})
