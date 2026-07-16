import { describe, expect, it } from "vitest"
import { deploymentProfile } from "@/config/profile"
import { basePublicClient, createBasePublicClient, createProfileChain, profileChain } from "./client"

describe("Base clients", () => {
  it("uses the deployment profile for the default client", () => {
    expect(profileChain.id).toBe(deploymentProfile.chainId)
    expect(basePublicClient.chain?.id).toBe(deploymentProfile.chainId)
  })

  it("creates an isolated chain and client for an arbitrary profile", () => {
    const custom = { ...deploymentProfile, chainId: 31_337, label: "Local Base", baseRpcUrl: "http://127.0.0.1:8545" }
    expect(createProfileChain(custom).rpcUrls.default.http).toEqual([custom.baseRpcUrl])
    expect(createBasePublicClient(custom).chain?.id).toBe(31_337)
  })
})
