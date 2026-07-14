import { expect, test } from "@playwright/test"
import type { Page, TestInfo } from "@playwright/test"

test("bridge defaults to IC to Base and stays fail-closed", async ({ page }, testInfo) => {
  await page.goto("/")
  await expect(page.getByRole("heading", { name: "Bridge KINIC" })).toBeVisible()
  await expect(page.getByText("Internet Computer")).toBeVisible()
  await expect(page.getByText("Transfers are locked during preflight.")).toBeVisible()
  await expect(page.getByRole("button", { name: "Bridge to Base" })).toBeDisabled()
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
  await capture(page, testInfo, "bridge-deposit")
})

test("direction switch is URL-backed and exposes withdrawal protection", async ({ page }, testInfo) => {
  await page.goto("/?direction=deposit")
  await page.getByRole("button", { name: "Reverse bridge direction" }).click()
  await expect(page).toHaveURL(/direction=withdraw/)
  await expect(page.getByRole("button", { name: "Bridge to IC" })).toBeDisabled()
  await expect(page.getByRole("button", { name: "Minimum received" })).toBeVisible()
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
  await expect(page.getByRole("tab", { name: "Deposits" })).toHaveAttribute("aria-selected", "true")
  await capture(page, testInfo, "history")
  await page.goto("/status")
  await expect(page.getByRole("heading", { name: "Bridge status" })).toBeVisible()
  await expect(page.getByText("Transfers are locked.")).toBeVisible()
  await capture(page, testInfo, "status")
})

async function capture(page: Page, testInfo: TestInfo, name: string) {
  const path = testInfo.outputPath(`${name}.png`)
  await page.screenshot({ fullPage: true, path })
  await testInfo.attach(`${name}-${testInfo.project.name}`, { path, contentType: "image/png" })
}
