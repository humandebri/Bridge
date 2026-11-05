/// <reference lib="dom" />
import { createRequire } from "node:module"
import { pathToFileURL } from "node:url"
import { afterEach, describe, expect, it, vi } from "vitest"

const localRequire = createRequire(import.meta.url)
const walletConnectRequire = createRequire(localRequire.resolve("@walletconnect/ethereum-provider"))
const appKitRequire = createRequire(walletConnectRequire.resolve("@reown/appkit/core"))
const walletEntry = pathToFileURL(appKitRequire.resolve("@reown/appkit-wallet"))
const { W3mFrame } = await import(
  /* @vite-ignore */ new URL("../src/W3mFrame.js", walletEntry).href
)
const { W3mFrameSchema } = await import(
  /* @vite-ignore */ new URL("../src/W3mFrameSchema.js", walletEntry).href
)

afterEach(() => {
  document.body.replaceChildren()
  vi.restoreAllMocks()
})

describe("Reown iframe messages", () => {
  it("accepts the configured iframe and rejects forged origins and sources", () => {
    const frame = new W3mFrame({ projectId: "test", isAppClient: true })
    frame.initFrame()
    const received = vi.fn()
    frame.events.onFrameEvent(received)
    const data = { type: "@w3m-frame/READY", payload: {} }
    const source = frame.iframe.contentWindow
    const origin = new URL(frame.iframe.src).origin

    window.dispatchEvent(
      new MessageEvent("message", { data, origin: "https://attacker.example", source }),
    )
    window.dispatchEvent(new MessageEvent("message", { data, origin, source: window }))
    expect(received).not.toHaveBeenCalled()

    window.dispatchEvent(new MessageEvent("message", { data, origin, source }))
    expect(received).toHaveBeenCalledTimes(1)
  })

  it("checks the origin and source before parsing one-shot responses", () => {
    const frame = new W3mFrame({ projectId: "test", isAppClient: true })
    frame.initFrame()
    const received = vi.fn()
    const controller = new AbortController()
    frame.events.registerFrameEventHandler("request-1", received, controller.signal)
    const data = { id: "request-1", type: "@w3m-frame/READY", payload: {} }
    const source = frame.iframe.contentWindow
    const origin = new URL(frame.iframe.src).origin

    window.dispatchEvent(new MessageEvent("message", { data, origin, source: window }))
    expect(received).not.toHaveBeenCalled()
    window.dispatchEvent(new MessageEvent("message", { data, origin, source }))
    expect(received).toHaveBeenCalledTimes(1)
    controller.abort()
  })

  it("does not parse oversized messages from an unrelated window", () => {
    const frame = new W3mFrame({ projectId: "test", isAppClient: true })
    frame.initFrame()
    const parse = vi.spyOn(W3mFrameSchema.frameEvent, "safeParse")
    const data = {
      type: "@w3m-frame/GET_USER_SUCCESS",
      payload: { address: "0xabc", chainId: 1, accounts: Array(2000).fill({ address: 1 }) },
    }

    window.dispatchEvent(
      new MessageEvent("message", {
        data,
        origin: new URL(frame.iframe.src).origin,
        source: window,
      }),
    )
    expect(parse).not.toHaveBeenCalled()
  })
})
