import { createContext, useCallback, useContext, useMemo, useState, type ReactNode } from "react"
import type {
  DepositPhase,
  DepositReceipt,
  DepositView,
  SettlementActionResult,
} from "@/generated/bridge.did"
import { clearIcHistoryOwner, loadIcHistoryOwner, saveIcHistoryOwner } from "@/lib/ic-history-owner"
import type {
  ApprovalCall,
  DepositCall,
  IcAccount,
  IcWalletAdapter,
  IcWalletProvider,
} from "@/lib/ic/wallet"

const CONTROL = "http://127.0.0.1:43119"

interface IcWalletState {
  account?: IcAccount
  historyAccount?: IcAccount
  historyProvider?: IcWalletProvider
  provider?: IcWalletProvider
  adapter?: IcWalletAdapter
  connecting?: IcWalletProvider
  connect(provider: IcWalletProvider): Promise<void>
  disconnect(): Promise<void>
}

const Context = createContext<IcWalletState | undefined>(undefined)

class HarnessWalletAdapter implements IcWalletAdapter {
  readonly provider: IcWalletProvider
  readonly requiresUserGesture = false
  constructor(provider: IcWalletProvider) {
    this.provider = provider
  }
  connect() {
    return request<IcAccount>("/ic/account")
  }
  getAccount() {
    return request<IcAccount>("/ic/account")
  }
  async disconnect() {
    await request("/ic/disconnect", {})
  }
  prepare(): Promise<() => Promise<void>> {
    return Promise.resolve(() => Promise.resolve())
  }
  approve(call: ApprovalCall) {
    return request<bigint>(
      "/ic/approve",
      {
        amount: call.amount.toString(),
        currentAllowance: call.currentAllowance.toString(),
        ledgerFee: call.ledgerFee.toString(),
      },
      reviveBigInt,
    )
  }
  requestDeposit(call: DepositCall) {
    return request<DepositReceipt>(
      "/ic/deposit",
      {
        ownerSequence: call.ownerSequence.toString(),
        baseRecipient: hex(call.baseRecipient),
        grossAmount: call.grossAmount.toString(),
        maxServiceFee: call.maxServiceFee.toString(),
      },
      (value) => {
        const receipt = value as { deposit_id: string; owner_sequence: string; state: DepositPhase }
        return {
          deposit_id: bytes(receipt.deposit_id),
          owner_sequence: BigInt(receipt.owner_sequence),
          state: receipt.state,
        }
      },
    )
  }
  requestDepositRefund(depositId: Uint8Array) {
    return request<DepositView>(
      "/ic/request-deposit-refund",
      { id: hex(depositId) },
      (value) => value as DepositView,
    )
  }
  continueDeposit(depositId: Uint8Array) {
    return request<SettlementActionResult>("/ic/continue-deposit", { id: hex(depositId) })
  }
  continueWithdrawal(withdrawalId: Uint8Array) {
    return request<SettlementActionResult>("/ic/continue-withdrawal", { id: hex(withdrawalId) })
  }
}

export function IcWalletProviderRoot({ children }: { children: ReactNode }) {
  const [rememberedOwner] = useState(loadIcHistoryOwner)
  const [restoredWallet] = useState(() => restoreWallet(rememberedOwner))
  const [account, setAccount] = useState<IcAccount | undefined>(restoredWallet?.account)
  const [historyAccount, setHistoryAccount] = useState<IcAccount | undefined>(
    rememberedOwner?.account,
  )
  const [historyProvider, setHistoryProvider] = useState<IcWalletProvider | undefined>(
    rememberedOwner?.provider,
  )
  const [provider, setProvider] = useState<IcWalletProvider | undefined>(restoredWallet?.provider)
  const [adapter, setAdapter] = useState<IcWalletAdapter | undefined>(restoredWallet?.adapter)
  const [connecting, setConnecting] = useState<IcWalletProvider>()
  const connect = useCallback(async (nextProvider: IcWalletProvider) => {
    setConnecting(nextProvider)
    try {
      const next = new HarnessWalletAdapter(nextProvider)
      const nextAccount = await next.connect()
      setAccount(nextAccount)
      setAdapter(next)
      setProvider(nextProvider)
      setHistoryAccount(nextAccount)
      setHistoryProvider(nextProvider)
      saveIcHistoryOwner({ account: nextAccount, provider: nextProvider })
    } finally {
      setConnecting(undefined)
    }
  }, [])
  const disconnect = useCallback(async () => {
    setAccount(undefined)
    setAdapter(undefined)
    setProvider(undefined)
    setHistoryAccount(undefined)
    setHistoryProvider(undefined)
    clearIcHistoryOwner()
    await adapter?.disconnect()
  }, [adapter])
  const value = useMemo(
    () => ({
      account,
      historyAccount,
      historyProvider,
      provider,
      adapter,
      connecting,
      connect,
      disconnect,
    }),
    [account, historyAccount, historyProvider, provider, adapter, connecting, connect, disconnect],
  )
  return <Context.Provider value={value}>{children}</Context.Provider>
}

function restoreWallet(owner: ReturnType<typeof loadIcHistoryOwner>):
  | {
      account: IcAccount
      provider: IcWalletProvider
      adapter: IcWalletAdapter
    }
  | undefined {
  if (!owner) return undefined
  const account = { owner: owner.account.owner, subaccount: owner.account.subaccount?.slice() }
  return { account, provider: owner.provider, adapter: new HarnessWalletAdapter(owner.provider) }
}

export function useIcWallet(): IcWalletState {
  const value = useContext(Context)
  if (!value) throw new Error("useIcWallet must be used inside IcWalletProviderRoot")
  return value
}

async function request<T = void>(
  path: string,
  body?: unknown,
  convert?: (value: unknown) => T,
): Promise<T> {
  const response = await fetch(`${CONTROL}${path}`, {
    method: body === undefined ? "GET" : "POST",
    headers: { "content-type": "application/json" },
    body: body === undefined ? undefined : JSON.stringify(body),
  })
  const value: unknown = await response.json()
  if (!response.ok)
    throw new Error(
      typeof value === "object" && value !== null && "error" in value
        ? String(value.error)
        : `Harness HTTP ${response.status}`,
    )
  return convert ? convert(value) : (value as T)
}

function hex(value: Uint8Array): `0x${string}` {
  return `0x${Array.from(value, (byte) => byte.toString(16).padStart(2, "0")).join("")}`
}
function bytes(value: string): Uint8Array {
  return Uint8Array.from(value.slice(2).match(/../g) ?? [], (byte) => Number.parseInt(byte, 16))
}
function reviveBigInt(value: unknown): bigint {
  return BigInt(String(value))
}
