import { afterEach, describe, expect, it } from "vitest"
import { currentInjectedWallet, requireWalletSnapshot, sameIcAccount } from "@/lib/wallet-snapshot"

afterEach(() => {
  Reflect.deleteProperty(window, "ethereum")
})

describe("wallet snapshots", () => {
  it("reads the injected account and chain at signing time", async () => {
    Object.defineProperty(window, "ethereum", {
      configurable: true,
      value: {
        request: ({ method }: { method: string }) =>
          Promise.resolve(
            method === "eth_accounts" ? ["0x1111111111111111111111111111111111111111"] : "0x14a34",
          ),
      },
    })
    await expect(currentInjectedWallet()).resolves.toEqual({
      address: "0x1111111111111111111111111111111111111111",
      chainId: 84532,
    })
  })

  it("fails closed for a disconnected or malformed injected wallet", async () => {
    Object.defineProperty(window, "ethereum", {
      configurable: true,
      value: { request: () => Promise.resolve([]) },
    })
    await expect(currentInjectedWallet()).rejects.toThrow("disconnected")
  })

  it("treats an omitted IC subaccount as the zero subaccount", () => {
    expect(
      sameIcAccount({ owner: "aaaaa-aa" }, { owner: "aaaaa-aa", subaccount: new Uint8Array(32) }),
    ).toBe(true)
    expect(
      sameIcAccount(
        { owner: "aaaaa-aa", subaccount: new Uint8Array(32).fill(1) },
        { owner: "aaaaa-aa", subaccount: new Uint8Array(32) },
      ),
    ).toBe(false)
  })

  it("rejects account or chain drift immediately before a write", () => {
    const expected = {
      address: "0x1111111111111111111111111111111111111111" as const,
      chainId: 8453,
      icAccount: { owner: "aaaaa-aa" },
    }
    expect(() =>
      requireWalletSnapshot(expected, {
        ...expected,
        address: "0x1111111111111111111111111111111111111111",
      }),
    ).not.toThrow()
    expect(() => requireWalletSnapshot(expected, { ...expected, chainId: 84532 })).toThrow(
      "changed during confirmation",
    )
    expect(() =>
      requireWalletSnapshot(
        expected,
        { ...expected, icAccount: { owner: "2vxsx-fae" } },
        "after approval",
      ),
    ).toThrow("changed after approval")
  })
})
