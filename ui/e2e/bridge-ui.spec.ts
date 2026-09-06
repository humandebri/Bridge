import AxeBuilder from "@axe-core/playwright"
import { expect, test } from "@playwright/test"
import type { Page, TestInfo } from "@playwright/test"

const RISK_ACKNOWLEDGEMENT_KEY = "kinic.bridge.risk-acknowledgement.v1:84532::"
const BRIDGE_PROGRESS_KEY = "kinic.bridge.latest-progress.v3:84532:null::"
const PENDING_CONFIRMATIONS_KEY = "kinic.bridge.pending-confirmations.v7:84532::"
const OBSOLETE_BRIDGE_PROGRESS_KEY = "kinic.bridge.latest-progress.v2:84532:null::"
const OTHER_DEPLOYMENT_PROGRESS_KEY =
  "kinic.bridge.latest-progress.v3:84532:0x1111111111111111111111111111111111111111:aaaaa-aa:other"

test.beforeEach(async ({ page }, testInfo) => {
  if (testInfo.title === "requires risk acknowledgement on first use") return
  await page.addInitScript(
    (key) => window.localStorage.setItem(key, "acknowledged"),
    RISK_ACKNOWLEDGEMENT_KEY,
  )
})

test("requires risk acknowledgement on first use", async ({ page }, testInfo) => {
  await page.goto("/")
  await expect(page.getByRole("heading", { name: "Unaudited bridge" })).toBeVisible()
  await expect(page.getByText("This bridge has not been audited.")).toBeVisible()
  await expect(page.getByRole("button", { name: "Close confirmation" })).toHaveCount(0)
  await expect(page.getByRole("button", { name: "Acknowledge and continue" })).toBeDisabled()
  await expectNoWcag21AaViolations(page, '[role="dialog"]')
  await capture(page, testInfo, "risk-acknowledgement")

  await page.keyboard.press("Escape")
  await expect(page.getByRole("dialog")).toBeVisible()
  await page.mouse.click(5, 5)
  await expect(page.getByRole("dialog")).toBeVisible()
  await page.getByRole("checkbox", { name: "Acknowledge unaudited bridge risk" }).check()
  await page.getByRole("button", { name: "Acknowledge and continue" }).click()
  await expect(page.getByRole("dialog")).toBeHidden()
  await expect
    .poll(() => page.evaluate((key) => window.localStorage.getItem(key), RISK_ACKNOWLEDGEMENT_KEY))
    .toBe("acknowledged")

  await page.reload()
  await expect(page.getByRole("heading", { name: "Unaudited bridge" })).toBeHidden()
})

