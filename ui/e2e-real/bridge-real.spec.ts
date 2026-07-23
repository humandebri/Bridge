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
  await page.getByRole("checkbox", { name: "Acknowledge unaudited bridge risk" }).check()
  await page.getByRole("button", { name: "Acknowledge and continue" }).click()
  await expect(page.getByRole("heading", { name: "Bridge KINIC" })).toBeVisible()
  await page.goto("/status")
  await expect(page.getByText("Availability is fail-closed until fresh status checks succeed.")).toBeHidden()
  await expect(page.getByText("To Base").locator("..")).toContainText("Available")
  await page.goto("/")
  await page.getByRole("button", { name: "Connect Base wallet", exact: true }).click()
  await page.getByRole("button", { name: "Connect Browser wallet", exact: true }).click()
  await expect(page.getByRole("dialog").getByText("0xf39F…2266", { exact: true })).toBeVisible()
  await page.getByRole("button", { name: "Close confirmation" }).click()
  await expect(page.getByRole("button", { name: /Base wallet connected as 0xf39F/i })).toBeVisible()

  await page.getByRole("button", { name: "Connect IC wallet", exact: true }).click()
  await page.getByRole("button", { name: "Plug" }).click()
  await expect(page.getByRole("dialog").getByRole("button", { name: "Disconnect Plug", exact: true })).toBeVisible()
  await page.getByRole("button", { name: "Close confirmation" }).click()
  await expect(page.getByRole("button", { name: /IC wallet connected as /i })).toBeVisible()

  await refreshBridgeData(page)
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
  await page.getByRole("button", { name: "Confirm and open wallet" }).click()
  await expect.poll(async () => (await controlState(request)).knownDepositCount).toBe(1)
  await expect(page.getByText("Deposit status unavailable", { exact: true })).toBeVisible()
  await expect(page.getByRole("button", { name: "Retry same deposit" })).toBeVisible()
  expect(await controlState(request)).toMatchObject({ knownDepositCount: 1, depositSequences: ["0"], nextDepositSequence: "1" })

  await page.reload()
  await expect(page.getByRole("button", { name: /Base wallet connected as 0xf39F/i })).toBeVisible()
  await page.getByRole("button", { name: "Connect IC wallet", exact: true }).click()
  await page.getByRole("button", { name: "Plug" }).click()
  await page.getByRole("button", { name: "Close confirmation" }).click()
  await refreshBridgeData(page)
  await expect(page.getByRole("button", { name: "Retry same deposit" })).toBeVisible()
  await page.getByRole("button", { name: "Retry same deposit" }).click()
  await page.getByRole("button", { name: "Confirm and open wallet" }).click()
  await expect(page.getByText(/Deposit 0x[0-9a-f]+… is scheduled/i)).toBeVisible()
  await expect(page.getByText("Deposit status unavailable", { exact: true })).toHaveCount(0)
  await expect(page.getByRole("button", { name: "Bridge to Base" })).toBeVisible()
  const afterRecovery = await controlState(request)
  expect(afterRecovery).toMatchObject({ knownDepositCount: 1, depositSequences: ["0", "0"], nextDepositSequence: "1" })
  expect(BigInt(initial.ledgerBalance) - BigInt(afterRecovery.ledgerBalance)).toBe(200_020_000n)

  await page.evaluate(() => {
    Object.defineProperty(document, "visibilityState", { configurable: true, value: "hidden" })
  })
  const secondPage = await page.context().newPage()
  await installAnvilWallet(secondPage)
  await secondPage.goto("/")
  await secondPage.getByRole("button", { name: "Connect IC wallet", exact: true }).click()
  await secondPage.getByRole("button", { name: "Plug" }).click()
  await secondPage.getByRole("button", { name: "Close confirmation" }).click()
  await expect.poll(async () => page.evaluate(() => Object.keys(localStorage).some((key) => key.startsWith("kinic.bridge.confirmation-lease.v2:")))).toBe(false)
  const beforeTwoTabConfirmation = await controlState(request)
  await postControl(request, "/test/hold-next-confirm-deposit", {})
  await postControl(request, "/test/settle", {})
  const pendingDeposit = await postControl(request, "/test/pending-deposit", {}) as PendingDepositFixture
  await secondPage.bringToFront()
  await secondPage.evaluate((pending) => {
    const key = `kinic.bridge.pending-confirmations.v4:${pending.chainId}:${pending.bridgeAddress.toLowerCase()}:${pending.bridgeCanisterId}`
    window.localStorage.setItem(key, JSON.stringify({ version: 4, entries: [{ ...pending, bridgeAddress: pending.bridgeAddress.toLowerCase(), blocked: false, kind: "deposit" }] }))
    window.dispatchEvent(new Event("kinic-pending-confirmations-changed"))
  }, pendingDeposit)
  await expect.poll(async () => (await controlState(request)).confirmDepositCalls).toBe(beforeTwoTabConfirmation.confirmDepositCalls + 1)
  await secondPage.evaluate(() => Object.defineProperty(document, "visibilityState", { configurable: true, value: "hidden" }))
  await page.evaluate(() => Object.defineProperty(document, "visibilityState", { configurable: true, value: "visible" }))
  await page.bringToFront()
  await page.evaluate(() => window.dispatchEvent(new Event("kinic-pending-confirmations-changed")))
  await page.waitForTimeout(300)
  expect((await controlState(request)).confirmDepositCalls).toBe(beforeTwoTabConfirmation.confirmDepositCalls + 1)
  await postControl(request, "/test/release-confirm-deposit", {})
  await expect.poll(async () => (await controlState(request)).completedConfirmDepositCalls).toBe(beforeTwoTabConfirmation.completedConfirmDepositCalls + 1)
  await page.waitForTimeout(300)
  expect((await controlState(request)).confirmDepositCalls).toBe(beforeTwoTabConfirmation.confirmDepositCalls + 1)
  await secondPage.close()

  await page.getByLabel("You send").fill("1.00000000")
  await page.getByRole("button", { name: "Bridge to Base" }).click()
  await page.getByRole("button", { name: "Confirm and open wallet" }).click()
  await expect(page.getByText(/Deposit 0x[0-9a-f]+… is scheduled/i)).toBeVisible()
  await expect.poll(async () => {
    const state = await controlState(request)
    return {
      knownDepositCount: state.knownDepositCount,
      depositSequences: state.depositSequences,
      nextDepositSequence: state.nextDepositSequence,
    }
  }).toEqual({ knownDepositCount: 2, depositSequences: ["0", "0", "1"], nextDepositSequence: "2" })

  await postControl(request, "/test/settle", {})
  const upgrade = (await postControl(request, "/test/upgrade", {})) as { before: unknown; after: unknown }
  expect(upgrade.after).toEqual(upgrade.before)
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
  const depositState = await refreshHistoryUntil(page, /^(Processing|Complete)$/)
  if (depositState === "Processing") {
    await expect(page.getByText("Waiting for wallet-confirmed finalized verification", { exact: true }).first()).toBeVisible()
    await expect(page.getByRole("button", { name: "Retry", exact: true })).toHaveCount(0)
    // Cross the two-minute boundary with one minute of margin for PocketIC timer rounding.
    const firstAdvance = await postControl(request, "/test/advance-confirmation", { minutes: 3 }) as { time: number }
    await page.clock.setFixedTime(firstAdvance.time)
  }

  const beforeWithdrawal = await controlState(request)
  await page.getByRole("link", { name: "KINIC Bridge home" }).click()
  await page.getByRole("button", { name: "Reverse bridge direction" }).click()
  await refreshBridgeData(page)
  await expect(page.getByText("KINIC", { exact: true }).first()).toBeVisible()
  await page.getByLabel("You send").fill("1.00000000")
  await expect(page.getByText("0.99 TICRC1", { exact: true })).toBeVisible()
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
  await page.getByRole("checkbox", { name: "Acknowledge irreversible burn" }).check()
  await page.getByRole("button", { name: "Confirm and open wallet" }).click()
  await expect(page.getByText(/Withdrawal submitted:/)).toBeVisible()
  await expect.poll(async () => (await controlState(request)).notifyCalls).toBe(beforeWithdrawal.notifyCalls + 1)
  const retryClock = await page.evaluate(() => Date.now())
  await page.clock.setFixedTime(retryClock + 60_000)
  await expect.poll(async () => (await controlState(request)).notifyCalls, { timeout: 45_000 }).toBeGreaterThanOrEqual(beforeWithdrawal.notifyCalls + 2)
  await expect(page.getByText("Withdrawal is recorded. Processing will continue automatically.", { exact: true })).toBeVisible()
  expect((await controlState(request)).bsnsAllowance).toBe("0")
  await openHistory(page)
  await page.getByRole("tab", { name: "Withdrawals" }).click()
  await page.getByRole("button", { name: "Refresh", exact: true }).click()
  await postControl(request, "/test/relay", {})
  await page.reload()
  await expect(page.getByRole("button", { name: /Base wallet connected as 0xf39F/i })).toBeVisible()
  await page.getByRole("button", { name: "Connect IC wallet", exact: true }).click()
  await page.getByRole("button", { name: "Plug" }).click()
  await page.getByRole("button", { name: "Close confirmation" }).click()
  await page.getByRole("button", { name: "Refresh", exact: true }).click()
  await expect(page.getByText("Paid", { exact: true })).toBeVisible({ timeout: 30_000 })
  await expect(page.getByRole("button", { name: "Retry", exact: true })).toHaveCount(0)
  await expect.poll(async () => BigInt((await controlState(request)).ledgerBalance)).toBe(BigInt(beforeWithdrawal.ledgerBalance) + 99_000_000n)
  await expect.poll(async () => {
    const state = await controlState(request)
    return state.indexBalance === state.ledgerBalance
  }, { timeout: 30_000 }).toBe(true)
  const final = await controlState(request)
  expect(BigInt(final.indexBlocksSynced)).toBeGreaterThan(BigInt(afterDeposit.indexBlocksSynced))
  expect(BigInt(final.bsnsBalance)).toBe(198_000_000n)
  await expect(page.getByText("Paid", { exact: true })).toBeVisible({ timeout: 30_000 })
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

