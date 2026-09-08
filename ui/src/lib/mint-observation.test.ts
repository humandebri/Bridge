import { beforeEach, afterEach, describe, expect, it, vi } from "vitest"
import { encodeAbiParameters, encodeEventTopics, type Hex } from "viem"
import { bridgeAbi } from "@/generated/abi/bridge.generated"
import { observeMint, clearMintObservations } from "./mint-observation"
import { clearBaseObservationCache } from "./base-transaction-observation"

const mocks = vi.hoisted(() => ({ receipt: vi.fn(), block: vi.fn(), notify: vi.fn() }))
vi.mock("@/config/profile", () => ({
  deploymentProfile: {
    chainId: 8453,
    bridgeAddress: `0x${"44".repeat(20)}`,
    deploymentInstanceId: "test",
    bridgeCanisterId: "aaaaa-aa",
    icHost: "https://icp-api.io",
  },
}))
vi.mock("@/lib/evm/client", () => ({
  basePublicClient: { getTransactionReceipt: mocks.receipt, getBlock: mocks.block },
}))
vi.mock("@/lib/ic/bridge", () => ({
  createBridgeActor: async () => ({ notify_deposit_mint: mocks.notify }),
}))
vi.mock("@/lib/ic/withdrawal-notification-client", () => ({
  getWithdrawalNotificationIdentity: async () => ({}),
}))
vi.mock("@/lib/browser-lock", () => ({
  withBrowserLock: async (_name: string, action: () => Promise<unknown>) => action(),
}))
const pending = {
  depositId: `0x${"11".repeat(32)}` as Hex,
  transactionHash: `0x${"22".repeat(32)}` as Hex,
  authorizationDigest: `0x${"33".repeat(32)}` as Hex,
  recipient: `0x${"55".repeat(20)}` as Hex,
  grossAmount: "100",
  chargedServiceFee: "10",
  mintedAmount: "90",
}
const blockHash = `0x${"66".repeat(32)}` as Hex
function receipt() {
  return {
    status: "success",
    blockNumber: 42n,
    blockHash,
    logs: [
      {
        address: `0x${"44".repeat(20)}`,
        topics: encodeEventTopics({
          abi: bridgeAbi,
          eventName: "DepositMinted",
          args: {
            depositId: pending.depositId,
            recipient: pending.recipient,
            authorizationDigest: pending.authorizationDigest,
          },
        }),
        data: encodeAbiParameters(
          [{ type: "uint256" }, { type: "uint256" }, { type: "uint256" }],
          [100n, 10n, 90n],
        ),
      },
    ],
  }
}
beforeEach(() => {
  vi.useFakeTimers()
  clearBaseObservationCache()
  clearMintObservations()
  vi.resetAllMocks()
  mocks.receipt.mockResolvedValue(receipt())
  mocks.block.mockResolvedValue({ number: 41n, hash: blockHash, timestamp: 1000n })
  mocks.notify.mockResolvedValue({
    Ok: { Recorded: { deposit_id: new Uint8Array(32).fill(0x11) } },
  })
})
afterEach(() => vi.useRealTimers())
function deferred<T>() {
  let resolve!: (value: T) => void
  const promise = new Promise<T>((done) => {
    resolve = done
  })
  return { promise, resolve }
}
const flush = () => vi.advanceTimersByTimeAsync(0)
const advancePoll = () => vi.advanceTimersByTimeAsync(10_000)
const finalizedBlock = { number: 42n, hash: blockHash, timestamp: 1000n }