test("bridge defaults to IC to Base and reports incomplete configuration", async ({
  page,
}, testInfo) => {
  await page.goto("/")
  await expect(page.getByText("Bridge KINIC", { exact: true })).toHaveCount(0)
  await expect(page.getByText("Move tokens between IC and Base.", { exact: true })).toHaveCount(0)
  await expect(page.getByText("1:1 across both networks", { exact: true })).toHaveCount(0)
  await expect(page.getByRole("region", { name: "KINIC bridge" })).toBeVisible()
  const homeLink = page.getByRole("link", { name: "KINIC Bridge home" })
  await expect(homeLink).toContainText("KINIC Bridge")
  await expect(homeLink.locator("img")).toHaveAttribute("src", /blue_kinic/)
  await expect(page.locator('link[rel="icon"]')).toHaveAttribute("href", /blue_kinic/)
  await expect(page.locator('link[rel="apple-touch-icon"]')).toHaveAttribute("href", /blue_kinic/)
  await expect(page.getByText("IC ↔ Base", { exact: true })).toHaveCount(0)
  await expect(page.getByText("Internet Computer")).toBeVisible()
  await expect(
    page
      .getByRole("button", { name: "From Internet Computer Connect IC wallet", exact: true })
      .locator('[data-network-logo="ic"]'),
  ).toBeVisible()
  await expect(
    page
      .getByRole("button", { name: "To Base Connect EVM wallet", exact: true })
      .locator('[data-network-logo="base"]'),
  ).toBeVisible()
  await expect(page.getByText("Refresh before continuing.")).toBeHidden()
  await expect(
    page.getByText(
      "Live status is not confirmed. Current conditions will be checked before continuing.",
    ),
  ).toHaveCount(0)
  await expect(page.getByRole("button", { name: "Bridge to Base" })).toBeDisabled()
  await expect(page.getByRole("button", { name: "MAX" })).toBeVisible()
  await expect(page.getByRole("button", { name: "MAX" })).toBeDisabled()
  await expect(page.getByLabel("You send")).toHaveAttribute("aria-invalid", "true")
  await expect(page.getByLabel("You send")).toHaveAttribute(
    "aria-describedby",
    "bridge-amount-feedback",
  )
  await expect(page.getByRole("button", { name: "Reverse bridge direction" })).toHaveCSS(
    "width",
    "32px",
  )
  await expect(page.getByRole("button", { name: "Reverse bridge direction" })).toHaveCSS(
    "height",
    "32px",
  )
  if ((page.viewportSize()?.width ?? 0) >= 768) {
    await expect(page.getByRole("link", { name: "Open history" })).toBeVisible()
    await expect(page.getByRole("link", { name: "Open status" })).toBeVisible()
    await expect(page.getByLabel("Open navigation menu")).toBeHidden()
  } else {
    await expect(page.getByRole("link", { name: "Open history" })).toBeHidden()
    await expect(page.getByRole("link", { name: "Open status" })).toBeHidden()
    await expect(page.getByLabel("Open navigation menu")).toBeVisible()
  }
  if ((page.viewportSize()?.width ?? 0) >= 1024) {
    const panel = await page.getByTestId("bridge-panel").boundingBox()
    expect(panel).not.toBeNull()
    expect(panel!.width).toBeLessThanOrEqual(621)
    expect(
      Math.abs(panel!.x + panel!.width / 2 - (page.viewportSize()?.width ?? 0) / 2),
    ).toBeLessThanOrEqual(2)
  }
  await expect(page.locator(".kinic-rail i")).toHaveCount(4)
  await expectNoWcag21AaViolations(page)
  await capture(page, testInfo, "bridge-deposit")
})

test("direction switch is URL-backed and updates bridge endpoints", async ({ page }, testInfo) => {
  await page.goto("/?direction=deposit")
  await expect(page.locator(".kinic-rail")).not.toHaveClass(/is-withdraw/)
  await page.getByRole("button", { name: "Reverse bridge direction" }).click()
  await expect(page).toHaveURL(/direction=withdraw/)
  await expect(page.locator(".kinic-rail")).toHaveClass(/is-withdraw/)
  await expect(
    page
      .getByRole("button", { name: "From Base Connect EVM wallet", exact: true })
      .locator('[data-network-logo="base"]'),
  ).toBeVisible()
  await expect(
    page
      .getByRole("button", { name: "To Internet Computer Connect IC wallet", exact: true })
      .locator('[data-network-logo="ic"]'),
  ).toBeVisible()
  await expect(page.getByRole("button", { name: "Bridge to IC" })).toBeDisabled()
  await expect(page.getByText("Estimated receive", { exact: true })).toBeVisible()
  await capture(page, testInfo, "bridge-withdraw")
})