async function refreshBridgeData(page: Page): Promise<void> {
  const bridge = page.getByRole("region", { name: "KINIC bridge" })
  const refresh = bridge.getByRole("button", { name: "Refresh", exact: true })
  await refresh.click()
  await expect(bridge.getByRole("button", { name: "Refreshing…", exact: true })).toBeVisible()
  await expect(refresh).toBeEnabled({ timeout: 90_000 })
}

async function refreshHistoryUntil(page: Page, state: RegExp): Promise<string> {
  for (let attempt = 0; attempt < 5; attempt += 1) {
    await page.getByRole("button", { name: "Refresh", exact: true }).click()
    const badge = page.getByText(state, { exact: true }).first()
    try {
      await badge.waitFor({ state: "visible", timeout: 5_000 })
      return await badge.innerText()
    } catch {
      // A concurrent automatic confirmation can invalidate the same history query.
    }
  }
  throw new Error(`History did not show ${state}`)
}

async function postControl(request: APIRequestContext, path: string, data: unknown): Promise<unknown> {
  const response = await request.post(`http://127.0.0.1:43119${path}`, { data })
  expect(response.ok(), await response.text()).toBe(true)
  return response.json()
}

interface PendingDepositFixture {
  bridgeAddress: string
  bridgeCanisterId: string
  chainId: number
  owner: string
  settlementId: string
  transactionHash: string
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
  confirmDepositCalls: number
  completedConfirmDepositCalls: number
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
