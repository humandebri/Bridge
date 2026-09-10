import { beforeEach, afterEach, describe, expect, it, vi } from "vitest"
import { encodeAbiParameters, encodeEventTopics, type Hex } from "viem"
import type * as EvmClientModule from "./evm/client"
import type * as RecoveryModule from "./mint-recovery"
import type * as PendingModule from "./pending-confirmations"
import type * as ObservationModule from "./base-transaction-observation"
import type { DepositView } from "@/generated/bridge.did"
import { bridgeAbi } from "@/generated/abi/bridge.generated"

const mocks = vi.hoisted(() => ({
  get: vi.fn(),
  list: vi.fn(),
  logs: vi.fn(),
  backupLogs: vi.fn(),
  block: vi.fn(),
  receipt: vi.fn(),
  notify: vi.fn(),
  fetch: vi.fn(),
}))
vi.mock("@/config/profile", () => ({
  canonicalRpcUrl: (url: string) => url,
  resolvedBaseRpcUrl: () => "https://example.com",
  deploymentProfile: {
    chainId: 8453,
    bridgeAddress: `0x${"44".repeat(20)}`,
    deploymentInstanceId: `0x${"88".repeat(32)}`,
    mintRecoveryUrl: "https://recovery.bridge.kinic.xyz/v1/mint-recovery",
    bridgeCanisterId: "aaaaa-aa",
    icHost: "https://icp-api.io",
    deploymentBlock: 1n,
  },
}))
vi.mock("@/lib/evm/client", async (importOriginal) => ({
  firstSuccessfulHistoryClient: (await importOriginal<typeof EvmClientModule>())
    .firstSuccessfulHistoryClient,
  basePublicClient: { getBlock: mocks.block, getTransactionReceipt: mocks.receipt },
  baseHistoryClients: [
    { getBlock: mocks.block, getTransactionReceipt: mocks.receipt, getContractEvents: mocks.logs },
    {
      getBlock: mocks.block,
      getTransactionReceipt: mocks.receipt,
      getContractEvents: mocks.backupLogs,
    },
  ],
}))
vi.mock("@/lib/ic/bridge", () => ({
  createBridgeActor: async () => ({
    get_deposit: mocks.get,
    list_deposit_ids: mocks.list,
    notify_deposit_mint: mocks.notify,
  }),
}))
vi.mock("@/lib/ic/withdrawal-notification-client", () => ({
  getWithdrawalNotificationIdentity: async () => ({}),
}))

const depositId = `0x${"11".repeat(32)}` as Hex
const digest = `0x${"33".repeat(32)}` as Hex
const transactionHash = `0x${"22".repeat(32)}` as Hex
const blockHash = `0x${"66".repeat(32)}` as Hex
const address = `0x${"44".repeat(20)}` as Hex
const recipient = `0x${"55".repeat(20)}` as Hex
const args = {
  depositId,
  authorizationDigest: digest,
  recipient,
  grossAmount: 100n,
  serviceFee: 10n,
  mintedAmount: 90n,
}
const record = {
  deposit_id: new Uint8Array(32).fill(0x11),
  state: { AuthorizationAvailable: null },
  mint_receipt: [],
  mint_authorization: [
    {
      signature: [new Uint8Array(65)],
      digest: new Uint8Array(32).fill(0x33),
      recipient: new Uint8Array(20).fill(0x55),
      gross_amount: 100n,
      charged_service_fee: 10n,
      finalized_block_number: 1n,
      deadline: 2000n,
    },
  ],
} as unknown as DepositView
function receipt() {
  return {
    status: "success",
    blockNumber: 42n,
    blockHash,
    logs: [
      {
        address,
        topics: encodeEventTopics({ abi: bridgeAbi, eventName: "DepositMinted", args }),
        data: encodeAbiParameters(
          [{ type: "uint256" }, { type: "uint256" }, { type: "uint256" }],
          [100n, 10n, 90n],
        ),
      },
    ],
  }
}
let recovery: typeof RecoveryModule
let pending: typeof PendingModule
let observation: typeof ObservationModule
beforeEach(async () => {
  vi.resetModules()
  vi.clearAllMocks()
  window.localStorage.clear()
  mocks.fetch.mockImplementation(async () =>
    Response.json({
      deploymentInstanceId: `0x${"88".repeat(32)}`,
      depositId,
      authorizationDigest: digest,
      hashes: [transactionHash],
      cursor: null,
    }),
  )
  vi.stubGlobal("fetch", mocks.fetch)
  vi.stubGlobal("navigator", {
    locks: { request: async (_: string, _opts: object, run: () => unknown) => run() },
  })
  mocks.get.mockResolvedValue([record])
  mocks.list.mockResolvedValue({ Ok: { deposit_ids: [], next_cursor: [] } })
  mocks.logs.mockResolvedValue([])
  mocks.backupLogs.mockRejectedValue(new Error("Backup unavailable"))
  mocks.receipt.mockResolvedValue(receipt())
  mocks.block.mockResolvedValue({ number: 100n, hash: blockHash, timestamp: 3000n })
  mocks.notify.mockResolvedValue({ Ok: { Recorded: { deposit_id: record.deposit_id } } })
  recovery = await import("./mint-recovery")
  pending = await import("./pending-confirmations")
  observation = await import("./base-transaction-observation")
})
afterEach(() => {
  vi.unstubAllGlobals()
  vi.useRealTimers()
})
const flush = () => new Promise((resolve) => setTimeout(resolve, 0))

