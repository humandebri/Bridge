import { expect, test, type APIRequestContext, type Page } from "@playwright/test"

const DEPLOYER = "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266"

test.beforeEach(async ({ page }) => {
  await installAnvilWallet(page)
})

test("deposits through the real ledger, canister, and Anvil contract", async ({ page, request }) => {
  await page.goto("/")
  await expect(page.getByRole("heading", { name: "Bridge KINIC" })).toBeVisible()
  await page.getByRole("button", { name: "Connect Base wallet", exact: true }).click()
  await page.getByRole("button", { name: "Connect Base" }).click()
  await expect(page.getByRole("dialog").getByText("0xf39F…2266", { exact: true })).toBeVisible()
  await page.getByRole("button", { name: "Close confirmation" }).click()
  await expect(page.getByRole("button", { name: /Base wallet connected as 0xf39F/i })).toBeVisible()

  await page.getByRole("button", { name: "Connect IC wallet", exact: true }).click()
  await page.getByRole("button", { name: "Plug" }).click()
  await expect(page.getByRole("dialog").getByText(/^plug · /i)).toBeVisible()
  await page.getByRole("button", { name: "Close confirmation" }).click()
  await expect(page.getByRole("button", { name: /IC wallet connected as /i })).toBeVisible()

  await page.getByLabel("You send").fill("2.00000000")
  await page.getByRole("button", { name: "Bridge to Base" }).click()
  await page.getByRole("checkbox").check()
  await page.getByRole("button", { name: "Confirm and open wallet" }).click()
  await expect(page.getByText(/Deposit 0x[0-9a-f]+… accepted/i)).toBeVisible()

  await postControl(request, "/test/settle", {})
  await expect.poll(async () => BigInt((await controlState(request)).bsnsBalance)).toBeGreaterThan(0n)
  await openHistory(page)
  await expect(page.getByText("Minted", { exact: true })).toBeVisible({ timeout: 30_000 })

  const beforeWithdrawal = await controlState(request)
  await page.getByRole("link", { name: "KINIC Bridge home" }).click()
  await page.getByRole("button", { name: "Reverse bridge direction" }).click()
  await page.getByLabel("You send").fill("1.00000000")
  const withdraw = page.getByRole("button", { name: "Bridge to IC" })
  await expect(withdraw).toBeEnabled()
  await withdraw.click()
  await page.getByRole("button", { name: "Confirm burn" }).click()
  await expect(page.getByText(/Finalized withdrawal was queued for IC settlement/i)).toBeVisible()
  await postControl(request, "/test/settle", {})
  await expect.poll(async () => BigInt((await controlState(request)).ledgerBalance)).toBeGreaterThan(BigInt(beforeWithdrawal.ledgerBalance))
  await openHistory(page)
  await page.getByRole("tab", { name: "Withdrawals" }).click()
  await expect(page.getByText("Released", { exact: true })).toBeVisible({ timeout: 30_000 })
})

async function openHistory(page: Page): Promise<void> {
  const direct = page.getByRole("link", { name: "Open history" })
  if (await direct.isVisible()) {
    await direct.click()
    return
  }
  const history = page.getByRole("link", { name: "History" })
  if (!await history.isVisible()) await page.getByLabel("Open navigation menu").click()
  await history.click()
}

async function postControl(request: APIRequestContext, path: string, data: unknown): Promise<unknown> {
  const response = await request.post(`http://127.0.0.1:43119${path}`, { data })
  expect(response.ok(), await response.text()).toBe(true)
  return response.json()
}

async function controlState(request: APIRequestContext): Promise<{ bsnsBalance: string; ledgerBalance: string }> {
  const response = await request.get("http://127.0.0.1:43119/test/state")
  expect(response.ok(), await response.text()).toBe(true)
  return response.json() as Promise<{ bsnsBalance: string; ledgerBalance: string }>
}

async function installAnvilWallet(page: Page): Promise<void> {
  await page.addInitScript(({ account, rpcUrl }) => {
    type Listener = (...args: unknown[]) => void
    const listeners = new Map<string, Set<Listener>>()
    const provider = {
      isMetaMask: true,
      async request({ method, params = [] }: { method: string; params?: unknown[] }) {
        if (method === "eth_accounts" || method === "eth_requestAccounts") return [account]
        if (method === "eth_chainId") return "0x7a69"
        if (method === "wallet_switchEthereumChain" || method === "wallet_addEthereumChain") return null
        const response = await fetch(rpcUrl, {
          method: "POST",
          headers: { "content-type": "application/json" },
          body: JSON.stringify({ jsonrpc: "2.0", id: 1, method, params }),
        })
        const value = await response.json() as { result?: unknown; error?: { message?: string } }
        if (value.error) throw new Error(value.error.message ?? `Anvil rejected ${method}`)
        if (method === "eth_sendTransaction") {
          await fetch(rpcUrl, {
            method: "POST",
            headers: { "content-type": "application/json" },
            body: JSON.stringify({ jsonrpc: "2.0", id: 2, method: "anvil_mine", params: ["0x40"] }),
          })
        }
        return value.result
      },
      on(event: string, listener: Listener) {
        const current = listeners.get(event) ?? new Set<Listener>()
        current.add(listener); listeners.set(event, current)
        return provider
      },
      removeListener(event: string, listener: Listener) {
        listeners.get(event)?.delete(listener)
        return provider
      },
    }
    Object.defineProperty(window, "ethereum", { configurable: true, value: provider })
  }, { account: DEPLOYER, rpcUrl: "http://127.0.0.1:8545" })
}
