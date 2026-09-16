// @vitest-environment node
import { describe, expect, it, vi } from "vitest"
import {
  deploymentProfileBootstrap,
  parseProductionProfileBootstrap,
  productionProfileUrl,
  requireUnchangedProductionProfile,
} from "./production-profile-continuity.mjs"

const profile = JSON.stringify({ environment: "mainnet-candidate", canisterSchemaVersion: 36 })

/** @param {string} body @param {{ status?: number, headers?: Record<string, string> }} [overrides] */
function response(body, overrides = {}) {
  return new Response(body, {
    status: overrides.status ?? 200,
    headers: overrides.headers,
  })
}

/** @param {string} body @param {{ status?: number, headers?: Record<string, string>, redirected?: boolean, url?: string }} [overrides] */
function fixedResponse(body, overrides = {}) {
  const value = response(body, overrides)
  Object.defineProperties(value, {
    redirected: { value: overrides.redirected ?? false },
    url: { value: overrides.url ?? productionProfileUrl },
  })
  return value
}

describe("production deployment profile continuity", () => {
  it("accepts only the exact generated bootstrap and profile bytes", async () => {
    const script = deploymentProfileBootstrap(profile)
    expect(parseProductionProfileBootstrap(script)).toBe(profile)
    const fetchImpl = vi.fn().mockResolvedValue(fixedResponse(script))
    await expect(requireUnchangedProductionProfile(profile, fetchImpl)).resolves.toBeUndefined()
    expect(fetchImpl).toHaveBeenCalledWith(productionProfileUrl, {
      headers: {
        accept: "application/javascript",
        "cache-control": "no-cache",
        pragma: "no-cache",
      },
      redirect: "manual",
    })
  })

  it.each([
    `${deploymentProfileBootstrap(profile)}alert(1)`,
    `window.__KINIC_DEPLOYMENT_PROFILE_JSON__ = ${JSON.stringify(profile)};\n`,
    `globalThis.__KINIC_DEPLOYMENT_PROFILE_JSON__ = {};\n`,
  ])("rejects an unexpected bootstrap shape", (script) => {
    expect(() => parseProductionProfileBootstrap(script)).toThrow()
  })

  it("rejects profile drift", async () => {
    const fetchImpl = vi
      .fn()
      .mockResolvedValue(fixedResponse(deploymentProfileBootstrap(JSON.stringify({ drift: true }))))
    await expect(requireUnchangedProductionProfile(profile, fetchImpl)).rejects.toThrow(
      "cannot change",
    )
  })

  it.each([
    fixedResponse("", { status: 302 }),
    fixedResponse(deploymentProfileBootstrap(profile), {
      redirected: true,
      url: "https://example.com/deployment-profile.js",
    }),
    fixedResponse(deploymentProfileBootstrap(profile), {
      url: "https://example.com/deployment-profile.js",
    }),
  ])("rejects redirects and the wrong origin", async (badResponse) => {
    await expect(
      requireUnchangedProductionProfile(profile, vi.fn().mockResolvedValue(badResponse)),
    ).rejects.toThrow("without a redirect")
  })

  it("rejects an oversized bootstrap", async () => {
    const fetchImpl = vi.fn().mockResolvedValue(
      fixedResponse(deploymentProfileBootstrap(profile), {
        headers: { "content-length": String(256 * 1024 + 1) },
      }),
    )
    await expect(requireUnchangedProductionProfile(profile, fetchImpl)).rejects.toThrow("too large")
  })
})
