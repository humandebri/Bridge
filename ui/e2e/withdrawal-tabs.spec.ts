import path from "node:path"
import { realpathSync } from "node:fs"
import { createServer, type ViteDevServer } from "vite"
import react from "@vitejs/plugin-react"
import tailwindcss from "@tailwindcss/vite"
import { expect, test } from "@playwright/test"
let server: ViteDevServer
let fixtureUrl: string
test.beforeAll(async () => {
  const root = path.resolve(import.meta.dirname, "..")
  const environment = path.join(root, "e2e/fixtures/withdrawal-tabs-environment.ts")
  const mocks = [
    "wagmi",
    "@/config/profile",
    "@/features/status/use-status",
    "@/lib/runtime-validation",
    "@/lib/mint-execution",
    "@/lib/transfer-operations",
    "@/lib/base-transaction-observation",
    "@/lib/evm/client",
    "@/lib/ic/bridge",
    "@/lib/ic/withdrawal-notification-client",
    "@/lib/transaction-recovery",
  ]
  server = await createServer({
    root,
    cacheDir: path.join(root, "test-results/vite-withdrawal-tabs"),
    configFile: false,
    plugins: [react(), tailwindcss()],
    resolve: {
      alias: [
        ...mocks.map((find) => ({ find, replacement: environment })),
        { find: "@", replacement: path.join(root, "src") },
      ],
    },
    server: {
      host: "127.0.0.1",
      port: 0,
      fs: { allow: [root, realpathSync(path.resolve(root, "node_modules"))] },
    },
  })
  await server.listen()
  fixtureUrl = `${server.resolvedUrls!.local[0]}e2e/fixtures/withdrawal-tabs.html`
})
test.afterAll(async () => {
  await server?.close()
})
for (const completingTabHasModal of [true, false]) {
  test(`withdrawal follows shared queue removal with completing modal=${completingTabHasModal}`, async ({
    context,
  }) => {
    const original = await context.newPage()
    const completing = await context.newPage()
    const reads = { original: 0, completing: 0 }
    let paid = false
    let failOriginal = false
    let writes = 0
    await context.route("**/withdrawal-test/forbidden-write", async (route) => {
      writes++
      await route.fulfill({ status: 500 })
    })
    await context.route("**/withdrawal-test/read?*", async (route) => {
      const tab = new URL(route.request().url()).searchParams.get("tab") as keyof typeof reads
      reads[tab]++
      if (tab === "original" && failOriginal) {
        await route.fulfill({ status: 503 })
        return
      }
      await route.fulfill({ json: { paid } })
    })
    await original.goto(`${fixtureUrl}?tab=original`)
    await completing.goto(`${fixtureUrl}?tab=completing`)
    await original.getByRole("button", { name: "Track withdrawal" }).click()
    await expect(original.getByTestId("phase")).toHaveText("ledger-payout")
    if (completingTabHasModal)
      await completing.getByRole("button", { name: "Track withdrawal" }).click()
    else await expect(completing.getByTestId("phase")).toHaveText("none")
    await original
      .getByRole("button", { name: "Observe", exact: true, includeHidden: true })
      .evaluate((button) => (button as HTMLButtonElement).click())
    await expect.poll(() => reads.original).toBe(1)
    await expect(
      original.getByRole("button", { name: "Continue payout", exact: true }),
    ).toBeVisible()
    paid = true
    await completing
      .getByRole("button", { name: "Observe", exact: true, includeHidden: true })
      .evaluate((button) => (button as HTMLButtonElement).click())
    await expect
      .poll(() =>
        completing.evaluate(() => {
          const key = Object.keys(localStorage).find((key) =>
            key.startsWith("kinic.bridge.pending-confirmations.v7"),
          )
          return key ? JSON.parse(localStorage.getItem(key)!).entries.length : -1
        }),
      )
      .toBe(0)
    if (completingTabHasModal) await expect(completing.getByTestId("phase")).toHaveText("complete")
    failOriginal = !completingTabHasModal
    await original.bringToFront()
    await original.evaluate(() => document.dispatchEvent(new Event("visibilitychange")))
    if (failOriginal) {
      await expect(original.getByRole("alert")).toContainText(
        "Transfer status could not be refreshed",
      )
      await expect(original.getByTestId("phase")).not.toHaveText("complete")
      failOriginal = false
      await original.evaluate(() => document.dispatchEvent(new Event("visibilitychange")))
    }
    await expect(original.getByTestId("phase")).toHaveText("complete")
    await expect(original.getByTestId("transport-warning")).toHaveText("")
    expect(reads.original).toBeGreaterThanOrEqual(2)
    expect(writes).toBe(0)
  })
}
