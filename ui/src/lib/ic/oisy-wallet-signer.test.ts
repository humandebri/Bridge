import { IcrcWallet } from "@dfinity/oisy-wallet-signer/icrc-wallet"
import { readFileSync } from "node:fs"
import { fileURLToPath } from "node:url"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"

const SIGNER_ORIGIN = "https://oisy.com"
const SIGNER_URL = `${SIGNER_ORIGIN}/sign`
const STATUS_METHOD = "icrc29_status"
const ACCOUNTS_METHOD = "icrc27_accounts"

interface JsonRpcRequest {
  jsonrpc: "2.0"
  id: string
  method: string
}

class FakeSignerPopup {
  closed = false
  requests: JsonRpcRequest[] = []
  private readyResponses = 1

  focus = vi.fn()
  close = vi.fn(() => { this.closed = true })

  postMessage = (message: unknown) => {
    const request = message as JsonRpcRequest
    this.requests.push(request)
    if (request.method === STATUS_METHOD && this.readyResponses > 0) {
      this.readyResponses -= 1
      queueMicrotask(() => this.respond(request, "ready"))
    }
  }

  latest(method: string): JsonRpcRequest {
    for (let index = this.requests.length - 1; index >= 0; index -= 1) {
      const request = this.requests[index]
      if (request?.method === method) return request
    }
    throw new Error(`Missing signer request for ${method}`)
  }

  respond(request: JsonRpcRequest, result: unknown, origin = SIGNER_ORIGIN): void {
    window.dispatchEvent(new MessageEvent("message", {
      data: { jsonrpc: "2.0", id: request.id, result },
      origin,
      source: this as unknown as Window,
    }))
  }
}

async function connect(popup: FakeSignerPopup): Promise<IcrcWallet> {
  vi.spyOn(window, "open").mockReturnValue(popup as unknown as Window)
  const wallet = IcrcWallet.connect({
    url: SIGNER_URL,
    connectionOptions: {
      pollingIntervalInMilliseconds: 10,
      timeoutInMilliseconds: 1_000,
    },
  })
  await vi.advanceTimersByTimeAsync(10)
  return wallet
}

describe("patched OISY signer status polling", () => {
  beforeEach(() => {
    vi.useFakeTimers()
  })

  afterEach(() => {
    vi.clearAllTimers()
    vi.useRealTimers()
    vi.restoreAllMocks()
  })

  it("ignores an accounts response received while a status probe is pending", async () => {
    const popup = new FakeSignerPopup()
    const wallet = await connect(popup)
    const firstAccounts = wallet.accounts({ options: { timeoutInMilliseconds: 20_000 } })

    await vi.advanceTimersByTimeAsync(5_000)
    const statusRequest = popup.latest(STATUS_METHOD)
    const accountsRequest = popup.latest(ACCOUNTS_METHOD)
    popup.respond(accountsRequest, { accounts: [{ owner: "aaaaa-aa" }] })

    await expect(firstAccounts).resolves.toEqual([{ owner: "aaaaa-aa" }])
    expect(popup.close).not.toHaveBeenCalled()

    popup.respond(statusRequest, "ready")
    const secondAccounts = wallet.accounts({ options: { timeoutInMilliseconds: 20_000 } })
    popup.respond(popup.latest(ACCOUNTS_METHOD), { accounts: [{ owner: "aaaaa-aa" }] })

    await expect(secondAccounts).resolves.toEqual([{ owner: "aaaaa-aa" }])
    await wallet.disconnect()
  })

  it("keeps an open popup connected when only the status probe times out", async () => {
    const popup = new FakeSignerPopup()
    const wallet = await connect(popup)

    await vi.advanceTimersByTimeAsync(10_001)
    expect(popup.close).not.toHaveBeenCalled()

    const accounts = wallet.accounts({ options: { timeoutInMilliseconds: 20_000 } })
    popup.respond(popup.latest(ACCOUNTS_METHOD), { accounts: [{ owner: "aaaaa-aa" }] })

    await expect(accounts).resolves.toEqual([{ owner: "aaaaa-aa" }])
    await wallet.disconnect()
  })

  it("still rejects a request after the signer popup is actually closed", async () => {
    const popup = new FakeSignerPopup()
    const wallet = await connect(popup)
    popup.closed = true

    await expect(wallet.accounts()).rejects.toThrow("The signer has been closed")
    await wallet.disconnect()

    expect(popup.close).toHaveBeenCalledOnce()
  })

  it("still disconnects when a status response has the wrong origin", async () => {
    const popup = new FakeSignerPopup()
    const wallet = await connect(popup)

    await vi.advanceTimersByTimeAsync(5_000)
    popup.respond(popup.latest(STATUS_METHOD), "ready", "https://attacker.example")
    await vi.advanceTimersByTimeAsync(1)

    expect(popup.close).toHaveBeenCalledOnce()
    await expect(wallet.accounts()).rejects.toThrow("The signer has been disconnected")
  })
})

describe("patched OISY signer certificate verification", () => {
  it("passes the ledger canister ID through the Core 5 principal option", () => {
    const signerRuntime = readFileSync(
      fileURLToPath(import.meta.resolve("@dfinity/oisy-wallet-signer/icrc-wallet")),
      "utf8",
    )

    expect(signerRuntime).toMatch(
      /rootKey:[^,]+,principal:\{canisterId:[^}]+\}/,
    )
    expect(signerRuntime).not.toMatch(/rootKey:[^,]+,canisterId:/)
  })
})