test("withdrawal progress separates wallet operations and restores after minimize", async ({
  page,
}, testInfo) => {
  await page.addInitScript(
    ({ key, value }) => window.localStorage.setItem(key, JSON.stringify(value)),
    {
      key: BRIDGE_PROGRESS_KEY,
      value: {
        version: 3,
        id: "withdraw:e2e-progress",
        direction: "withdraw",
        phase: "base-withdrawal-submitted",
        source: "0x1111111111111111111111111111111111111111",
        destination: "aaaaa-aa",
        sendAmount: "10",
        receiveAmount: "10",
        sendSymbol: "KINIC",
        receiveSymbol: "KINIC",
        tokenApproval: "not-required",
        createdAt: 1,
        transactionHash: `0x${"ab".repeat(32)}`,
        withdrawal: { owner: "0x1111111111111111111111111111111111111111" },
      },
    },
  )
  await page.goto("/")

  const restore = page.getByRole("button", { name: /Open transfer progress/ })
  await expect(restore).toBeVisible()
  await restore.click()
  await expect(page.getByRole("heading", { name: "Bridge to IC" })).toBeVisible()

  const progress = page.getByRole("list", { name: "Transfer progress" })
  await expect(progress.getByRole("listitem")).toHaveCount(7)
  await expect(progress.getByText("IC destination verification", { exact: true })).toBeVisible()
  await expect(progress.getByText("Base token approval", { exact: true })).toBeVisible()
  await expect(progress.getByText("Not required", { exact: true })).toBeVisible()
  await expect(progress.locator('li[aria-current="step"]')).toContainText(
    "Base withdrawal transaction",
  )
  await expect(progress.getByText("Base finality", { exact: true })).toBeVisible()
  await expect(progress.getByText("IC notification", { exact: true })).toBeVisible()
  await expect(progress.getByText("Ledger payout", { exact: true })).toBeVisible()
  await expect(progress.getByText("Complete", { exact: true })).toBeVisible()
  await capture(page, testInfo, "withdrawal-progress")

  await page.getByRole("button", { name: "Minimize" }).click()
  await expect(page.getByRole("heading", { name: "Bridge to IC" })).toBeHidden()
  await expect(restore).toBeVisible()
  await restore.click()
  await expect(page.getByRole("heading", { name: "Bridge to IC" })).toBeVisible()
})

test("restored deposit progress survives offline reload boundaries without repeating a wallet write", async ({
  page,
}, testInfo) => {
  await installWalletCallCounters(page)
  await page.addInitScript(
    ({ key, value }) => {
      if (window.sessionStorage.getItem("kinic-e2e-deposit-recovery-seeded") === "true") return
      window.localStorage.setItem(key, JSON.stringify(value))
      window.sessionStorage.setItem("kinic-e2e-deposit-recovery-seeded", "true")
    },
    {
      key: BRIDGE_PROGRESS_KEY,
      value: {
        version: 3,
        id: "deposit:e2e-recovery",
        direction: "deposit",
        phase: "authorization-generating",
        source: "aaaaa-aa",
        destination: "0x1111111111111111111111111111111111111111",
        sendAmount: "10",
        receiveAmount: "9.5",
        sendSymbol: "KINIC",
        receiveSymbol: "KINIC",
        tokenApproval: "not-required",
        createdAt: 1,
        deposit: {
          owner: "aaaaa-aa",
          ownerSequence: "3",
          depositId: `0x${"cd".repeat(32)}`,
        },
      },
    },
  )
  await page.goto("/")

  const restore = page.getByRole("button", { name: /Open transfer progress/ })
  await expect(restore).toBeVisible()
  await restore.click()
  await expect(page.getByRole("heading", { name: "Bridge to Base" })).toBeVisible()
  await expect(page.locator('li[aria-current="step"]')).toContainText("Bridge authorization")
  expect(await walletCallCounters(page)).toEqual({ evm: 0, ic: 0 })

  await page.context().setOffline(true)
  await expect(page.getByRole("heading", { name: "Bridge to Base" })).toBeVisible()
  await page.context().setOffline(false)
  await page.reload()
  await expect(restore).toBeVisible()
  await restore.click()
  await expect(page.locator('li[aria-current="step"]')).toContainText("Bridge authorization")
  expect(await walletCallCounters(page)).toEqual({ evm: 0, ic: 0 })
  await capture(page, testInfo, "deposit-recovery")
})

