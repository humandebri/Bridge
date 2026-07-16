import { expect, test } from "@playwright/test"
import type { Page, TestInfo } from "@playwright/test"

test("bridge defaults to IC to Base and reports incomplete configuration", async ({ page }, testInfo) => {
  await page.goto("/")
  await expect(page.getByRole("heading", { name: "Bridge KINIC" })).toBeVisible()
  await expect(page.getByRole("link", { name: "KINIC Bridge home" }).locator("img")).toHaveAttribute("src", /blue_kinic/)
  await expect(page.locator('link[rel="icon"]')).toHaveAttribute("href", /blue_kinic/)
  await expect(page.locator('link[rel="apple-touch-icon"]')).toHaveAttribute("href", /blue_kinic/)
  await expect(page.getByText("Internet Computer")).toBeVisible()
  await expect(page.getByText("Refresh before continuing.")).toBeVisible()
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
  await expect(page.getByRole("button", { name: "Bridge to IC" })).toBeDisabled()
  await expect(page.getByText("Estimated receive", { exact: true })).toBeVisible()
  await expect(page.getByText("Base refund is not available after burn.", { exact: true })).toBeVisible()
  await capture(page, testInfo, "bridge-withdraw")
})

test("IC and Base wallet controls are separate", async ({ page }) => {
  await page.goto("/")
  await expect(page.getByRole("button", { name: "Connect IC wallet", exact: true })).toBeVisible()
  await expect(page.getByRole("button", { name: "Connect Base wallet", exact: true })).toBeVisible()
  await page.getByRole("button", { name: "Connect IC wallet", exact: true }).click()
  await expect(page.getByRole("heading", { name: "IC wallet" })).toBeVisible()
  await expect(page.getByRole("button", { name: "OISY" })).toBeVisible()
  await expect(page.getByRole("button", { name: "Plug" })).toBeVisible()
  await expect(page.getByRole("button", { name: "Connect Base" })).toBeHidden()
  await page.getByRole("button", { name: "Close confirmation" }).click()
  await page.getByRole("button", { name: "Connect Base wallet", exact: true }).click()
  await expect(page.getByRole("heading", { name: "Base wallet" })).toBeVisible()
  await expect(page.getByRole("button", { name: "Connect Base" })).toBeVisible()
  await expect(page.getByRole("button", { name: "OISY" })).toBeHidden()
})

test("history and status are separate low-density surfaces", async ({ page }, testInfo) => {
  await page.goto("/history")
  await expect(page.getByRole("heading", { name: "Bridge history" })).toBeVisible()
  await expect(page.getByRole("alert")).toContainText("Actions unavailable")
  await expect(page.getByRole("alert")).toContainText("Refresh before continuing")
  await expect(page.getByRole("tab", { name: "Deposits" })).toHaveAttribute("aria-selected", "true")
  await page.getByRole("tab", { name: "Deposits" }).press("ArrowRight")
  await expect(page).toHaveURL(/tab=withdraw/)
  await expect(page.getByRole("tab", { name: "Withdrawals" })).toBeFocused()
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
