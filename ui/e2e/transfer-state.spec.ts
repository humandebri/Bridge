import path from "node:path"
import { createServer, type ViteDevServer } from "vite"
import react from "@vitejs/plugin-react"
import tailwindcss from "@tailwindcss/vite"
import { expect, test, type Page } from "@playwright/test"

let server: ViteDevServer
let fixtureUrl: string
test.beforeAll(async () => {
  const root = path.resolve(import.meta.dirname, "..")
  server = await createServer({
    root,
    configFile: false,
    plugins: [react(), tailwindcss()],
    resolve: { alias: { "@": path.join(root, "src") } },
    server: { host: "127.0.0.1", port: 0 },
  })
  await server.listen()
  fixtureUrl = `${server.resolvedUrls!.local[0]}e2e/fixtures/transfer-state.html`
})
test.afterAll(async () => {
  await server?.close()
})

const signal = async (page: Page, name: string) => {
  await page
    .getByRole("button", { name, exact: true, includeHidden: true })
    .evaluate((button) => (button as HTMLButtonElement).click())
}

test("transfer modal and history recover together without terminal regression", async ({
  page,
}) => {
  await page.goto(fixtureUrl)
  await page.getByRole("button", { name: "Start deposit" }).click()
  await signal(page, "Fail read")
  await expect(page.getByRole("alert")).toContainText("Status unavailable")
  await expect(page.getByTestId("history-title")).toHaveText("Status unavailable")
  await expect(page.getByRole("dialog").locator(".animate-spin")).toHaveCount(0)
  await page.getByRole("button", { name: "Minimize", exact: true }).click()
  await expect(page.getByRole("button", { name: /Open transfer progress/ })).toBeVisible()
  await page.getByRole("button", { name: /Open transfer progress/ }).click()
  await signal(page, "Recover")
  await expect(page.getByTestId("history-title")).toHaveText("Mint included")
  await expect(page.getByRole("alert")).toHaveCount(0)
  await signal(page, "Finalize")
  await signal(page, "Late inclusion")
  await expect(page.getByTestId("history-title")).toHaveText("Mint complete")
  await expect(page.getByRole("status")).toContainText("Mint complete")
})

test("refund action runs the mocked operation inside the modal", async ({ page }) => {
  let requests = 0
  await page.route("**/transfer-test/refund", async (route) => {
    requests++
    await route.fulfill({ status: 200, body: "{}" })
  })
  await page.goto(fixtureUrl)
  await page.getByRole("button", { name: "Start deposit" }).click()
  await signal(page, "Refund ready")
  await expect(page.getByRole("alert")).toContainText("This transfer needs attention")
  await page.getByRole("button", { name: "Check refund", exact: true }).click()
  await expect(page.getByRole("status")).toContainText("Refund complete")
  await expect(page.getByTestId("history-title")).toHaveText("Refund complete")
  expect(requests).toBe(1)
  expect(page.url()).toContain("transfer-state.html")
})

test("withdrawal waits for IC payment and survives a late notification", async ({ page }) => {
  await page.goto(fixtureUrl)
  await page.getByRole("button", { name: "Start withdrawal" }).click()
  await signal(page, "Fail read")
  await expect(page.getByTestId("history-title")).toHaveText("Status unavailable")
  await expect(page.getByRole("dialog").locator(".animate-spin")).toHaveCount(0)
  await signal(page, "Notify IC")
  await expect(page.getByTestId("history-title")).toHaveText("Recording withdrawal on IC")
  await signal(page, "Payout pending")
  await expect(page.getByTestId("history-title")).toHaveText("Payout pending")
  await signal(page, "Paid")
  await signal(page, "Notify IC")
  await expect(page.getByTestId("history-title")).toHaveText("Withdrawal complete")
  await expect(page.getByRole("status")).toContainText("Withdrawal complete")
})

for (const width of [1280, 390]) {
  test(`included mint can Finish and history stays compact at ${width}px`, async ({ page }) => {
    await page.setViewportSize({ width, height: 900 })
    await page.goto(fixtureUrl)
    const row = page.getByRole("region", { name: "Included mint history" })
    await expect(row.getByRole("button", { name: "Copy mint diagnostics" })).toHaveCount(0)
    await expect(row.getByRole("link")).toHaveCount(2)
    for (const link of await row.getByRole("link").all()) {
      expect(await link.evaluate((el) => getComputedStyle(el).color)).toBe(
        "oklch(0.546 0.245 262.881)",
      )
    }
    expect(
      await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth),
    ).toBe(true)
    await page.getByRole("button", { name: "Start deposit" }).click()
    await expect(page.getByRole("button", { name: "Finish", exact: true })).toHaveCount(0)
    await signal(page, "Recover")
    await expect(page.getByRole("status")).toContainText("Mint included")
    await expect(page.getByRole("dialog").locator(".animate-spin")).toHaveCount(0)
    await page.getByRole("button", { name: "Finish", exact: true }).click()
    await expect(page.getByRole("dialog")).toHaveCount(0)
    await expect(page.getByRole("button", { name: /Open transfer progress/ })).toHaveCount(0)
    await page.reload()
    await expect(page.getByRole("dialog")).toHaveCount(0)
    await expect(page.getByRole("button", { name: /Open transfer progress/ })).toHaveCount(0)
  })
}

test("completed mint history has no redundant recording confirmation", async ({ page }) => {
  await page.goto(fixtureUrl)
  await page.getByRole("button", { name: "Complete history mint" }).click()
  const row = page.getByRole("region", { name: "Included mint history" })
  await expect(row.getByText("Mint complete", { exact: true })).toBeVisible()
  await expect(row.getByText("Recorded on IC")).toHaveCount(0)
  await expect(row.getByText(/Waiting for IC recording/)).toHaveCount(0)
})
