import { describe, expect, it, vi } from "vitest"
import { deploymentProfile } from "@/config/profile"
import type { baseHistoryClients } from "./client"
import {
  basePublicClient,
  baseTransactionExplorerUrl,
  createBaseHistoryClients,
  createBasePublicClient,
  createProfileChain,
  firstSuccessfulHistoryClient,
  hasIndependentFinalizedRevertQuorum,
  profileChain,
  wagmiConfig,
  withHistoryClientFailover,
} from "./client"

describe("Base clients", () => {
  it("uses the deployment profile for the default client", () => {
    expect(profileChain.id).toBe(deploymentProfile.chainId)
    expect(basePublicClient.chain?.id).toBe(deploymentProfile.chainId)
  })

  it("offers injected and Coinbase connectors without the MetaMask SDK connector", () => {
    expect(wagmiConfig.connectors.some((connector) => connector.id === "injected")).toBe(true)
    expect(wagmiConfig.connectors.some((connector) => connector.id === "coinbaseWalletSDK")).toBe(
      true,
    )
    expect(wagmiConfig.connectors.some((connector) => connector.id === "metaMaskSDK")).toBe(false)
  })

  it("builds Base explorer transaction URLs only for supported chains and hashes", () => {
    const hash = `0x${"ab".repeat(32)}` as const
    expect(baseTransactionExplorerUrl(8453, hash)).toBe(`https://basescan.org/tx/${hash}`)
    expect(baseTransactionExplorerUrl(84532, hash)).toBe(`https://sepolia.basescan.org/tx/${hash}`)
    expect(baseTransactionExplorerUrl(31_337, hash)).toBeUndefined()
    expect(baseTransactionExplorerUrl(84532, "0x12")).toBeUndefined()
  })

  it("creates an isolated chain and client for an arbitrary profile", () => {
    const custom = {
      ...deploymentProfile,
      chainId: 31_337,
      label: "Local Base",
      baseRpcUrl: "http://127.0.0.1:8545",
    }
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

  it("does not create two quorum votes for a duplicated RPC URL", async () => {
    const clients = createBaseHistoryClients({
      ...deploymentProfile,
      baseHistoryRpcUrls: ["https://history.example", "https://history.example/"],
    })
    expect(clients).toHaveLength(1)
    await expect(
      hasIndependentFinalizedRevertQuorum(`0x${"12".repeat(32)}`, clients),
    ).resolves.toBe(false)
  })

  it("accepts matching finalized revert evidence from two distinct clients", async () => {
    const blockHash = `0x${"34".repeat(32)}` as const
    const client = () =>
      ({
        getTransactionReceipt: vi.fn().mockResolvedValue({
          status: "reverted",
          blockNumber: 42n,
          blockHash,
        }),
        getBlock: vi
          .fn()
          .mockImplementation((args: { blockTag?: string }) =>
            Promise.resolve(
              args.blockTag === "finalized"
                ? { number: 43n, hash: `0x${"56".repeat(32)}` }
                : { number: 42n, hash: blockHash },
            ),
          ),
      }) as unknown as (typeof baseHistoryClients)[number]

    await expect(
      hasIndependentFinalizedRevertQuorum(`0x${"12".repeat(32)}`, [client(), client()]),
    ).resolves.toBe(true)
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
    await expect(
      firstSuccessfulHistoryClient(["first", "second"], () =>
        Promise.reject(new Error("unavailable")),
      ),
    ).rejects.toThrow("Base history RPCs are unavailable")
  })

  it("fails over when a downstream history operation fails after the finalized head", async () => {
    const attempts: string[] = []
    const failed = new Set<number>()
    const result = await withHistoryClientFailover(["first", "second"], failed, (client) => {
      attempts.push(`finalized:${client}`)
      if (client === "first") return Promise.reject(new Error("checkpoint request rejected"))
      attempts.push(`logs:${client}`)
      return Promise.resolve("complete")
    })
    expect(result).toBe("complete")
    expect(attempts).toEqual(["finalized:first", "finalized:second", "logs:second"])
    expect([...failed]).toEqual([0])
  })

  it("retries all history providers after every provider has failed", async () => {
    const failed = new Set([0, 1])
    const attempts: string[] = []
    await withHistoryClientFailover(["first", "second"], failed, (client) => {
      attempts.push(client)
      return Promise.resolve("complete")
    })
    expect(attempts).toEqual(["first"])
    expect(failed.size).toBe(0)
  })
})
