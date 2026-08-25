import { beforeEach, describe, expect, it, vi } from "vitest"

const mocks = vi.hoisted(() => ({
  createSync: vi.fn(),
  fetchRootKey: vi.fn(),
  isLocal: vi.fn(),
}))

vi.mock("@icp-sdk/core/agent", () => ({
  HttpAgent: { createSync: mocks.createSync },
}))

import { createIcAgent } from "./agent"

describe("createIcAgent", () => {
  beforeEach(() => {
    vi.clearAllMocks()
    mocks.createSync.mockReturnValue({ isLocal: mocks.isLocal, fetchRootKey: mocks.fetchRootKey })
  })

  it("fetches the root key for a local replica", async () => {
    mocks.isLocal.mockReturnValue(true)
    await createIcAgent("http://127.0.0.1:4943")
    expect(mocks.fetchRootKey).toHaveBeenCalledOnce()
  })

  it("does not fetch the root key for mainnet", async () => {
    mocks.isLocal.mockReturnValue(false)
    await createIcAgent("https://icp-api.io")
    expect(mocks.fetchRootKey).not.toHaveBeenCalled()
  })

  it("passes a caller identity to the agent", async () => {
    const identity = { transformRequest: vi.fn(), getPrincipal: vi.fn() }
    mocks.isLocal.mockReturnValue(false)

    await createIcAgent("https://icp-api.io", identity)

    expect(mocks.createSync).toHaveBeenCalledWith({ host: "https://icp-api.io", identity })
  })
})
