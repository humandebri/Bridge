import { createContext, useCallback, useContext, useMemo, useState, type ReactNode } from "react"
import { deploymentProfile } from "@/config/profile"
import { clearIcHistoryOwner, loadIcHistoryOwner, saveIcHistoryOwner } from "@/lib/ic-history-owner"
import { OisyAdapter, PlugAdapter, type IcAccount, type IcWalletAdapter, type IcWalletProvider } from "@/lib/ic/wallet"

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

const IcWalletContext = createContext<IcWalletState | undefined>(undefined)

export function IcWalletProviderRoot({ children }: { children: ReactNode }) {
  const [rememberedOwner] = useState(loadIcHistoryOwner)
  const [restoredWallet] = useState(() => restoreWallet(rememberedOwner))
  const [account, setAccount] = useState<IcAccount | undefined>(restoredWallet?.account)
  const [historyAccount, setHistoryAccount] = useState<IcAccount | undefined>(rememberedOwner?.account)
  const [historyProvider, setHistoryProvider] = useState<IcWalletProvider | undefined>(rememberedOwner?.provider)
  const [provider, setProvider] = useState<IcWalletProvider | undefined>(restoredWallet?.provider)
  const [adapter, setAdapter] = useState<IcWalletAdapter | undefined>(restoredWallet?.adapter)
  const [connecting, setConnecting] = useState<IcWalletProvider>()

  const connect = useCallback(async (nextProvider: IcWalletProvider) => {
    if (!deploymentProfile.ledgerCanisterId || !deploymentProfile.bridgeCanisterId) throw new Error("Bridge is temporarily unavailable")
    setConnecting(nextProvider)
    const previous = adapter
    setAdapter(undefined); setProvider(undefined); setAccount(undefined)
    try {
      await previous?.disconnect()
      const next = nextProvider === "oisy"
        ? new OisyAdapter(deploymentProfile.icHost, deploymentProfile.ledgerCanisterId, deploymentProfile.bridgeCanisterId)
        : new PlugAdapter(deploymentProfile.icHost, deploymentProfile.ledgerCanisterId, deploymentProfile.bridgeCanisterId)
      const nextAccount = await next.connect()
      setAdapter(next)
      setProvider(nextProvider)
      setAccount(nextAccount)
      setHistoryAccount(nextAccount)
      setHistoryProvider(nextProvider)
      saveIcHistoryOwner({ account: nextAccount, provider: nextProvider })
    } finally { setConnecting(undefined) }
  }, [adapter])

  const disconnect = useCallback(async () => {
    const previous = adapter
    setAdapter(undefined); setProvider(undefined); setAccount(undefined)
    setHistoryAccount(undefined); setHistoryProvider(undefined)
    clearIcHistoryOwner()
    await previous?.disconnect()
  }, [adapter])

  const value = useMemo(
    () => ({ account, historyAccount, historyProvider, provider, adapter, connecting, connect, disconnect }),
    [account, historyAccount, historyProvider, provider, adapter, connecting, connect, disconnect],
  )
  return <IcWalletContext.Provider value={value}>{children}</IcWalletContext.Provider>
}

function restoreWallet(owner: ReturnType<typeof loadIcHistoryOwner>): {
  account: IcAccount
  provider: IcWalletProvider
  adapter: IcWalletAdapter
} | undefined {
  if (!owner || !deploymentProfile.ledgerCanisterId || !deploymentProfile.bridgeCanisterId) return undefined
  const account = { owner: owner.account.owner, subaccount: owner.account.subaccount?.slice() }
  const adapter = owner.provider === "oisy"
    ? new OisyAdapter(
      deploymentProfile.icHost,
      deploymentProfile.ledgerCanisterId,
      deploymentProfile.bridgeCanisterId,
      undefined,
      account,
    )
    : new PlugAdapter(
      deploymentProfile.icHost,
      deploymentProfile.ledgerCanisterId,
      deploymentProfile.bridgeCanisterId,
      account,
    )
  return { account, provider: owner.provider, adapter }
}

export function useIcWallet(): IcWalletState {
  const context = useContext(IcWalletContext)
  if (!context) throw new Error("useIcWallet must be used inside IcWalletProviderRoot")
  return context
}
