import { createContext, useCallback, useContext, useMemo, useState, type ReactNode } from "react"
import type { DepositReceipt } from "@/generated/bridge.did"
import type { ApprovalCall, DepositCall, IcAccount, IcWalletAdapter, IcWalletProvider } from "@/lib/ic/wallet"

const CONTROL = "http://127.0.0.1:43119"

interface IcWalletState {
  account?: IcAccount
  provider?: IcWalletProvider
  adapter?: IcWalletAdapter
  connecting?: IcWalletProvider
  connect(provider: IcWalletProvider): Promise<void>
  disconnect(): Promise<void>
}

const Context = createContext<IcWalletState | undefined>(undefined)

class HarnessWalletAdapter implements IcWalletAdapter {
  readonly provider: IcWalletProvider
  constructor(provider: IcWalletProvider) { this.provider = provider }
  connect() { return request<IcAccount>("/ic/account") }
  getAccount() { return request<IcAccount>("/ic/account") }
  async disconnect() { await request("/ic/disconnect", {}) }
  approve(call: ApprovalCall) {
    return request<bigint>("/ic/approve", {
      amount: call.amount.toString(),
      currentAllowance: call.currentAllowance.toString(),
      ledgerFee: call.ledgerFee.toString(),
    }, reviveBigInt)
  }
  requestDeposit(call: DepositCall) {
    return request<DepositReceipt>("/ic/deposit", {
      clientRequestId: hex(call.clientRequestId),
      baseRecipient: hex(call.baseRecipient),
      grossAmount: call.grossAmount.toString(),
      maxServiceFee: call.maxServiceFee.toString(),
    }, (value) => {
      const receipt = value as { deposit_id: string; state: string }
      return { deposit_id: bytes(receipt.deposit_id), state: receipt.state }
    })
  }
  async notifyWithdrawal(transactionHash: Uint8Array) {
    await request("/ic/notify", { transactionHash: hex(transactionHash) })
  }
}

export function IcWalletProviderRoot({ children }: { children: ReactNode }) {
  const [account, setAccount] = useState<IcAccount>()
  const [provider, setProvider] = useState<IcWalletProvider>()
  const [adapter, setAdapter] = useState<IcWalletAdapter>()
  const [connecting, setConnecting] = useState<IcWalletProvider>()
  const connect = useCallback(async (nextProvider: IcWalletProvider) => {
    setConnecting(nextProvider)
    try {
      const next = new HarnessWalletAdapter(nextProvider)
      setAccount(await next.connect())
      setAdapter(next)
      setProvider(nextProvider)
    } finally { setConnecting(undefined) }
  }, [])
  const disconnect = useCallback(async () => {
    await adapter?.disconnect()
    setAccount(undefined); setAdapter(undefined); setProvider(undefined)
  }, [adapter])
  const value = useMemo(() => ({ account, provider, adapter, connecting, connect, disconnect }), [account, provider, adapter, connecting, connect, disconnect])
  return <Context.Provider value={value}>{children}</Context.Provider>
}

export function useIcWallet(): IcWalletState {
  const value = useContext(Context)
  if (!value) throw new Error("useIcWallet must be used inside IcWalletProviderRoot")
  return value
}

async function request<T = void>(path: string, body?: unknown, convert?: (value: unknown) => T): Promise<T> {
  const response = await fetch(`${CONTROL}${path}`, {
    method: body === undefined ? "GET" : "POST",
    headers: { "content-type": "application/json" },
    body: body === undefined ? undefined : JSON.stringify(body),
  })
  const value: unknown = await response.json()
  if (!response.ok) throw new Error(typeof value === "object" && value !== null && "error" in value ? String(value.error) : `Harness HTTP ${response.status}`)
  return convert ? convert(value) : value as T
}

function hex(value: Uint8Array): `0x${string}` { return `0x${Array.from(value, (byte) => byte.toString(16).padStart(2, "0")).join("")}` }
function bytes(value: string): Uint8Array { return Uint8Array.from(value.slice(2).match(/../g) ?? [], (byte) => Number.parseInt(byte, 16)) }
function reviveBigInt(value: unknown): bigint { return BigInt(String(value)) }
