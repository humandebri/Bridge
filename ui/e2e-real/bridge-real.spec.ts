import { expect, test, type APIRequestContext, type Page } from "@playwright/test"

const DEPLOYER = "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266"

test.beforeEach(async ({ page }) => {
  await installAnvilWallet(page)
})

test("deposits through the real ledger, canister, and Anvil contract", async ({ page, request }) => {
  await expect.poll(async () => {
    const state = await controlState(request)
    return { bound: state.indexLedgerId === state.ledgerId, synced: state.indexBalance === state.ledgerBalance }
  }, { timeout: 30_000 }).toEqual({ bound: true, synced: true })
  const initial = await controlState(request)
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

  await page.getByRole("button", { name: "Refresh bridge data" }).click()
  await expect(page.getByText("TICRC1", { exact: true }).first()).toBeVisible()
  await page.getByLabel("You send").fill("2.00000000")
  await expect(page.getByText("1.99 KINIC", { exact: true })).toBeVisible()
  await postControl(request, "/test/fail-next-deposit-response", {})
  const depositButton = page.getByRole("button", { name: "Bridge to Base" })
  await expect.poll(async () => {
    if (!await depositButton.isEnabled()) return await page.getByText(/^Next:/).textContent()
    try {
      await depositButton.click({ timeout: 500 })
      return "opened"
    } catch {
      return await page.getByText(/^Next:/).textContent()
    }
  }, { timeout: 30_000 }).toBe("opened")
  await page.getByRole("checkbox").check()
  await page.getByRole("button", { name: "Confirm and open wallet" }).click()
  await expect(page.getByText("Injected response loss after deposit acceptance", { exact: false })).toBeVisible()
  await expect(page.getByText("Deposit response unresolved", { exact: true })).toBeVisible()
  await expect(page.getByRole("button", { name: "Retry same deposit" })).toBeVisible()
  expect(await controlState(request)).toMatchObject({ knownDepositCount: 1, depositSequences: ["0"], nextDepositSequence: "1" })

  await page.getByRole("button", { name: "Refresh bridge data" }).click()
  await expect(page.getByRole("button", { name: "Retry same deposit" })).toBeVisible()
  await page.getByRole("button", { name: "Retry same deposit" }).click()
  await page.getByRole("button", { name: "Confirm and open wallet" }).click()
  await expect(page.getByText(/Deposit 0x[0-9a-f]+… accepted/i)).toBeVisible()
  await expect(page.getByText("Deposit response unresolved", { exact: true })).toHaveCount(0)
  await expect(page.getByRole("button", { name: "Bridge to Base" })).toBeVisible()
  const afterRecovery = await controlState(request)
  expect(afterRecovery).toMatchObject({ knownDepositCount: 1, depositSequences: ["0", "0"], nextDepositSequence: "1" })
  expect(BigInt(initial.ledgerBalance) - BigInt(afterRecovery.ledgerBalance)).toBe(200_020_000n)

  await page.getByLabel("You send").fill("1.00000000")
  await page.getByRole("button", { name: "Bridge to Base" }).click()
  await page.getByRole("checkbox").check()
  await page.getByRole("button", { name: "Confirm and open wallet" }).click()
  await expect(page.getByText(/Deposit 0x[0-9a-f]+… accepted/i)).toBeVisible()
  await expect.poll(async () => {
    const state = await controlState(request)
    return {
      knownDepositCount: state.knownDepositCount,
      depositSequences: state.depositSequences,
      nextDepositSequence: state.nextDepositSequence,
    }
  }).toEqual({ knownDepositCount: 2, depositSequences: ["0", "0", "1"], nextDepositSequence: "2" })

  await postControl(request, "/test/relay", {})
  await expect.poll(async () => BigInt((await controlState(request)).bsnsBalance)).toBe(298_000_000n)
  await expect.poll(async () => {
    const state = await controlState(request)
    return state.indexBalance === state.ledgerBalance
  }, { timeout: 30_000 }).toBe(true)
  const afterDeposit = await controlState(request)
  expect(BigInt(initial.ledgerBalance) - BigInt(afterDeposit.ledgerBalance)).toBe(300_040_000n)
  expect(BigInt(afterDeposit.indexBlocksSynced)).toBeGreaterThanOrEqual(BigInt(initial.indexBlocksSynced) + 4n)
  await openHistory(page)
  await page.getByRole("button", { name: "Refresh", exact: true }).click()
  await expect(page.getByText("MintPending", { exact: true }).first()).toBeVisible({ timeout: 30_000 })
  await expect(page.getByText(/Confirming automatically/).first()).toBeVisible()
  await expect(page.getByRole("button", { name: "Retry settlement" })).toHaveCount(0)
  // Cross the two-minute boundary with one minute of margin for PocketIC timer rounding.
  const firstAdvance = await postControl(request, "/test/advance-confirmation", { minutes: 3 }) as { time: number }
  await page.clock.setFixedTime(firstAdvance.time)
  await page.getByRole("button", { name: "Refresh", exact: true }).click()
  await expect(page.getByText("Minted", { exact: true }).first()).toBeVisible({ timeout: 30_000 })

  const beforeWithdrawal = await controlState(request)
  await page.getByRole("link", { name: "KINIC Bridge home" }).click()
  await page.getByRole("button", { name: "Reverse bridge direction" }).click()
  await page.getByRole("button", { name: "Refresh bridge data" }).click()
  await expect(page.getByText("KINIC", { exact: true }).first()).toBeVisible()
  await page.getByLabel("You send").fill("1.00000000")
  await expect(page.getByText("0.9899 TICRC1", { exact: true })).toBeVisible()
  const withdraw = page.getByRole("button", { name: "Bridge to IC" })
  await postControl(request, "/test/fail-next-notification", {})
  await expect.poll(async () => {
    if (!await withdraw.isEnabled()) return await page.getByText(/^Next:/).textContent()
    try {
      await withdraw.click({ timeout: 500 })
      return "opened"
    } catch {
      return await page.getByText(/^Next:/).textContent()
    }
  }, { timeout: 30_000 }).toBe("opened")
  await page.getByRole("button", { name: "Confirm burn" }).click()
  await expect(page.getByText(/Withdrawal submitted:/)).toBeVisible()
  await expect.poll(async () => (await controlState(request)).notifyCalls).toBe(beforeWithdrawal.notifyCalls + 1)
  await expect(page.getByText(/automatic notification did not finish/i)).toBeVisible()
  expect((await controlState(request)).bsnsAllowance).toBe("0")
  await openHistory(page)
  await page.getByRole("tab", { name: "Withdrawals" }).click()
  await page.getByRole("button", { name: "Refresh", exact: true }).click()
  await expect(page.getByRole("button", { name: "Check and notify" })).toBeVisible()
  await page.getByRole("button", { name: "Check and notify" }).click()
  await expect(page.getByText("Withdrawal notification succeeded", { exact: true })).toBeVisible()
  await postControl(request, "/test/relay", {})
  await page.reload()
  await expect(page.getByRole("button", { name: /Base wallet connected as 0xf39F/i })).toBeVisible()
  await page.getByRole("button", { name: "Connect IC wallet", exact: true }).click()
  await page.getByRole("button", { name: "Plug" }).click()
  await page.getByRole("button", { name: "Close confirmation" }).click()
  await page.getByRole("button", { name: "Refresh", exact: true }).click()
  await expect(page.getByText("AcknowledgePending", { exact: true })).toBeVisible({ timeout: 30_000 })
  await expect(page.getByText(/Confirming automatically/)).toBeVisible()
  await expect(page.getByRole("button", { name: "Retry settlement" })).toHaveCount(0)
  const secondAdvance = await postControl(request, "/test/advance-confirmation", { minutes: 3 }) as { time: number }
  await page.clock.setFixedTime(secondAdvance.time)
  await page.getByRole("button", { name: "Refresh", exact: true }).click()
  await expect.poll(async () => BigInt((await controlState(request)).ledgerBalance)).toBe(BigInt(beforeWithdrawal.ledgerBalance) + 98_990_000n)
  await expect.poll(async () => {
    const state = await controlState(request)
    return state.indexBalance === state.ledgerBalance
  }, { timeout: 30_000 }).toBe(true)
  const final = await controlState(request)
  expect(BigInt(final.indexBlocksSynced)).toBeGreaterThan(BigInt(afterDeposit.indexBlocksSynced))
  expect(BigInt(final.bsnsBalance)).toBe(198_000_000n)
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

interface ControlState {
  bsnsBalance: string
  bsnsAllowance: string
  ledgerBalance: string
  ledgerId: string
  indexBalance: string
  indexLedgerId: string
  indexBlocksSynced: string
  notifyCalls: number
  knownDepositCount: number
  depositSequences: string[]
  nextDepositSequence: string
}

async function controlState(request: APIRequestContext): Promise<ControlState> {
  const response = await request.get("http://127.0.0.1:43119/test/state")
  expect(response.ok(), await response.text()).toBe(true)
  return response.json() as Promise<ControlState>
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
