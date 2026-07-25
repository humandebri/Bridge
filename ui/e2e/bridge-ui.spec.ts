import { expect, test } from "@playwright/test"
import type { Page, TestInfo } from "@playwright/test"

const RISK_ACKNOWLEDGEMENT_KEY = "kinic.bridge.risk-acknowledgement.v1:84532::"

test.beforeEach(async ({ page }, testInfo) => {
  if (testInfo.title === "requires risk acknowledgement on first use") return
  await page.addInitScript((key) => window.localStorage.setItem(key, "acknowledged"), RISK_ACKNOWLEDGEMENT_KEY)
})

test("requires risk acknowledgement on first use", async ({ page }, testInfo) => {
  await page.goto("/")
  await expect(page.getByRole("heading", { name: "Unaudited bridge" })).toBeVisible()
  await expect(page.getByText("This bridge has not been audited.")).toBeVisible()
  await expect(page.getByRole("button", { name: "Close confirmation" })).toHaveCount(0)
  await expect(page.getByRole("button", { name: "Acknowledge and continue" })).toBeDisabled()
  await capture(page, testInfo, "risk-acknowledgement")

  await page.keyboard.press("Escape")
  await expect(page.getByRole("dialog")).toBeVisible()
  await page.mouse.click(5, 5)
  await expect(page.getByRole("dialog")).toBeVisible()
  await page.getByRole("checkbox", { name: "Acknowledge unaudited bridge risk" }).check()
  await page.getByRole("button", { name: "Acknowledge and continue" }).click()
  await expect(page.getByRole("dialog")).toBeHidden()
  await expect.poll(() => page.evaluate((key) => window.localStorage.getItem(key), RISK_ACKNOWLEDGEMENT_KEY)).toBe("acknowledged")

  await page.reload()
  await expect(page.getByRole("heading", { name: "Unaudited bridge" })).toBeHidden()
})

test("bridge defaults to IC to Base and reports incomplete configuration", async ({ page }, testInfo) => {
  await page.goto("/")
  await expect(page.getByRole("heading", { name: "Bridge KINIC" })).toBeVisible()
  const homeLink = page.getByRole("link", { name: "KINIC Bridge home" })
  await expect(homeLink).toContainText("KINIC Bridge")
  await expect(homeLink.locator("img")).toHaveAttribute("src", /blue_kinic/)
  await expect(page.locator('link[rel="icon"]')).toHaveAttribute("href", /blue_kinic/)
  await expect(page.locator('link[rel="apple-touch-icon"]')).toHaveAttribute("href", /blue_kinic/)
  await expect(page.getByText("IC ↔ Base", { exact: true })).toHaveCount(0)
  await expect(page.getByText("Internet Computer")).toBeVisible()
  await expect(page.getByRole("button", { name: "From Internet Computer Connect IC wallet", exact: true }).locator('[data-network-logo="ic"]')).toBeVisible()
  await expect(page.getByRole("button", { name: "To Base Connect EVM wallet", exact: true }).locator('[data-network-logo="base"]')).toBeVisible()
  await expect(page.getByText("Refresh before continuing.")).toBeHidden()
  await expect(page.getByText("Bridge is temporarily unavailable. Try Refresh.")).toBeVisible()
  await expect(page.getByRole("button", { name: "Bridge to Base" })).toBeDisabled()
  await expect(page.getByLabel("You send")).toHaveAttribute("aria-invalid", "true")
  await expect(page.getByLabel("You send")).toHaveAttribute("aria-describedby", "bridge-amount-feedback")
  await expect(page.getByRole("button", { name: "Reverse bridge direction" })).toHaveCSS("width", "32px")
  await expect(page.getByRole("button", { name: "Reverse bridge direction" })).toHaveCSS("height", "32px")
  if ((page.viewportSize()?.width ?? 0) >= 768) {
    await expect(page.getByRole("link", { name: "Open history" })).toBeVisible()
    await expect(page.getByRole("link", { name: "Open status" })).toBeVisible()
    await expect(page.getByLabel("Open navigation menu")).toBeHidden()
  } else {
    await expect(page.getByRole("link", { name: "Open history" })).toBeHidden()
    await expect(page.getByRole("link", { name: "Open status" })).toBeHidden()
    await expect(page.getByLabel("Open navigation menu")).toBeVisible()
  }
  if ((page.viewportSize()?.width ?? 0) >= 1024) {
    const intro = await page.getByTestId("bridge-intro").boundingBox()
    const panel = await page.getByTestId("bridge-panel").boundingBox()
    expect(intro).not.toBeNull()
    expect(panel).not.toBeNull()
    expect(panel!.x).toBeGreaterThan(intro!.x)
    expect(panel!.width).toBeLessThanOrEqual(621)
  }
  await expect(page.locator(".kinic-rail i")).toHaveCount(4)
  await capture(page, testInfo, "bridge-deposit")
})

test("direction switch is URL-backed and exposes withdrawal protection", async ({ page }, testInfo) => {
  await page.goto("/?direction=deposit")
  await expect(page.locator(".kinic-rail")).not.toHaveClass(/is-withdraw/)
  await page.getByRole("button", { name: "Reverse bridge direction" }).click()
  await expect(page).toHaveURL(/direction=withdraw/)
  await expect(page.locator(".kinic-rail")).toHaveClass(/is-withdraw/)
  await expect(page.getByRole("button", { name: "From Base Connect EVM wallet", exact: true }).locator('[data-network-logo="base"]')).toBeVisible()
  await expect(page.getByRole("button", { name: "To Internet Computer Connect IC wallet", exact: true }).locator('[data-network-logo="ic"]')).toBeVisible()
  await expect(page.getByRole("button", { name: "Bridge to IC" })).toBeDisabled()
  await expect(page.getByText("Estimated receive", { exact: true })).toBeVisible()
  await expect(page.getByText("Base refund is not available after burn.", { exact: true })).toBeVisible()
  await capture(page, testInfo, "bridge-withdraw")
})

