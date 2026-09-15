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
