import type { IcAccount } from "@/lib/ic/wallet"

interface InjectedProvider {
  request(args: { method: "eth_accounts" | "eth_chainId" }): Promise<unknown>
}

declare global {
  interface Window {
    ethereum?: InjectedProvider
  }
}

export async function currentInjectedWallet(
  selectedProvider?: InjectedProvider,
): Promise<{ address: `0x${string}`; chainId: number }> {
  const provider = selectedProvider ?? window.ethereum
  if (!provider) throw new Error("The injected EVM wallet is unavailable")
  const [accounts, chain] = await Promise.all([
    provider.request({ method: "eth_accounts" }),
    provider.request({ method: "eth_chainId" }),
  ])
  if (
    !Array.isArray(accounts) ||
    typeof accounts[0] !== "string" ||
    !/^0x[0-9a-fA-F]{40}$/.test(accounts[0])
  )
    throw new Error("The EVM wallet is disconnected")
  if (typeof chain !== "string" || !/^0x[0-9a-fA-F]+$/.test(chain))
    throw new Error("The EVM wallet returned an invalid chain")
  return { address: accounts[0] as `0x${string}`, chainId: Number.parseInt(chain.slice(2), 16) }
}

export function sameIcAccount(left: IcAccount, right: IcAccount): boolean {
  if (left.owner !== right.owner) return false
  const leftSubaccount = left.subaccount ?? new Uint8Array(32)
  const rightSubaccount = right.subaccount ?? new Uint8Array(32)
  return (
    leftSubaccount.length === rightSubaccount.length &&
    leftSubaccount.every((byte, index) => byte === rightSubaccount[index])
  )
}

export function requireWalletSnapshot(
  expected: { address: `0x${string}`; chainId: number; icAccount: IcAccount },
  current: { address: `0x${string}`; chainId: number; icAccount: IcAccount },
  context = "during confirmation",
): void {
  if (
    current.chainId !== expected.chainId ||
    current.address.toLowerCase() !== expected.address.toLowerCase() ||
    !sameIcAccount(current.icAccount, expected.icAccount)
  ) {
    throw new Error(`A connected account or chain changed ${context}; review and submit again`)
  }
}