test("IC and EVM wallet controls are separate", async ({ page }, testInfo) => {
  await page.addInitScript(() => {
    const provider = { request: () => Promise.resolve([]) }
    const wallets = [
      { uuid: "350670db-19fa-4704-a166-e52e178b59d2", name: "MetaMask", icon: "data:image/svg+xml,<svg xmlns='http://www.w3.org/2000/svg' width='96' height='96'><rect width='96' height='96' fill='orange'/></svg>", rdns: "io.metamask" },
      { uuid: "7c867694-1697-4d85-bf2e-70d6c97a08b5", name: "Rabby Wallet", icon: "data:image/svg+xml,<svg xmlns='http://www.w3.org/2000/svg' width='96' height='96'><rect width='96' height='96' fill='blue'/></svg>", rdns: "io.rabby" },
      { uuid: "27415cfc-f282-4f8c-8f6e-ec79df93be90", name: "Plug", icon: "data:image/svg+xml,<svg xmlns='http://www.w3.org/2000/svg' width='96' height='96'><rect width='96' height='96' fill='pink'/></svg>", rdns: "com.plugwallet" },
    ]
    const announce = () => wallets.forEach((info) => window.dispatchEvent(new CustomEvent("eip6963:announceProvider", { detail: { info, provider } })))
    window.addEventListener("eip6963:requestProvider", announce)
  })
  await page.goto("/")
  await expect(page.getByRole("button", { name: "Connect IC wallet", exact: true })).toBeVisible()
  await expect(page.getByRole("button", { name: "Connect EVM wallet", exact: true })).toBeVisible()
  await page.getByRole("button", { name: "Connect IC wallet", exact: true }).click()
  await expect(page.getByRole("heading", { name: "IC wallet" })).toBeVisible()
  await expect(page.getByRole("dialog").locator('[data-dialog-network-logo="ic"]')).toBeVisible()
  await expect(page.getByRole("button", { name: "Connect OISY Wallet" })).toBeVisible()
  await expect(page.getByRole("img", { name: "OISY Wallet logo" })).toBeVisible()
  await expect(page.getByRole("button", { name: "Connect Plug" })).toBeVisible()
  await expect(page.getByRole("img", { name: "Plug logo" })).toBeVisible()
  await expect(page.getByRole("button", { name: "Connect MetaMask" })).toBeHidden()
  await capture(page, testInfo, "wallet-ic-options")
  await page.getByRole("button", { name: "Close confirmation" }).click()
  await page.getByRole("button", { name: "Connect EVM wallet", exact: true }).click()
  await expect(page.getByRole("heading", { name: "EVM wallet" })).toBeVisible()
  await expect(page.getByRole("dialog").locator('[data-dialog-network-logo="base"]')).toBeVisible()
  await expect(page.getByRole("button", { name: "Connect Coinbase Wallet" })).toBeVisible()
  await expect(page.getByRole("img", { name: "Coinbase Wallet logo" })).toBeVisible()
  await expect(page.getByRole("button", { name: "Connect MetaMask" })).toBeVisible()
  await expect(page.getByRole("button", { name: "Connect Rabby Wallet" })).toBeVisible()
  await expect(page.getByRole("button", { name: "Connect Plug" })).toBeHidden()
  await expect(page.getByRole("button", { name: "Connect Browser wallet" })).toBeHidden()
  await expect(page.getByRole("button", { name: "Connect OISY Wallet" })).toBeHidden()
  await capture(page, testInfo, "wallet-base-options")
})

test("history and status are separate low-density surfaces", async ({ page }, testInfo) => {
  await page.goto("/history")
  await expect(page.getByRole("heading", { name: "Bridge history" })).toBeVisible()
  await expect(page.getByRole("alert")).toContainText("Actions unavailable")
  await expect(page.getByRole("alert")).toContainText("Refresh before continuing")
  await expect(page.getByRole("tab")).toHaveCount(0)
  await expect(page.getByRole("button", { name: "All", exact: true })).toHaveAttribute("aria-pressed", "true")
  await page.getByRole("button", { name: "To IC", exact: true }).click()
  await expect(page.getByRole("button", { name: "To IC", exact: true })).toHaveAttribute("aria-pressed", "true")
  await expect(page).not.toHaveURL(/tab=/)
  await capture(page, testInfo, "history")
  await page.goto("/status")
  await expect(page.getByRole("heading", { name: "Bridge status" })).toBeVisible()
  await expect(page.getByText("Current availability across Internet Computer and Base.")).toBeVisible()
  await expect(page.getByText("Bridge checks have not passed.")).toBeHidden()
  await expect(page.getByText("Safe is not finality.")).toBeHidden()
  await expect(page.getByText("Availability", { exact: true })).toBeVisible()
  await expect(page.getByText("Current terms", { exact: true })).toBeVisible()
  await capture(page, testInfo, "status")
})

async function capture(page: Page, testInfo: TestInfo, name: string) {
  const path = testInfo.outputPath(`${name}.png`)
  await page.screenshot({ fullPage: true, path })
  await testInfo.attach(`${name}-${testInfo.project.name}`, { path, contentType: "image/png" })
}
