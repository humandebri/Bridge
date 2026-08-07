import { Principal } from "@dfinity/principal"
import { beforeEach, describe, expect, it, vi } from "vitest"
import type { DeploymentProfile } from "@/config/profile"

const mocks = vi.hoisted(() => ({
  createBridgeActor: vi.fn(),
  notifyWithdrawal: vi.fn(),
}))

vi.mock("@/lib/ic/bridge", () => ({ createBridgeActor: mocks.createBridgeActor }))
vi.mock("@/config/profile", () => ({
  deploymentProfile: {
    chainId: 84_532,
    bridgeCanisterId: "aaaaa-aa",
    deploymentInstanceId: `0x${"11".repeat(32)}`,
    icHost: "https://ic.example",
  },
}))

import {
  getWithdrawalNotificationIdentity,
  NotifyWithdrawalCallError,
  notifyWithdrawalErrorMessage,
  notifyWithdrawalWithBrowserIdentity,
  unwrapNotifyWithdrawalResult,
  withdrawalNotificationIdentityStorageKey,
} from "./withdrawal-notification-client"
import { browserLocalStorage } from "@/lib/browser-lock"

function profile(instanceByte: string): Pick<DeploymentProfile, "chainId" | "bridgeCanisterId" | "deploymentInstanceId" | "icHost"> {
  return {
    chainId: 84_532,
    bridgeCanisterId: "aaaaa-aa",
    deploymentInstanceId: `0x${instanceByte.repeat(64)}`,
    icHost: "https://ic.example",
  }
}

beforeEach(() => {
  vi.clearAllMocks()
  browserLocalStorage().clear()
  mocks.createBridgeActor.mockResolvedValue({ notify_withdrawal: mocks.notifyWithdrawal })
  mocks.notifyWithdrawal.mockResolvedValue({
    Ok: { Ingested: { finalized_head_block_number: 42n, withdrawal_id: new Uint8Array(32).fill(7) } },
  })
})

describe("withdrawal notification identity", () => {
  it("persists_and_reuses_one_non_anonymous_identity_for_a_deployment", async () => {
    const deployment = profile("1")
    const first = await getWithdrawalNotificationIdentity(deployment)
    const storageKey = withdrawalNotificationIdentityStorageKey(deployment)
    expect(browserLocalStorage().getItem(storageKey)).not.toBeNull()

    vi.resetModules()
    const reloadedClient = await import("./withdrawal-notification-client")
    const second = await reloadedClient.getWithdrawalNotificationIdentity(deployment)

    expect(first.getPrincipal().toText()).toBe(second.getPrincipal().toText())
    expect(first.getPrincipal().toText()).not.toBe(Principal.anonymous().toText())
  })

  it("uses distinct identities for distinct deployment instances", async () => {
    const first = await getWithdrawalNotificationIdentity(profile("2"))
    const second = await getWithdrawalNotificationIdentity(profile("3"))

    expect(first.getPrincipal().toText()).not.toBe(second.getPrincipal().toText())
  })

  it("replaces a malformed persisted identity", async () => {
    const deployment = profile("4")
    const key = withdrawalNotificationIdentityStorageKey(deployment)
    browserLocalStorage().setItem(key, "malformed")

    const identity = await getWithdrawalNotificationIdentity(deployment)

    expect(identity.getPrincipal().toText()).not.toBe(Principal.anonymous().toText())
    expect(browserLocalStorage().getItem(key)).not.toBe("malformed")
  })

  it("keeps a session identity when browser storage is unavailable", async () => {
    const deployment = profile("5")
    const getItem = vi.spyOn(Storage.prototype, "getItem").mockImplementation(() => { throw new Error("blocked") })
    const setItem = vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => { throw new Error("blocked") })
    try {
      const first = await getWithdrawalNotificationIdentity(deployment)
      const second = await getWithdrawalNotificationIdentity(deployment)
      expect(first.getPrincipal().toText()).toBe(second.getPrincipal().toText())
      expect(first.getPrincipal().toText()).not.toBe(Principal.anonymous().toText())
    } finally {
      getItem.mockRestore()
      setItem.mockRestore()
    }
  })

  it("converges concurrent initialization on one identity", async () => {
    const deployment = profile("6")
    const identities = await Promise.all([
      getWithdrawalNotificationIdentity(deployment),
      getWithdrawalNotificationIdentity(deployment),
    ])

    expect(identities[0].getPrincipal().toText()).toBe(identities[1].getPrincipal().toText())
  })
})

describe("withdrawal notification client", () => {
  it("sends_one_update_with_the_deployment_identity", async () => {
    const deployment = profile("7")
    const transactionHash = new Uint8Array(32).fill(9)

    await expect(notifyWithdrawalWithBrowserIdentity(transactionHash, deployment)).resolves.toMatchObject({
      Ingested: { finalized_head_block_number: 42n },
    })

    expect(mocks.createBridgeActor).toHaveBeenCalledOnce()
    expect(mocks.createBridgeActor).toHaveBeenCalledWith(
      deployment.icHost,
      deployment.bridgeCanisterId,
      expect.anything(),
    )
    expect(mocks.notifyWithdrawal).toHaveBeenCalledOnce()
    expect(mocks.notifyWithdrawal).toHaveBeenCalledWith({ transaction_hash: transactionHash })
  })

  it("decodes typed notification failures", () => {
    expect(notifyWithdrawalErrorMessage({ RpcInconsistent: null })).toContain("providers disagreed")
    expect(() => unwrapNotifyWithdrawalResult({ Err: { BaseStateMismatch: null } })).toThrow("state does not match")
    expect(() => unwrapNotifyWithdrawalResult({ Err: { RateLimited: null } })).toThrow("rate limited")

    let thrown: unknown
    try {
      unwrapNotifyWithdrawalResult({
        Err: { LedgerFeeExceedsServiceFee: { charged_service_fee: 10n, ledger_fee: 11n } },
      })
    } catch (error) {
      thrown = error
    }
    expect(thrown).toBeInstanceOf(NotifyWithdrawalCallError)
    expect((thrown as NotifyWithdrawalCallError).code).toBe("LedgerFeeExceedsServiceFee")
  })
})
