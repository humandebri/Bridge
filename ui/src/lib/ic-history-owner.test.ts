import { Principal } from "@icp-sdk/core/principal"
import { describe, expect, it, vi } from "vitest"
import {
  clearIcHistoryOwner,
  icHistoryOwnerStorageKey,
  loadIcHistoryOwner,
  sameIcAccount,
  saveIcHistoryOwner,
} from "./ic-history-owner"

function memoryStorage() {
  const values = new Map<string, string>()
  return {
    getItem: (key: string) => values.get(key) ?? null,
    setItem: (key: string, value: string) => {
      values.set(key, value)
    },
    removeItem: (key: string) => {
      values.delete(key)
    },
  }
}

describe("remembered IC history owner", () => {
  it("restores only the validated read-only owner metadata", () => {
    const storage = memoryStorage()
    const account = {
      owner: Principal.anonymous().toText(),
      subaccount: Uint8Array.from({ length: 32 }, (_, index) => index),
    }
    saveIcHistoryOwner({ account, provider: "oisy" }, storage)

    expect(loadIcHistoryOwner(storage)).toEqual({ account, provider: "oisy" })
  })

  it("rejects malformed, noncanonical, and wrong-sized stored values", () => {
    const storage = memoryStorage()
    const key = icHistoryOwnerStorageKey()
    storage.setItem(
      key,
      JSON.stringify({ version: 1, owner: "not-a-principal", subaccount: null, provider: "oisy" }),
    )
    expect(loadIcHistoryOwner(storage)).toBeUndefined()
    storage.setItem(
      key,
      JSON.stringify({ version: 1, owner: "aaaaa-aa", subaccount: "00", provider: "oisy" }),
    )
    expect(loadIcHistoryOwner(storage)).toBeUndefined()
    storage.setItem(
      key,
      JSON.stringify({ version: 1, owner: "aaaaa-aa", subaccount: null, provider: "unknown" }),
    )
    expect(loadIcHistoryOwner(storage)).toBeUndefined()
  })

  it("clears the remembered owner on explicit disconnect", () => {
    const storage = memoryStorage()
    saveIcHistoryOwner({ account: { owner: "aaaaa-aa" }, provider: "plug" }, storage)
    clearIcHistoryOwner(storage)
    expect(loadIcHistoryOwner(storage)).toBeUndefined()
  })

  it("does not let browser storage failures break wallet lifecycle calls", () => {
    const unavailable = {
      getItem: vi.fn(() => {
        throw new Error("unavailable")
      }),
      setItem: vi.fn(() => {
        throw new Error("unavailable")
      }),
      removeItem: vi.fn(() => {
        throw new Error("unavailable")
      }),
    }
    expect(loadIcHistoryOwner(unavailable)).toBeUndefined()
    expect(() =>
      saveIcHistoryOwner({ account: { owner: "aaaaa-aa" }, provider: "oisy" }, unavailable),
    ).not.toThrow()
    expect(() => clearIcHistoryOwner(unavailable)).not.toThrow()
  })

  it("requires owner and subaccount equality before a restored-owner action", () => {
    const account = { owner: "aaaaa-aa", subaccount: new Uint8Array(32) }
    expect(sameIcAccount(account, { owner: "aaaaa-aa", subaccount: new Uint8Array(32) })).toBe(true)
    expect(sameIcAccount(account, { owner: "aaaaa-aa" })).toBe(false)
    expect(sameIcAccount(account, { owner: "2vxsx-fae", subaccount: new Uint8Array(32) })).toBe(
      false,
    )
  })
})
