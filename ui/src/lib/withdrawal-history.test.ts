import { Principal } from "@icp-sdk/core/principal"
import { bytesToHex } from "viem"
import { describe, expect, it, vi } from "vitest"
import { NotifyWithdrawalCallError } from "./ic/withdrawal-notification-client"
import {
  decodeWithdrawalDestination,
  fetchInBatches,
  notifyHistoryWithdrawal,
} from "./withdrawal-history"

describe("withdrawal log scanning", () => {
  it("decodes_the_finalized_IC_destination_independently_of_the_EVM_requester", () => {
    const principal = Principal.selfAuthenticating(new Uint8Array(32).fill(0x11))
    const subaccount = new Uint8Array(32).fill(0x22)

    expect(
      decodeWithdrawalDestination(bytesToHex(principal.toUint8Array()), bytesToHex(subaccount)),
    ).toEqual({
      owner: principal.toText(),
      subaccount,
    })
  })

  it("rejects malformed finalized IC destinations", () => {
    const subaccount = bytesToHex(new Uint8Array(32))
    expect(() => decodeWithdrawalDestination("0x", subaccount)).toThrow("owner is invalid")
    expect(() => decodeWithdrawalDestination("0x04", subaccount)).toThrow("owner is invalid")
    expect(() => decodeWithdrawalDestination(bytesToHex(new Uint8Array(30)), subaccount)).toThrow(
      "owner is invalid",
    )
    expect(() => decodeWithdrawalDestination("0x01", bytesToHex(new Uint8Array(31)))).toThrow(
      "subaccount must be 32 bytes",
    )
  })

  it("persists_the_event_owner_before_notifying_from_History", async () => {
    const withdrawalId = new Uint8Array(32).fill(0x33)
    const ensurePending = vi.fn(() => Promise.resolve())
    const notify = vi.fn(() =>
      Promise.resolve({ Duplicate: { withdrawal_id: withdrawalId } } as const),
    )
    const markNotified = vi.fn(() => Promise.resolve())

    const result = await notifyHistoryWithdrawal(
      {
        hash: `0x${"44".repeat(32)}`,
        destinationAccount: { owner: "aaaaa-aa", subaccount: new Uint8Array(32) },
      },
      { ensurePending, notify, markNotified },
    )

    expect(result.pending.owner).toBe("aaaaa-aa")
    expect(ensurePending.mock.invocationCallOrder[0]).toBeLessThan(
      notify.mock.invocationCallOrder[0]!,
    )
    expect(notify.mock.invocationCallOrder[0]).toBeLessThan(
      markNotified.mock.invocationCallOrder[0]!,
    )
    expect(markNotified).toHaveBeenCalledWith(result.pending, bytesToHex(withdrawalId))
  })

  it("does_not_notify_when_the_History_destination_conflicts", async () => {
    const ensurePending = vi.fn(() => Promise.reject(new Error("destination owner conflict")))
    const notify = vi.fn()
    const markNotified = vi.fn()

    await expect(
      notifyHistoryWithdrawal(
        {
          hash: `0x${"55".repeat(32)}`,
          destinationAccount: { owner: "aaaaa-aa", subaccount: new Uint8Array(32) },
        },
        { ensurePending, notify, markNotified },
      ),
    ).rejects.toThrow("destination owner conflict")

    expect(notify).not.toHaveBeenCalled()
    expect(markNotified).not.toHaveBeenCalled()
  })

  it("treats_a_pre_boundary_withdrawal_as_terminal_without_retry", async () => {
    const failure = new NotifyWithdrawalCallError(
      "WithdrawalBeforeAdmissionBoundary",
      "Withdrawal predates the admission boundary",
    )
    const notify = vi.fn(() => Promise.reject(failure))
    const setFailure = vi.fn(() => Promise.resolve())
    const delay = vi.fn(() => Promise.resolve())

    await expect(
      notifyHistoryWithdrawal(
        {
          hash: `0x${"66".repeat(32)}`,
          destinationAccount: { owner: "aaaaa-aa", subaccount: new Uint8Array(32) },
        },
        {
          ensurePending: vi.fn(() => Promise.resolve()),
          notify,
          markNotified: vi.fn(() => Promise.resolve()),
          markAttempt: vi.fn(() => Promise.resolve()),
          setFailure,
          delay,
        },
      ),
    ).rejects.toBe(failure)

    expect(notify).toHaveBeenCalledOnce()
    expect(delay).not.toHaveBeenCalled()
    expect(setFailure).toHaveBeenCalledWith(
      expect.anything(),
      expect.objectContaining({
        code: "WithdrawalBeforeAdmissionBoundary",
        disposition: "terminal",
      }),
    )
  })

  it("loads more than 20 canister views in ordered batches", async () => {
    const fetchBatch = vi.fn((batch: number[]) =>
      Promise.resolve(batch.map((value) => `view-${value}`)),
    )

    const result = await fetchInBatches(
      Array.from({ length: 21 }, (_, index) => index),
      20,
      fetchBatch,
    )

    expect(fetchBatch.mock.calls.map(([batch]) => batch.length)).toEqual([20, 1])
    expect(result).toEqual(Array.from({ length: 21 }, (_, index) => `view-${index}`))
  })
})