describe("individual mint observation", () => {
  it("shows_success_before_finality_without_notifying_the_canister", async () => {
    const head = deferred<typeof finalizedBlock>()
    mocks.block.mockReturnValueOnce(head.promise)
    // A never-resolved finality request must not hold the receipt result open.
    const first = await observeMint(pending)
    expect(first).toMatchObject({ status: "success", finalized: false, recorded: false })
    expect(mocks.notify).not.toHaveBeenCalled()
    await advancePoll()
    expect(await observeMint(pending)).toMatchObject({ status: "success", finalized: false })
    expect(mocks.block).toHaveBeenCalledOnce()
    const notification = deferred<unknown>()
    mocks.notify.mockReturnValueOnce(notification.promise)
    mocks.block.mockResolvedValue(finalizedBlock)
    head.resolve(finalizedBlock)
    await flush()
    expect(mocks.notify).toHaveBeenCalledOnce()
    expect(first).toMatchObject({ finalized: false, recorded: false })
    await advancePoll()
    const finalized = await observeMint(pending)
    expect(finalized).toMatchObject({ status: "success", finalized: true, recorded: false })
    notification.resolve({ Ok: { Recorded: { deposit_id: new Uint8Array(32).fill(0x11) } } })
    await flush()
    expect(finalized.recorded).toBe(false)
    await advancePoll()
    expect(await observeMint(pending)).toMatchObject({ finalized: true, recorded: true })
    await flush()
  })
  it("preserves_success_when_transport_or_notification_fails", async () => {
    await observeMint(pending)
    await flush()
    await advancePoll()
    mocks.receipt.mockRejectedValue({ status: 403 })
    expect(await observeMint(pending)).toMatchObject({ status: "success", unavailable: true })
    clearBaseObservationCache()
    mocks.receipt.mockResolvedValue(receipt())
    mocks.block.mockRejectedValue({ status: 429 })
    expect(await observeMint(pending)).toMatchObject({ status: "success" })
    await flush()
    await advancePoll()
    expect(await observeMint(pending)).toMatchObject({ status: "success", unavailable: true })
    await flush()
    clearBaseObservationCache()
    mocks.block.mockResolvedValue(finalizedBlock)
    mocks.notify.mockResolvedValue({ Err: { RpcUnavailable: null } })
    const initial = await observeMint(pending)
    await flush()
    expect(initial.notificationError).toBeUndefined()
    await advancePoll()
    const observed = await observeMint(pending)
    expect(observed).toMatchObject({
      status: "success",
      finalized: true,
      recorded: false,
      notificationError: "RpcUnavailable",
    })
    await flush()
  })
  it("records_only_the_exact_canonical_finalized_mint", async () => {
    mocks.block.mockResolvedValue(finalizedBlock)
    const initial = await Promise.all([
      observeMint(pending),
      observeMint(pending),
      observeMint(pending),
    ])
    expect(initial.every((observed) => observed.status === "success")).toBe(true)
    await flush()
    expect(mocks.receipt).toHaveBeenCalledOnce()
    expect(mocks.block).toHaveBeenCalledTimes(2)
    expect(mocks.notify).toHaveBeenCalledOnce()
    expect(initial[0]).toMatchObject({ finalized: false, recorded: false })
    await advancePoll()
    expect(await observeMint(pending)).toMatchObject({
      status: "success",
      finalized: true,
      recorded: true,
    })
    await flush()
    expect(mocks.notify).toHaveBeenCalledOnce()
  })
  it("rejects_payload_mismatch_and_revises_success_after_a_reorg", async () => {
    mocks.receipt.mockResolvedValue({ ...receipt(), logs: [] })
    expect(await observeMint(pending)).toMatchObject({ status: "conflict", recorded: false })
    await flush()
    clearBaseObservationCache()
    mocks.receipt.mockResolvedValue(receipt())
    expect(await observeMint(pending)).toMatchObject({ status: "success" })
    await flush()
    await advancePoll()
    mocks.block.mockResolvedValue({ ...finalizedBlock, hash: `0x${"77".repeat(32)}` })
    await observeMint(pending)
    await flush()
    await advancePoll()
    expect(await observeMint(pending)).toMatchObject({
      status: "submitted",
      finalized: false,
      recorded: false,
    })
    await flush()
    await advancePoll()
    expect(await observeMint(pending)).toMatchObject({ status: "submitted" })
    await flush()
    expect(mocks.notify).not.toHaveBeenCalled()

    clearBaseObservationCache()
    clearMintObservations()
    mocks.block.mockResolvedValue(finalizedBlock)
    const delayedNotification = deferred<unknown>()
    mocks.notify.mockReturnValueOnce(delayedNotification.promise)
    await observeMint(pending)
    await flush()
    expect(mocks.notify).toHaveBeenCalledOnce()
    await advancePoll()
    mocks.block.mockResolvedValue({ ...finalizedBlock, hash: `0x${"77".repeat(32)}` })
    expect(await observeMint(pending)).toMatchObject({ status: "success", finalized: true })
    await flush()
    // Notification remains unresolved while canonical observation still advances.
    await advancePoll()
    expect(await observeMint(pending)).toMatchObject({ status: "submitted", recorded: false })
    await flush()
    delayedNotification.resolve({ Ok: { Recorded: { deposit_id: new Uint8Array(32).fill(0x11) } } })
    await flush()
    await advancePoll()
    expect(await observeMint(pending)).toMatchObject({
      status: "submitted",
      finalized: false,
      recorded: false,
    })
    await flush()
    expect(mocks.notify).toHaveBeenCalledOnce()

    // Results from an old receipt must not overwrite a disappearance, mismatch,
    // or replacement receipt, nor start a notification for the old evidence.
    for (const replacement of ["missing", "mismatch", "new-block"] as const) {
      clearBaseObservationCache()
      clearMintObservations()
      mocks.notify.mockClear()
      mocks.receipt.mockResolvedValue(receipt())
      const oldCanonical = deferred<typeof finalizedBlock>()
      mocks.block.mockImplementation(({ blockTag }) =>
        blockTag === "finalized" ? Promise.resolve(finalizedBlock) : oldCanonical.promise,
      )
      await observeMint(pending)
      await flush()
      await advancePoll()
      if (replacement === "missing") {
        mocks.receipt.mockRejectedValue({ name: "TransactionReceiptNotFoundError" })
      } else {
        mocks.receipt.mockResolvedValue(
          replacement === "mismatch"
            ? { ...receipt(), logs: [] }
            : { ...receipt(), blockNumber: 43n, blockHash: `0x${"88".repeat(32)}` },
        )
      }
      const replaced = await observeMint(pending)
      oldCanonical.resolve(finalizedBlock)
      await flush()
      expect(mocks.notify).not.toHaveBeenCalled()
      await advancePoll()
      const current = await observeMint(pending)
      expect(current.status).toBe(
        replacement === "missing"
          ? "submitted"
          : replacement === "mismatch"
            ? "conflict"
            : "success",
      )
      expect(current.recorded).toBe(false)
      expect(current.blockNumber).toBe(replaced.blockNumber)
      await flush()
    }
  })
})