test("restored attention remains fail closed until explicitly dismissed", async ({
  page,
}, testInfo) => {
  await page.addInitScript(
    ({ key, value }) => {
      if (window.sessionStorage.getItem("kinic-e2e-attention-recovery-seeded") === "true") return
      window.localStorage.setItem(key, JSON.stringify(value))
      window.sessionStorage.setItem("kinic-e2e-attention-recovery-seeded", "true")
    },
    {
      key: BRIDGE_PROGRESS_KEY,
      value: {
        version: 3,
        id: "withdraw:e2e-attention",
        direction: "withdraw",
        phase: "attention",
        source: "0x1111111111111111111111111111111111111111",
        destination: "aaaaa-aa",
        sendAmount: "10",
        receiveAmount: "9.5",
        sendSymbol: "KINIC",
        receiveSymbol: "KINIC",
        tokenApproval: "not-required",
        createdAt: 1,
        transactionHash: `0x${"ef".repeat(32)}`,
        withdrawal: { owner: "aaaaa-aa", withdrawalId: `0x${"ab".repeat(32)}` },
        attentionMessage:
          "The withdrawal is recorded but needs reconciliation. Open History to review the available action.",
        attentionPhase: "ledger-payout",
      },
    },
  )
  await page.goto("/")

  const restore = page.getByRole("button", {
    name: /Open transfer progress: This transfer needs attention/,
  })
  await expect(restore).toBeVisible()
  await restore.click()
  await expect(page.getByRole("alert")).toContainText("needs reconciliation")
  await expect(page.locator('li[aria-current="step"]')).toContainText("Ledger payout")
  await page.getByRole("button", { name: "Close", exact: true }).click()
  await page.reload()
  await expect(restore).toHaveCount(0)
  await capture(page, testInfo, "withdrawal-recovery-attention")
})

test("restores an IC notification failure with an explicit retry action", async ({
  page,
}, testInfo) => {
  const transactionHash = `0x${"ef".repeat(32)}` as const
  await page.addInitScript(
    ({ progressKey, pendingKey, transactionHash }) => {
      window.localStorage.setItem(
        progressKey,
        JSON.stringify({
          version: 3,
          id: "withdraw:e2e-notification-retry",
          direction: "withdraw",
          phase: "attention",
          source: "0x1111111111111111111111111111111111111111",
          destination: "aaaaa-aa",
          sendAmount: "10",
          receiveAmount: "9.5",
          sendSymbol: "KINIC",
          receiveSymbol: "KINIC",
          tokenApproval: "not-required",
          createdAt: 1,
          transactionHash,
          withdrawal: { owner: "aaaaa-aa" },
          attentionMessage: "Base RPC providers disagree on the selected checkpoint hash.",
          attentionPhase: "awaiting-ic-notification",
        }),
      )
      window.localStorage.setItem(
        pendingKey,
        JSON.stringify({
          version: 7,
          entries: [
            {
              kind: "withdrawal",
              transactionHash,
              owner: "aaaaa-aa",
              blocked: false,
              bridgeCanisterId: "",
              chainId: 84532,
              bridgeAddress: "",
              notification: {
                status: "awaiting-notification",
                automaticAttemptUsed: true,
                shortRetryUsed: false,
                finalityReadvanceUsed: false,
                lastAttemptedFinalizedBlock: "100",
                failure: {
                  code: "RpcInconsistent",
                  message: "Base RPC providers disagree on the selected checkpoint hash.",
                  disposition: "manual-retry",
                },
              },
            },
          ],
        }),
      )
    },
    { progressKey: BRIDGE_PROGRESS_KEY, pendingKey: PENDING_CONFIRMATIONS_KEY, transactionHash },
  )

  await page.goto("/")
  const restore = page.getByRole("button", {
    name: /Open transfer progress: This transfer needs attention/,
  })
  await expect(restore).toBeVisible()
  await restore.click()
  await expect(page.getByRole("alert")).toContainText("providers disagree")
  await expect(page.getByRole("button", { name: "Retry IC notification" })).toBeVisible()
  await page.reload()
  await expect(restore).toBeVisible()
  await restore.click()
  await expect(page.getByRole("button", { name: "Retry IC notification" })).toBeVisible()
  await capture(page, testInfo, "withdrawal-notification-retry")
})

