import { Cbor, requestIdOf } from "@icp-sdk/core/agent"
import { expect, test, type APIRequestContext, type Page, type Request, type Response, type Route } from "@playwright/test"

const DEPLOYER = "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266"

test.beforeEach(async ({ page }) => {
  await installAnvilWallet(page)
})

test("deposits through the real ledger, canister, and Anvil contract", async ({ page, request }) => {
  test.setTimeout(600_000)
  await expect.poll(async () => {
    const state = await controlState(request)
    return { bound: state.indexLedgerId === state.ledgerId, synced: state.indexBalance === state.ledgerBalance }
  }, { timeout: 30_000 }).toEqual({ bound: true, synced: true })
  const initial = await controlState(request)
  await page.goto("/")
  await page.getByRole("checkbox", { name: "Acknowledge unaudited bridge risk" }).check()
  await page.getByRole("button", { name: "Acknowledge and continue" }).click()
  await expect(page.getByRole("region", { name: "KINIC bridge" })).toBeVisible()
  await page.goto("/status")
  await expect(page.getByText("Live availability is unknown until current status checks succeed.")).toBeHidden()
  await expect(page.getByText("To Base").locator("..")).toContainText("Available")
  await page.goto("/")
  await page.getByRole("button", { name: "Connect EVM wallet", exact: true }).click()
  await page.getByRole("button", { name: "Connect Browser wallet", exact: true }).click()
  await expect(page.getByRole("dialog").getByText("0xf39F…2266", { exact: true })).toBeVisible()
  await page.getByRole("button", { name: "Close confirmation" }).click()
  await expect(page.getByRole("button", { name: /EVM wallet connected as 0xf39F/i })).toBeVisible()

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
  await page.getByRole("button", { name: "Continue to IC wallet" }).click()
  await expect.poll(
    async () => (await controlState(request)).knownDepositCount,
    { timeout: 60_000 },
  ).toBe(1)
  await expect(page.getByText("Deposit status unavailable", { exact: true })).toBeVisible()
  await page.getByRole("button", { name: "Close", exact: true }).click()
  await expect(page.getByRole("button", { name: "Check status" })).toBeVisible()
  expect(await controlState(request)).toMatchObject({ knownDepositCount: 1, depositSequences: ["0"], nextDepositSequence: "1" })

  await page.reload()
  await expect(page.getByRole("button", { name: /EVM wallet connected as 0xf39F/i })).toBeVisible()
  await expect(page.getByRole("button", { name: /IC wallet connected as /i })).toBeVisible()
  await expect(page.getByRole("button", { name: "Connect IC wallet", exact: true })).toHaveCount(0)
  await refreshBridgeData(page)
  await page.getByRole("button", { name: "Check status" }).click()
  await expect(page.getByText(/Ledger escrowを処理中|Mint Authorizationを署名中|Mint Authorization ready|Your tokens were minted on Base/).first()).toBeVisible()
  await expect(page.getByText("Deposit status unavailable", { exact: true })).toHaveCount(0)
  await expect(page.getByRole("button", { name: "Bridge to Base" })).toHaveCount(0)
  const afterRecovery = await controlState(request)
  expect(afterRecovery).toMatchObject({ knownDepositCount: 1, depositSequences: ["0"], nextDepositSequence: "1" })
  expect(BigInt(initial.ledgerBalance) - BigInt(afterRecovery.ledgerBalance)).toBe(
    200_000_000n + 2n * BigInt(initial.ledgerFee),
  )
  await expect.poll(async () => BigInt((await controlState(request)).bsnsBalance), { timeout: 60_000 }).toBe(199_000_000n)

  await postControl(request, "/test/settle", {})
  await expect(page.getByRole("button", { name: "Close", exact: true })).toBeVisible()
  await expect(page.getByLabel("You send")).toHaveValue("")
  await page.getByRole("button", { name: "Close", exact: true }).click()
  await refreshBridgeData(page)
  await expect(page.getByLabel("You send")).toBeEnabled()
  await expect(page.getByRole("button", { name: "Reverse bridge direction" })).toBeEnabled()
  await expect.poll(async () => BigInt((await controlState(request)).bsnsBalance)).toBe(199_000_000n)
  await expect(page.getByRole("button", { name: "Bridge to Base" })).toBeDisabled()

  await page.getByLabel("You send").fill("1.00000000")
  await page.getByRole("button", { name: "Bridge to Base" }).click()
  await page.getByRole("button", { name: "Continue to IC wallet" }).click()
  await expect(page.getByText(/Ledger escrowを処理中|Mint Authorizationを署名中|Mint Authorization ready|Your tokens were minted on Base/).first()).toBeVisible()
  await expect.poll(async () => {
    const state = await controlState(request)
    return {
      knownDepositCount: state.knownDepositCount,
      depositSequences: state.depositSequences,
      nextDepositSequence: state.nextDepositSequence,
    }
  }).toEqual({ knownDepositCount: 2, depositSequences: ["0", "1"], nextDepositSequence: "2" })
  await expect.poll(async () => BigInt((await controlState(request)).bsnsBalance), { timeout: 60_000 }).toBe(298_000_000n)

  await postControl(request, "/test/settle", {})
  await expect(page.getByRole("button", { name: "Close", exact: true })).toBeVisible()
  await page.getByRole("button", { name: "Close", exact: true }).click()
  await refreshBridgeData(page)
  await expect.poll(async () => BigInt((await controlState(request)).bsnsBalance)).toBe(298_000_000n)
  const upgrade = (await postControl(request, "/test/upgrade", {})) as { before: unknown; after: unknown }
  expect(upgrade.after).toEqual(upgrade.before)
  await postControl(request, "/test/relay", {})
  await expect.poll(async () => BigInt((await controlState(request)).bsnsBalance)).toBe(298_000_000n)
  await expect.poll(async () => {
    const state = await controlState(request)
    return state.indexBalance === state.ledgerBalance
  }, { timeout: 30_000 }).toBe(true)
  const afterDeposit = await controlState(request)
  expect(BigInt(initial.ledgerBalance) - BigInt(afterDeposit.ledgerBalance)).toBe(
    300_000_000n + 4n * BigInt(initial.ledgerFee),
  )
  expect(BigInt(afterDeposit.indexBlocksSynced)).toBeGreaterThanOrEqual(BigInt(initial.indexBlocksSynced) + 4n)
  await openHistory(page)
  await expect(page.getByText("Minted", { exact: true }).first()).toBeVisible({ timeout: 30_000 })
  for (const heading of ["Direction", "Base tx", "KINIC tx", "Amount", "Status", "Time", "Next step"]) {
    await expect(page.getByText(heading, { exact: true }).filter({ visible: true }).first()).toBeVisible()
  }
  await expect(page.getByText(/^Deposit #[\d,]+$/).first()).toBeVisible()
  await expect(page.getByText(/^Tx 0x[0-9a-f]+…$/).first()).toBeVisible()
  await expect(page.getByText("1.99 KINIC", { exact: true })).toBeVisible()
  await expect(page.getByText(/KINIC (?:on Base|returned to IC|awaiting quote)/)).toHaveCount(0)
  const nextStepHeader = page.getByText("Next step", { exact: true }).filter({ visible: true })
  const completedDepositNextStep = page.locator("article").filter({ hasText: "Minted" }).first().getByText("—", { exact: true })
  const nextStepHeaderBox = await nextStepHeader.boundingBox()
  const completedDepositNextStepBox = await completedDepositNextStep.boundingBox()
  expect(nextStepHeaderBox).not.toBeNull()
  expect(completedDepositNextStepBox).not.toBeNull()
  if (!nextStepHeaderBox || !completedDepositNextStepBox) throw new Error("History Next step alignment could not be measured")
  expect(completedDepositNextStepBox.x).toBeCloseTo(nextStepHeaderBox.x, 0)
  await expect(page.getByRole("button", { name: "Confirm mint on IC" })).toHaveCount(0)
  await page.reload()
  await expect(page.getByRole("button", { name: /IC wallet connected as /i })).toBeVisible()
  await expect(page.getByRole("button", { name: "Connect IC wallet", exact: true })).toHaveCount(0)
  await expect(page.getByText("Minted", { exact: true }).first()).toBeVisible({ timeout: 30_000 })

  const beforeWithdrawal = await controlState(request)
  const bridgeUpdateGate = await holdIcUpdateMethod(page, "continue_withdrawal")
  const bridgeUpdateObserver = observeIcUpdateMethods(page)
  await page.getByRole("link", { name: "KINIC Bridge home" }).click()
  await page.getByRole("button", { name: "Reverse bridge direction" }).click()
  await refreshBridgeData(page)
  await expect(page.getByText("KINIC", { exact: true }).first()).toBeVisible()
  await page.getByLabel("You send").fill("1.00000000")
  await expect(page.getByText("0.99 TICRC1", { exact: true })).toBeVisible()
  const withdraw = page.getByRole("button", { name: "Bridge to IC" })
  await expect.poll(async () => {
    if (!await withdraw.isEnabled()) return await page.getByText(/^Next:/).textContent()
    try {
      await withdraw.click({ timeout: 500 })
      return "opened"
    } catch {
      return await page.getByText(/^Next:/).textContent()
    }
  }, { timeout: 30_000 }).toBe("opened")
  await page.getByRole("button", { name: "Continue to Base wallet" }).click()
  await expect(page.getByText(/Withdrawal submitted:/)).toBeVisible()
  await postControl(request, "/test/prepare-latest-withdrawal", {})
  await expect.poll(async () => BigInt((await controlState(request)).withdrawalCount)).toBe(BigInt(beforeWithdrawal.withdrawalCount) + 1n)
  await expect.poll(() => [...bridgeUpdateObserver.acceptedCalls]).toEqual([
    expect.objectContaining({ method: "notify_withdrawal" }),
  ])
  await postControl(request, "/test/set-ledger-available", { available: false })
  bridgeUpdateGate.release()
  await expect.poll(async () => {
    const state = await postControl(request, "/test/latest-withdrawal-state", {}) as { phase: string; stopReason: string | null }
    return state.stopReason ? state.phase : "running"
  }, { timeout: 45_000 }).toBe("ReconciliationHold")
  await expect.poll(() => [...bridgeUpdateObserver.acceptedCalls]).toEqual([
    expect.objectContaining({ method: "notify_withdrawal" }),
    expect.objectContaining({ method: "continue_withdrawal" }),
  ])
  await postControl(request, "/test/set-ledger-available", { available: true })
  expect([...new Set(bridgeUpdateObserver.attempts.map((call) => call.method))]).toEqual(["notify_withdrawal", "continue_withdrawal"])
  const notificationReceiptCalls = (await controlState(request)).receiptCalls
  const retryClock = await page.evaluate(() => Date.now())
  await page.clock.setFixedTime(retryClock + 60_000)
  await expect.poll(async () => (await controlState(request)).receiptCalls, { timeout: 45_000 }).toBe(notificationReceiptCalls)
  await expect.poll(() => [...bridgeUpdateObserver.acceptedCalls], { timeout: 45_000 }).toEqual([
    expect.objectContaining({ method: "notify_withdrawal" }),
    expect.objectContaining({ method: "continue_withdrawal" }),
  ])
  expect([...new Set(bridgeUpdateObserver.attempts.map((call) => call.method))]).toEqual(["notify_withdrawal", "continue_withdrawal"])
  bridgeUpdateObserver.stop()
  await bridgeUpdateGate.stop()
  await expect(page.getByText("The withdrawal is recorded but needs reconciliation. Open History to review the available action.", { exact: true })).toBeVisible()
  expect((await controlState(request)).bsnsAllowance).toBe("0")
  await openHistory(page)
  await page.locator("header").filter({ has: page.getByRole("heading", { name: "Bridge history" }) })
    .getByRole("button", { name: "Refresh", exact: true }).click()
  await expect(page.getByText("Recovery needed", { exact: true })).toBeVisible()
  await expect(page.getByRole("button", { name: "Continue payout", exact: true })).toBeVisible()
  await page.getByRole("button", { name: /IC wallet connected as /i }).click()
  await page.getByRole("button", { name: /^Disconnect (OISY Wallet|Plug)$/ }).click()
  await page.getByRole("button", { name: "Close confirmation", exact: true }).click()
  await expect(page.getByRole("button", { name: "Connect IC wallet", exact: true })).toBeVisible()
  await page.getByRole("button", { name: "Continue payout", exact: true }).click()
  await postControl(request, "/test/relay", {})
  await page.reload()
  await expect(page.getByRole("button", { name: /EVM wallet connected as 0xf39F/i })).toBeVisible()
  await expect(page.getByRole("button", { name: "Connect IC wallet", exact: true })).toBeVisible()
  for (let attempt = 0; attempt < 4; attempt += 1) {
    await page.locator("header").getByRole("button", { name: "Refresh", exact: true }).click()
    if (await page.getByText("Paid", { exact: true }).isVisible()) break
    const continuePayout = page.getByRole("button", { name: "Continue payout", exact: true })
    if (await continuePayout.isVisible()) {
      await continuePayout.click()
      await postControl(request, "/test/relay", {})
    } else {
      await expect(page.getByText("Paid", { exact: true })).toBeVisible()
      break
    }
  }
  await expect(page.getByText("Paid", { exact: true })).toBeVisible({ timeout: 30_000 })
  await expect(page.getByText("0.99 KINIC", { exact: true }).last()).toBeVisible()
  await expect(page.getByText(/^Payout #[\d,]+$/).first()).toBeVisible()
  await expect(page.getByText(/KINIC to IC/)).toHaveCount(0)
  await expect(page.getByRole("button", { name: "Continue payout", exact: true })).toHaveCount(0)
  await expect.poll(async () => BigInt((await controlState(request)).ledgerBalance)).toBe(BigInt(beforeWithdrawal.ledgerBalance) + 99_000_000n)
  await expect.poll(async () => {
    const state = await controlState(request)
    return state.indexBalance === state.ledgerBalance
  }, { timeout: 30_000 }).toBe(true)
  const final = await controlState(request)
  expect(BigInt(final.indexBlocksSynced)).toBeGreaterThan(BigInt(afterDeposit.indexBlocksSynced))
  expect(BigInt(final.bsnsBalance)).toBe(198_000_000n)
  await expect(page.getByText("Base → IC", { exact: true }).first()).toBeVisible()
  await expect(page.getByText("Paid", { exact: true })).toBeVisible({ timeout: 30_000 })

})

test("claims an expired deposit refund from History", async ({ page, request }) => {
  test.setTimeout(600_000)
  await postControl(request, "/test/prepare-refundable-deposit", {})
  await page.goto("/")
  await page.getByRole("checkbox", { name: "Acknowledge unaudited bridge risk" }).check()
  await page.getByRole("button", { name: "Acknowledge and continue" }).click()
  await page.getByRole("button", { name: "Connect IC wallet", exact: true }).click()
  await page.getByRole("button", { name: "Plug" }).click()
  await expect(page.getByRole("dialog").getByRole("button", { name: "Disconnect Plug", exact: true })).toBeVisible()
  await page.getByRole("button", { name: "Close confirmation" }).click()
  await openHistory(page)
  await page.locator("header").getByRole("button", { name: "Refresh", exact: true }).click()
  const refundRow = page.locator("article").filter({ hasText: "Not submitted" }).filter({
    has: page.getByRole("button", { name: "Claim refund", exact: true }),
  }).first()
  await expect(refundRow).toHaveCount(1)
  await expect(refundRow.getByRole("button", { name: "Claim refund", exact: true })).toBeVisible()
  const refundResponse = page.waitForResponse((candidate) =>
    candidate.request().method() === "POST" && candidate.url().endsWith("/ic/request-deposit-refund"),
    { timeout: 120_000 },
  )
  await refundRow.getByRole("button", { name: "Claim refund", exact: true }).click()
  const completedRefund = await refundResponse
  expect(completedRefund.ok()).toBe(true)
  expect(await completedRefund.json()).toHaveProperty("state.Refunded")
  await page.reload()
  const refundedRow = page.locator("article").filter({ hasText: "Not submitted" }).first()
  await expect(refundedRow.getByText("Refunded", { exact: true })).toBeVisible({ timeout: 30_000 })
  await expect(refundedRow.getByRole("button", { name: /Claim refund|Request refund/ })).toHaveCount(0)
})

async function openHistory(page: Page): Promise<void> {
  const historyLink = page.locator('a[href="/history"]:visible').first()
  await expect(historyLink).toBeVisible()
  await historyLink.evaluate((link: HTMLAnchorElement) => link.click())
  await expect(page).toHaveURL(/\/history$/)
  const progressOverlay = page.locator('div[data-state="open"][aria-hidden="true"].fixed.inset-0').last()
  if (await progressOverlay.isVisible()) {
    await progressOverlay.click({ position: { x: 4, y: 4 } })
    await expect(progressOverlay).toBeHidden()
  }
}

async function refreshBridgeData(page: Page): Promise<void> {
  const bridge = page.getByRole("region", { name: "KINIC bridge" })
  const refresh = bridge.getByRole("button", { name: "Refresh", exact: true })
  await refresh.click()
  await expect(refresh).toBeEnabled({ timeout: 90_000 })
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
  ledgerFee: string
  ledgerId: string
  indexBalance: string
  indexLedgerId: string
  indexBlocksSynced: string
  withdrawalCount: string
  receiptCalls: string
  knownDepositCount: number
  depositSequences: string[]
  nextDepositSequence: string
}

function observeIcUpdateMethods(page: Page): {
  attempts: Array<{ method: string; requestId: string }>
  acceptedCalls: Array<{ method: string; requestId: string }>
  stop: () => void
} {
  const attempts: Array<{ method: string; requestId: string }> = []
  const acceptedCalls: Array<{ method: string; requestId: string }> = []
  const requestListener = (request: Request) => {
    const call = decodeIcUpdateRequest(request)
    if (call) attempts.push(call)
  }
  const responseListener = (response: Response) => {
    if (response.status() !== 200 && response.status() !== 202) return
    const call = decodeIcUpdateRequest(response.request())
    if (call) acceptedCalls.push(call)
  }
  page.on("request", requestListener)
  page.on("response", responseListener)
  return {
    attempts,
    acceptedCalls,
    stop: () => {
      page.off("request", requestListener)
      page.off("response", responseListener)
    },
  }
}

async function holdIcUpdateMethod(page: Page, method: string): Promise<{ release: () => void; stop: () => Promise<void> }> {
  let release!: () => void
  const released = new Promise<void>((resolve) => { release = resolve })
  const pattern = /\/api\/v[234]\/canister\/[^/]+\/call$/
  const handler = async (route: Route) => {
    if (decodeIcUpdateRequest(route.request())?.method === method) await released
    await route.continue()
  }
  await page.route(pattern, handler)
  return { release, stop: () => page.unroute(pattern, handler) }
}

function decodeIcUpdateRequest(request: Request): { method: string; requestId: string } | undefined {
  const pathname = new URL(request.url()).pathname
  if (request.method() !== "POST" || !/^\/api\/v[234]\/canister\/[^/]+\/call$/.test(pathname)) return undefined
  const body = request.postDataBuffer()
  if (!body) return { method: "<missing-body>", requestId: "<unknown>" }
  try {
    const envelope = Cbor.decode<{ content?: Record<string, unknown> & { method_name?: unknown } }>(new Uint8Array(body))
    return {
      method: typeof envelope.content?.method_name === "string" ? envelope.content.method_name : "<missing-method>",
      requestId: envelope.content ? hexBytes(requestIdOf(envelope.content)) : "<unknown>",
    }
  } catch {
    return { method: "<invalid-cbor>", requestId: "<unknown>" }
  }
}

function hexBytes(value: Uint8Array): string {
  return Array.from(value, (byte) => byte.toString(16).padStart(2, "0")).join("")
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
