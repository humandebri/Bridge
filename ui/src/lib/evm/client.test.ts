import { describe, expect, it } from "vitest"
import { deploymentProfile } from "@/config/profile"
import { basePublicClient, baseTransactionExplorerUrl, createBaseHistoryClients, createBasePublicClient, createProfileChain, firstSuccessfulHistoryClient, profileChain, wagmiConfig } from "./client"

describe("Base clients", () => {
  it("uses the deployment profile for the default client", () => {
    expect(profileChain.id).toBe(deploymentProfile.chainId)
    expect(basePublicClient.chain?.id).toBe(deploymentProfile.chainId)
  })

  it("offers the Coinbase Wallet connector", () => {
    expect(wagmiConfig.connectors.some((connector) => connector.id === "coinbaseWalletSDK")).toBe(true)
  })

  it("builds Base explorer transaction URLs only for supported chains and hashes", () => {
    const hash = `0x${"ab".repeat(32)}` as const
    expect(baseTransactionExplorerUrl(8453, hash)).toBe(`https://basescan.org/tx/${hash}`)
    expect(baseTransactionExplorerUrl(84532, hash)).toBe(`https://sepolia.basescan.org/tx/${hash}`)
    expect(baseTransactionExplorerUrl(31_337, hash)).toBeUndefined()
    expect(baseTransactionExplorerUrl(84532, "0x12")).toBeUndefined()
  })

  it("creates an isolated chain and client for an arbitrary profile", () => {
    const custom = { ...deploymentProfile, chainId: 31_337, label: "Local Base", baseRpcUrl: "http://127.0.0.1:8545" }
    expect(createProfileChain(custom).rpcUrls.default.http).toEqual([custom.baseRpcUrl])
    expect(createBasePublicClient(custom).chain?.id).toBe(31_337)
  })

  it("creates one history client for each reviewed RPC URL", () => {
    const clients = createBaseHistoryClients({
      ...deploymentProfile,
      baseHistoryRpcUrls: ["https://history-one.example", "https://history-two.example"],
    })
    expect(clients).toHaveLength(2)
  })

  it("retries the whole history operation on the next client", async () => {
    const attempts: string[] = []
    const result = await firstSuccessfulHistoryClient(["first", "second"], (client) => {
      attempts.push(client)
      if (client === "first") return Promise.reject(new Error("archive request rejected"))
      return Promise.resolve([] as string[])
    })
    expect(result).toEqual([])
    expect(attempts).toEqual(["first", "second"])
  })

  it("does not try another history client after a successful empty result", async () => {
    const attempts: string[] = []
    await firstSuccessfulHistoryClient(["first", "second"], (client) => {
      attempts.push(client)
      return Promise.resolve([])
    })
    expect(attempts).toEqual(["first"])
  })

  it("does not convert total history RPC failure into an empty history", async () => {
    await expect(firstSuccessfulHistoryClient(["first", "second"], () => Promise.reject(new Error("unavailable"))))
      .rejects.toThrow("Base history RPCs are unavailable")
  })
})