test("malformed obsolete and other-deployment recovery records fail closed", async ({ page }) => {
  await page.addInitScript(
    ({ activeKey, obsoleteKey, otherKey }) => {
      window.localStorage.setItem(
        activeKey,
        JSON.stringify({ version: 3, id: "malformed", phase: "ledger-payout" }),
      )
      const otherwiseValid = {
        version: 3,
        id: "withdraw:wrong-scope",
        direction: "withdraw",
        phase: "ledger-payout",
        source: "0x1111111111111111111111111111111111111111",
        destination: "aaaaa-aa",
        sendAmount: "10",
        receiveAmount: "9.5",
        sendSymbol: "KINIC",
        receiveSymbol: "KINIC",
        tokenApproval: "not-required",
        createdAt: 1,
        transactionHash: `0x${"12".repeat(32)}`,
        withdrawal: { owner: "aaaaa-aa", withdrawalId: `0x${"34".repeat(32)}` },
      }
      window.localStorage.setItem(obsoleteKey, JSON.stringify({ ...otherwiseValid, version: 2 }))
      window.localStorage.setItem(otherKey, JSON.stringify(otherwiseValid))
    },
    {
      activeKey: BRIDGE_PROGRESS_KEY,
      obsoleteKey: OBSOLETE_BRIDGE_PROGRESS_KEY,
      otherKey: OTHER_DEPLOYMENT_PROGRESS_KEY,
    },
  )
  await page.goto("/")

  await expect(page.getByRole("button", { name: /Open transfer progress/ })).toHaveCount(0)
  await expect(page.getByRole("button", { name: "Bridge to Base" })).toBeDisabled()
})

test("IC and EVM wallet controls are separate", async ({ page }, testInfo) => {
  await page.addInitScript(() => {
    const provider = { request: () => Promise.resolve([]) }
    const wallets = [
      {
        uuid: "350670db-19fa-4704-a166-e52e178b59d2",
        name: "MetaMask",
        icon: "data:image/svg+xml,<svg xmlns='http://www.w3.org/2000/svg' width='96' height='96'><rect width='96' height='96' fill='orange'/></svg>",
        rdns: "io.metamask",
      },
      {
        uuid: "7c867694-1697-4d85-bf2e-70d6c97a08b5",
        name: "Rabby Wallet",
        icon: "data:image/svg+xml,<svg xmlns='http://www.w3.org/2000/svg' width='96' height='96'><rect width='96' height='96' fill='blue'/></svg>",
        rdns: "io.rabby",
      },
      {
        uuid: "27415cfc-f282-4f8c-8f6e-ec79df93be90",
        name: "Plug",
        icon: "data:image/svg+xml,<svg xmlns='http://www.w3.org/2000/svg' width='96' height='96'><rect width='96' height='96' fill='pink'/></svg>",
        rdns: "com.plugwallet",
      },
    ]
    const announce = () =>
      wallets.forEach((info) =>
        window.dispatchEvent(
          new CustomEvent("eip6963:announceProvider", { detail: { info, provider } }),
        ),
      )
    window.addEventListener("eip6963:requestProvider", announce)
  })
  await page.goto("/")
  await expect(page.getByRole("button", { name: "Connect IC wallet", exact: true })).toBeVisible()
  await expect(page.getByRole("button", { name: "Connect EVM wallet", exact: true })).toBeVisible()
  await page.getByRole("button", { name: "Connect IC wallet", exact: true }).click()
  await expect(page.getByRole("heading", { name: "IC wallet" })).toBeVisible()
  await expect(page.getByRole("dialog").locator('[data-dialog-network-logo="ic"]')).toBeVisible()
  await expect(page.getByRole("button", { name: "Connect OISY Wallet" })).toBeVisible()
  await expect(page.getByRole("img", { name: "OISY Wallet logo" })).toBeVisible()
  await expect(page.getByRole("button", { name: "Connect Plug" })).toBeVisible()
  await expect(page.getByRole("img", { name: "Plug logo" })).toBeVisible()
  await expect(page.getByRole("button", { name: "Connect MetaMask" })).toBeHidden()
  await capture(page, testInfo, "wallet-ic-options")
  await page.getByRole("button", { name: "Close confirmation" }).click()
  await page.getByRole("button", { name: "Connect EVM wallet", exact: true }).click()
  await expect(page.getByRole("heading", { name: "EVM wallet" })).toBeVisible()
  await expect(page.getByRole("dialog").locator('[data-dialog-network-logo="base"]')).toBeVisible()
  await expect(page.getByRole("button", { name: "Connect Coinbase Wallet" })).toBeVisible()
  await expect(page.getByRole("img", { name: "Coinbase Wallet logo" })).toBeVisible()
  await expect(page.getByRole("button", { name: "Connect MetaMask" })).toBeVisible()
  await expect(page.getByRole("button", { name: "Connect Rabby Wallet" })).toBeVisible()
  await expect(page.getByRole("button", { name: "Connect Plug" })).toBeHidden()
  await expect(page.getByRole("button", { name: "Connect Browser wallet" })).toBeHidden()
  await expect(page.getByRole("button", { name: "Connect OISY Wallet" })).toBeHidden()
  await capture(page, testInfo, "wallet-base-options")
})

