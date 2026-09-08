import { readFileSync } from "node:fs"
import { expect, test } from "@playwright/test"

test("production CSP permits the Base Alchemy RPC and blocks unreviewed origins", async ({
  page,
}) => {
  const headers = readFileSync(new URL("../public/_headers", import.meta.url), "utf8")
  const policy = headers.match(/^\s+Content-Security-Policy: (.+)$/m)?.[1]
  expect(policy).toBeTruthy()
  await page.route("**/csp-probe", (route) =>
    route.fulfill({
      contentType: "text/html",
      headers: { "Content-Security-Policy": policy! },
      body: "<!doctype html><title>CSP probe</title>",
    }),
  )
  let rpcRequests = 0
  await page.route("https://base-mainnet.g.alchemy.com/v2/csp-test-fixture", (route) => {
    rpcRequests += 1
    return route.fulfill({
      contentType: "application/json",
      headers: { "Access-Control-Allow-Origin": "*" },
      body: JSON.stringify({ jsonrpc: "2.0", id: 1, result: "0x2105" }),
    })
  })
  let unreviewedRequests = 0
  await page.route("https://unreviewed-rpc.invalid/**", (route) => {
    unreviewedRequests += 1
    return route.fulfill({ body: "unexpected", headers: { "Access-Control-Allow-Origin": "*" } })
  })
  await page.goto("/csp-probe")
  const chainId = await page.evaluate(async () => {
    const response = await fetch("https://base-mainnet.g.alchemy.com/v2/csp-test-fixture", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ jsonrpc: "2.0", id: 1, method: "eth_chainId", params: [] }),
    })
    return (await response.json()).result
  })
  expect(chainId).toBe("0x2105")
  expect(rpcRequests).toBe(1)
  const blocked = await page.evaluate(async () => {
    try {
      await fetch("https://unreviewed-rpc.invalid/probe")
      return false
    } catch {
      return true
    }
  })
  expect(blocked).toBe(true)
  expect(unreviewedRequests).toBe(0)
})
