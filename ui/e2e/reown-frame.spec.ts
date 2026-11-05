import { createRequire } from "node:module"
import { mkdtempSync, rmSync } from "node:fs"
import { tmpdir } from "node:os"
import path from "node:path"
import { test, expect, type Frame } from "@playwright/test"
import { createServer, type ViteDevServer } from "vite"

const require = createRequire(import.meta.url)
const wc = createRequire(require.resolve("@walletconnect/ethereum-provider"))
const appkit = createRequire(wc.resolve("@reown/appkit/core"))
const wallet = path.dirname(appkit.resolve("@reown/appkit-wallet"))
let server: ViteDevServer
let url: string
let cacheDirectory: string | undefined

declare global {
  interface Window {
    reownTest: {
      parsed: number
      received: number
      once: number
      frame: { iframeIsReady: boolean; iframe: HTMLIFrameElement }
      connect: () => void
    }
  }
}

test.beforeAll(async () => {
  cacheDirectory = mkdtempSync(path.join(tmpdir(), "bridge-vite-reown-frame-"))
  server = await createServer({
    configFile: false,
    cacheDir: cacheDirectory,
    root: path.resolve(import.meta.dirname, ".."),
    server: { host: "127.0.0.1", port: 0 },
    appType: "custom",
    resolve: {
      alias: {
        "@bridge-test/reown-frame": path.resolve(wallet, "../src/W3mFrame.js"),
        "@bridge-test/reown-schema": path.resolve(wallet, "../src/W3mFrameSchema.js"),
      },
    },
    optimizeDeps: {
      noDiscovery: true,
      include: ["@bridge-test/reown-frame", "@bridge-test/reown-schema"],
    },
  })
  server.middlewares.use(async (req, res, next) => {
    if (req.url !== "/") return next()
    const html = `<html><body><h1>Reown message boundary</h1><script type="module">
      import { W3mFrame } from '@bridge-test/reown-frame';
      import { W3mFrameSchema } from '@bridge-test/reown-schema';
      const state = window.reownTest = {parsed:0, received:0, once:0};
      const parse = W3mFrameSchema.frameEvent.safeParse.bind(W3mFrameSchema.frameEvent);
      W3mFrameSchema.frameEvent.safeParse = (...args) => {state.parsed++; return parse(...args)};
      state.connect = () => {
        state.frame?.iframe.remove();
        state.frame = new W3mFrame({projectId:'00000000000000000000000000000000',isAppClient:true});
        state.frame.initFrame();
        state.frame.events.onFrameEvent(() => state.received++);
        state.frame.events.registerFrameEventHandler('request-1', () => state.once++, new AbortController().signal);
      };
      state.connect();
    </script></body></html>`
    res.setHeader("Content-Type", "text/html")
    res.end(await server.transformIndexHtml("/", html))
  })
  await server.listen()
  url = server.resolvedUrls!.local[0]!
})

test.afterAll(async () => {
  try {
    await server?.close()
  } finally {
    if (cacheDirectory) rmSync(cacheDirectory, { recursive: true, force: true })
  }
})

test("Reown validates real message origins and sources before parsing, including after reconnect", async ({
  page,
  context,
}) => {
  page.on("pageerror", (error) => console.error("Reown fixture:", error.message))
  // All remote iframe content is controlled by this local test. No wallet or asset operation occurs.
  await context.route("https://secure.walletconnect.org/**", (route) =>
    route.fulfill({
      contentType: "text/html",
      body: "<html><body>Secure iframe fixture</body></html>",
    }),
  )
  await context.route("https://attacker.example/**", (route) =>
    route.fulfill({
      contentType: "text/html",
      body: "<html><body>Untrusted iframe fixture</body></html>",
    }),
  )
  await page.goto(url)
  await expect.poll(() => page.evaluate(() => Boolean(window.reownTest?.frame.iframe))).toBe(true)
  const trusted = page.frameLocator("#w3m-iframe")
  await expect(trusted.getByText("Secure iframe fixture")).toBeAttached()
  const send = async (frame: Frame, large = false) =>
    frame.evaluate((large) => {
      const data = large
        ? {
            type: "@w3m-frame/GET_USER_SUCCESS",
            payload: { address: "0xabc", chainId: 1, accounts: Array(2000).fill({ address: 1 }) },
          }
        : { id: "request-1", type: "@w3m-frame/READY", payload: {} }
      parent.postMessage(data, "*")
      parent.postMessage({ testBoundaryBarrier: true }, "*")
    }, large)
  // A barrier observes the message queue after all preceding attacker messages were delivered.
  await page.evaluate(() =>
    window.addEventListener("message", (event) => {
      if (event.data?.testBoundaryBarrier)
        document.body.dataset.barrier = String(Number(document.body.dataset.barrier || 0) + 1)
    }),
  )
  await page.evaluate(() => {
    const extra = document.createElement("iframe")
    extra.id = "unrelated"
    extra.src = "https://secure.walletconnect.org/unrelated"
    document.body.append(extra)
  })
  await expect.poll(() => page.frames().some((f) => f.url().endsWith("/unrelated"))).toBe(true)
  await send(
    page.frames().find((f) => f.url().endsWith("/unrelated"))!,
    true,
  )
  await expect(page.locator("body")).toHaveAttribute("data-barrier", "1")
  expect(await page.evaluate(() => window.reownTest.parsed)).toBe(0)

  const popupPromise = page.waitForEvent("popup")
  await page.evaluate(() => window.open(location.origin + "/"))
  const popup = await popupPromise
  await popup.waitForLoadState("domcontentloaded")
  await popup.evaluate(() => {
    opener.postMessage({ id: "request-1", type: "@w3m-frame/READY", payload: {} }, "*")
    opener.postMessage({ testBoundaryBarrier: true }, "*")
  })
  await expect(page.locator("body")).toHaveAttribute("data-barrier", "2")
  expect(await page.evaluate(() => window.reownTest.parsed)).toBe(0)
  await popup.close()

  // The actual target WindowProxy now has an untrusted origin.
  await page.evaluate(() => {
    window.reownTest.frame.iframe.src = "https://attacker.example/"
  })
  await expect
    .poll(() => page.frames().some((f) => f.url() === "https://attacker.example/"))
    .toBe(true)
  await send(
    page.frames().find((f) => f.url() === "https://attacker.example/")!,
    true,
  )
  await expect(page.locator("body")).toHaveAttribute("data-barrier", "3")
  expect(await page.evaluate(() => window.reownTest.parsed)).toBe(0)
  expect(await page.evaluate(() => window.reownTest.received + window.reownTest.once)).toBe(0)

  for (let connection = 1; connection <= 2; connection++) {
    await page.evaluate(() => window.reownTest.connect())
    await expect
      .poll(() =>
        page.frames().some((f) => f.url().startsWith("https://secure.walletconnect.org/sdk")),
      )
      .toBe(true)
    await send(
      page.frames().find((f) => f.url().startsWith("https://secure.walletconnect.org/sdk"))!,
    )
    await expect.poll(() => page.evaluate(() => window.reownTest.received)).toBe(connection)
    expect(await page.evaluate(() => window.reownTest.once)).toBe(connection)
    expect(await page.evaluate(() => window.reownTest.frame.iframeIsReady)).toBe(true)
  }
})
