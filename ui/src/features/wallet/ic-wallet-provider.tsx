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
  const [account, setAccount] = useState<IcAccount>()
  const [historyAccount, setHistoryAccount] = useState<IcAccount | undefined>(rememberedOwner?.account)
  const [historyProvider, setHistoryProvider] = useState<IcWalletProvider | undefined>(rememberedOwner?.provider)
  const [provider, setProvider] = useState<IcWalletProvider>()
  const [adapter, setAdapter] = useState<IcWalletAdapter>()
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

export function useIcWallet(): IcWalletState {
  const context = useContext(IcWalletContext)
  if (!context) throw new Error("useIcWallet must be used inside IcWalletProviderRoot")
  return context
}
