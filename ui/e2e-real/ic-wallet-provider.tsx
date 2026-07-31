import { createContext, useCallback, useContext, useMemo, useState, type ReactNode } from "react"
import type { DepositPhase, DepositReceipt, NotifyDepositMintReceipt, NotifyWithdrawalReceipt, SettlementActionResult } from "@/generated/bridge.did"
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
  readonly requiresUserGesture = false
  constructor(provider: IcWalletProvider) { this.provider = provider }
  connect() { return request<IcAccount>("/ic/account") }
  getAccount() { return request<IcAccount>("/ic/account") }
  async disconnect() { await request("/ic/disconnect", {}) }
  prepare(): Promise<() => Promise<void>> { return Promise.resolve(() => Promise.resolve()) }
  approve(call: ApprovalCall) {
    return request<bigint>("/ic/approve", {
      amount: call.amount.toString(),
      currentAllowance: call.currentAllowance.toString(),
      ledgerFee: call.ledgerFee.toString(),
    }, reviveBigInt)
  }
  requestDeposit(call: DepositCall) {
    return request<DepositReceipt>("/ic/deposit", {
      ownerSequence: call.ownerSequence.toString(),
      baseRecipient: hex(call.baseRecipient),
      grossAmount: call.grossAmount.toString(),
      maxServiceFee: call.maxServiceFee.toString(),
    }, (value) => {
      const receipt = value as { deposit_id: string; owner_sequence: string; state: DepositPhase }
      return { deposit_id: bytes(receipt.deposit_id), owner_sequence: BigInt(receipt.owner_sequence), state: receipt.state }
    })
  }
  notifyDepositMint(depositId: Uint8Array, transactionHash: Uint8Array) {
    return request<NotifyDepositMintReceipt>("/ic/notify-deposit-mint", {
      depositId: hex(depositId),
      transactionHash: hex(transactionHash),
    }, (value) => {
      const receipt = value as {
        Minted?: { deposit_id: string; transaction_hash: string; finalized_head_block_number: string }
        Duplicate?: { deposit_id: string; transaction_hash: string }
      }
      if (receipt.Minted) return { Minted: {
        deposit_id: bytes(receipt.Minted.deposit_id),
        transaction_hash: bytes(receipt.Minted.transaction_hash),
        finalized_head_block_number: BigInt(receipt.Minted.finalized_head_block_number),
      } }
      if (receipt.Duplicate) return { Duplicate: {
        deposit_id: bytes(receipt.Duplicate.deposit_id),
        transaction_hash: bytes(receipt.Duplicate.transaction_hash),
      } }
      throw new Error("Harness returned an invalid mint notification receipt")
    })
  }
  notifyWithdrawal(transactionHash: Uint8Array) {
    return request<NotifyWithdrawalReceipt>("/ic/notify", { transactionHash: hex(transactionHash) }, (value) => {
      const receipt = value as { Ingested?: { finalized_head_block_number: string; withdrawal_id: string }; Duplicate?: { withdrawal_id: string } }
      if (receipt.Ingested) return { Ingested: { finalized_head_block_number: BigInt(receipt.Ingested.finalized_head_block_number), withdrawal_id: bytes(receipt.Ingested.withdrawal_id) } }
      if (receipt.Duplicate) return { Duplicate: { withdrawal_id: bytes(receipt.Duplicate.withdrawal_id) } }
      throw new Error("Harness returned an invalid notification receipt")
    })
  }
  continueDeposit(depositId: Uint8Array) { return request<SettlementActionResult>("/ic/continue-deposit", { id: hex(depositId) }) }
  continueWithdrawal(withdrawalId: Uint8Array) { return request<SettlementActionResult>("/ic/continue-withdrawal", { id: hex(withdrawalId) }) }
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