describe("mint recovery", () => {
  it("recovers_expired_mint_after_reload_and_records_exact_finalized_success", async () => {
    await recovery.rememberMintRecovery(record, "aaaaa-aa", true)
    vi.resetModules()
    recovery = await import("./mint-recovery")
    pending = await import("./pending-confirmations")
    expect(recovery.wasMintRequested(record)).toBe(true)
    await recovery.recoverMint(recovery.readMintRecoveryTargets()[0]!)
    expect(pending.readAllPendingMints()[0]?.transactionHash).toBe(transactionHash)
    await flush()
    expect(mocks.notify).toHaveBeenCalledWith({
      deposit_id: record.deposit_id,
      transaction_hash: new Uint8Array(32).fill(0x22),
    })
    expect(mocks.logs).not.toHaveBeenCalled()
  })
  it("continues_past_twenty_deposits_without_browser_records", async () => {
    mocks.list
      .mockResolvedValueOnce({
        Ok: {
          deposit_ids: Array.from({ length: 20 }, (_, i) => new Uint8Array(32).fill(0x80 + i)),
          next_cursor: [20n],
        },
      })
      .mockResolvedValueOnce({ Ok: { deposit_ids: [record.deposit_id], next_cursor: [] } })
    mocks.get.mockImplementation(async (id: Uint8Array) => [
      id[0] === 0x11 ? record : { ...record, mint_receipt: [{}] },
    ])
    const cursor = await recovery.discoverMintRecovery("aaaaa-aa")
    expect(recovery.readMintRecoveryTargets()).toHaveLength(0)
    await recovery.discoverMintRecovery("aaaaa-aa", cursor)
    expect(recovery.readMintRecoveryTargets()).toHaveLength(1)
  })
  it("recovery_rechecks_empty_history_after_sixty_seconds", async () => {
    vi.useFakeTimers()
    mocks.fetch.mockImplementation(async () =>
      Response.json({
        deploymentInstanceId: `0x${"88".repeat(32)}`,
        depositId,
        authorizationDigest: digest,
        hashes: [],
        cursor: null,
      }),
    )
    await recovery.rememberMintRecovery(record)
    const target = recovery.readMintRecoveryTargets()[0]!
    await recovery.recoverMint(target)
    vi.resetModules()
    recovery = await import("./mint-recovery")
    await recovery.recoverMint(target)
    expect(mocks.fetch).toHaveBeenCalledOnce()
    vi.advanceTimersByTime(60001)
    await recovery.recoverMint(target)
    expect(mocks.fetch).toHaveBeenCalledTimes(2)
  })
  it("recovery_caps_candidates_and_preserves_pagination", async () => {
    const hashes = Array.from(
      { length: 5 },
      (_, n) => `0x${(n + 1).toString(16).padStart(64, "0")}`,
    )
    mocks.fetch.mockImplementation(async () =>
      Response.json({
        deploymentInstanceId: `0x${"88".repeat(32)}`,
        depositId,
        authorizationDigest: digest,
        hashes,
        cursor: "page-two",
      }),
    )
    mocks.receipt.mockResolvedValue({ ...receipt(), logs: [] })
    await recovery.rememberMintRecovery(record)
    const target = recovery.readMintRecoveryTargets()[0]!
    await recovery.recoverMint(target)
    expect(mocks.receipt).toHaveBeenCalledTimes(4)
    vi.resetModules()
    recovery = await import("./mint-recovery")
    await recovery.recoverMint(target)
    expect(mocks.receipt).toHaveBeenCalledTimes(5)
    expect(mocks.fetch).toHaveBeenCalledOnce()
    await recovery.recoverMint(target)
    expect(JSON.parse(mocks.fetch.mock.calls[1]![1].body).cursor).toBe("page-two")
    expect(mocks.receipt).toHaveBeenCalledTimes(5)
    expect(mocks.notify).not.toHaveBeenCalled()
  })
  it("recovery_backs_off_shared_cycles_on_service_failure", async () => {
    vi.useFakeTimers()
    mocks.fetch.mockResolvedValue(new Response(null, { status: 429 }))
    await recovery.rememberMintRecovery(record)
    expect((await recovery.runMintRecoveryCycle())?.observation.unavailable).toBe(true)
    vi.advanceTimersByTime(10001)
    await recovery.runMintRecoveryCycle()
    expect(mocks.fetch).toHaveBeenCalledOnce()
    expect(mocks.notify).not.toHaveBeenCalled()
  })
  it("uses_receipts_instead_of_event_search_when_hash_is_saved", async () => {
    await pending.savePendingMint({
      depositId,
      authorizationDigest: digest,
      recipient,
      grossAmount: "100",
      chargedServiceFee: "10",
      mintedAmount: "90",
      transactionHash,
    })
    await recovery.runMintRecoveryCycle()
    expect(mocks.fetch).not.toHaveBeenCalled()
    const { deploymentProfile } = await import("@/config/profile")
    deploymentProfile.chainId = 84532
    try {
      await recovery.rememberMintRecovery(record)
      await recovery.recoverMint(recovery.readMintRecoveryTargets()[0]!)
      expect(mocks.fetch).not.toHaveBeenCalled()
    } finally {
      deploymentProfile.chainId = 8453
    }
    expect(mocks.receipt).toHaveBeenCalled()
    await flush()
  })
  it("recovery_rejects_mismatched_same_deposit_receipt", async () => {
    await recovery.rememberMintRecovery(record)
    const bad = receipt()
    bad.logs[0]!.data = encodeAbiParameters(
      [{ type: "uint256" }, { type: "uint256" }, { type: "uint256" }],
      [100n, 10n, 91n],
    )
    mocks.receipt.mockResolvedValue(bad)
    expect((await recovery.recoverMint(recovery.readMintRecoveryTargets()[0]!)).status).toBe(
      "conflict",
    )
    expect(pending.readAllPendingMints()).toHaveLength(0)
    expect(mocks.notify).not.toHaveBeenCalled()
  })
  it("recovery_preserves_noncanonical_and_unfinalized_candidates", async () => {
    await recovery.rememberMintRecovery(record)
    const target = recovery.readMintRecoveryTargets()[0]!
    mocks.block.mockResolvedValue({ number: 20n, hash: blockHash, timestamp: 3000n })
    await expect(recovery.recoverMint(target)).rejects.toThrow("canonical finality")
    observation.clearBaseObservationCache()
    mocks.block.mockResolvedValue({ number: 100n, hash: `0x${"77".repeat(32)}`, timestamp: 3000n })
    await expect(recovery.recoverMint(target)).rejects.toThrow("canonical finality")
    expect(pending.readAllPendingMints()).toHaveLength(0)
  })
  it("recovery_restarts_expired_cursor_and_rejects_foreign_response", async () => {
    await recovery.rememberMintRecovery(record)
    const target = recovery.readMintRecoveryTargets()[0]!
    mocks.fetch.mockResolvedValueOnce(new Response(null, { status: 410 }))
    await recovery.recoverMint(target)
    vi.useFakeTimers()
    vi.advanceTimersByTime(10001)
    mocks.fetch.mockResolvedValueOnce(
      Response.json({
        deploymentInstanceId: `0x${"99".repeat(32)}`,
        depositId,
        authorizationDigest: digest,
        hashes: [transactionHash],
        cursor: null,
      }),
    )
    await expect(recovery.recoverMint(target)).rejects.toThrow("binding mismatch")
    expect(mocks.receipt).not.toHaveBeenCalled()
  })
})