test("history and status are separate low-density surfaces", async ({ page }, testInfo) => {
  await page.goto("/history")
  await expect(page.getByRole("heading", { name: "Bridge history" })).toBeVisible()
  await expect(page.getByText("Actions unavailable")).toHaveCount(0)
  await expect(page.getByText("Some activity is unavailable")).toHaveCount(0)
  await expect(page.getByText("Connect an IC wallet to include IC → Base activity.")).toHaveCount(0)
  await expect(page.getByText("Connect a wallet", { exact: true })).toBeVisible()
  await expect(page.getByRole("tab")).toHaveCount(0)
  await expect(page.getByRole("button", { name: "All", exact: true })).toHaveCount(0)
  await expect(page.getByRole("button", { name: "To Base", exact: true })).toHaveCount(0)
  await expect(page.getByRole("button", { name: "To IC", exact: true })).toHaveCount(0)
  await expectNoWcag21AaViolations(page)
  await capture(page, testInfo, "history")
  await page.goto("/status")
  await expect(page.getByRole("heading", { name: "Bridge status" })).toBeVisible()
  await expect(
    page.getByText("Current availability across Internet Computer and Base."),
  ).toBeVisible()
  await expect(page.getByText("Bridge checks have not passed.")).toBeHidden()
  await expect(page.getByText("Safe is not finality.")).toBeHidden()
  await expect(page.getByText("Availability", { exact: true })).toBeVisible()
  await expect(page.getByText("Current terms", { exact: true })).toBeVisible()
  await expectNoWcag21AaViolations(page)
  await capture(page, testInfo, "status")
})

async function expectNoWcag21AaViolations(page: Page, include?: string) {
  await page
    .locator(".route-enter")
    .evaluate((element) =>
      Promise.all(element.getAnimations().map((animation) => animation.finished)),
    )
  let builder = new AxeBuilder({ page }).withTags(["wcag2a", "wcag2aa", "wcag21a", "wcag21aa"])
  if (include) builder = builder.include(include)
  const results = await builder.analyze()

  expect(results.violations).toEqual([])
}

async function capture(page: Page, testInfo: TestInfo, name: string) {
  const path = testInfo.outputPath(`${name}.png`)
  await page.screenshot({ fullPage: true, path })
  await testInfo.attach(`${name}-${testInfo.project.name}`, { path, contentType: "image/png" })
}

async function installWalletCallCounters(page: Page) {
  await page.addInitScript(() => {
    const counters = { evm: 0, ic: 0 }
    Reflect.set(window, "__kinicRecoveryWalletCalls", counters)
    Reflect.set(window, "ethereum", {
      request: () => {
        counters.evm += 1
        return Promise.reject(new Error("wallet request is disabled in recovery E2E"))
      },
    })
    Reflect.set(window, "ic", {
      plug: {
        requestConnect: () => {
          counters.ic += 1
          return Promise.reject(new Error("wallet request is disabled in recovery E2E"))
        },
      },
    })
  })
}

async function walletCallCounters(page: Page): Promise<{ evm: number; ic: number }> {
  return page.evaluate(
    () => Reflect.get(window, "__kinicRecoveryWalletCalls") as { evm: number; ic: number },
  )
}
