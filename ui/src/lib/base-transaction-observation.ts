import { basePublicClient } from "@/lib/evm/client"
import { deploymentProfile } from "@/config/profile"
import type { Hex } from "viem"

type Entry = { promise: Promise<unknown>; until: number; failures: number }
const reads = new Map<string, Entry>()
const domain = `${deploymentProfile.chainId}:${deploymentProfile.bridgeAddress}:${deploymentProfile.deploymentInstanceId}:${deploymentProfile.bridgeCanisterId}:${deploymentProfile.icHost}`

export function rpcFailureKind(error: unknown): "restricted" | "temporary" {
  const seen = new Set<unknown>()
  for (let current = error; current && typeof current === "object" && !seen.has(current);) {
    seen.add(current)
    const value = current as {
      status?: number
      details?: string
      message?: string
      cause?: unknown
    }
    if (
      value.status === 401 ||
      value.status === 403 ||
      /personal token|not on whitelist|block range|method not supported/i.test(
        `${value.details ?? ""} ${value.message ?? ""}`,
      )
    )
      return "restricted"
    current = value.cause
  }
  return "temporary"
}

export function sharedBaseRead<T>(key: string, read: () => Promise<T>): Promise<T> {
  const fullKey = `${domain}:${key}`
  const previous = reads.get(fullKey)
  if (previous && previous.until > Date.now()) return previous.promise as Promise<T>
  const entry: Entry = {
    promise: Promise.resolve(),
    until: Infinity,
    failures: previous?.failures ?? 0,
  }
  // Keep pending requests and active backoff entries; retire old settled reads.
  if (reads.size >= 512) {
    for (const [cachedKey, cached] of reads) {
      if (cachedKey !== fullKey && cached.until < Date.now() - 300_000) reads.delete(cachedKey)
    }
  }
  entry.promise = Promise.resolve()
    .then(read)
    .then(
      (value) => {
        entry.failures = 0
        entry.until = Date.now() + 10_000
        return value
      },
      (error) => {
        entry.failures += 1
        entry.until =
          Date.now() +
          (rpcFailureKind(error) === "restricted"
            ? 300_000
            : Math.min(120_000, 10_000 * 2 ** Math.min(entry.failures - 1, 4)))
        throw error
      },
    )
  reads.set(fullKey, entry)
  return entry.promise as Promise<T>
}

export const readBaseReceipt = (hash: Hex) =>
  sharedBaseRead(`receipt:${hash.toLowerCase()}`, () =>
    basePublicClient.getTransactionReceipt({ hash }),
  )
export const readBaseBlock = (block: bigint | "latest" | "finalized") =>
  sharedBaseRead(`block:${block}`, () =>
    basePublicClient.getBlock(
      typeof block === "bigint" ? { blockNumber: block } : { blockTag: block },
    ),
  )

export function clearBaseObservationCache() {
  reads.clear()
}
