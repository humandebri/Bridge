import { createContext, useCallback, useContext, useMemo, useState, type ReactNode } from "react"
import { deploymentProfile } from "@/config/profile"
import { OisyAdapter, PlugAdapter, type IcAccount, type IcWalletAdapter, type IcWalletProvider } from "@/lib/ic/wallet"

interface IcWalletState {
  account?: IcAccount
  provider?: IcWalletProvider
  adapter?: IcWalletAdapter
  connecting?: IcWalletProvider
  connect(provider: IcWalletProvider): Promise<void>
  disconnect(): Promise<void>
}

const IcWalletContext = createContext<IcWalletState | undefined>(undefined)

export function IcWalletProviderRoot({ children }: { children: ReactNode }) {
  const [account, setAccount] = useState<IcAccount>()
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
    } finally { setConnecting(undefined) }
  }, [adapter])

  const disconnect = useCallback(async () => {
    const previous = adapter
    setAdapter(undefined); setProvider(undefined); setAccount(undefined)
    await previous?.disconnect()
  }, [adapter])

  const value = useMemo(() => ({ account, provider, adapter, connecting, connect, disconnect }), [account, provider, adapter, connecting, connect, disconnect])
  return <IcWalletContext.Provider value={value}>{children}</IcWalletContext.Provider>
}

export function useIcWallet(): IcWalletState {
  const context = useContext(IcWalletContext)
  if (!context) throw new Error("useIcWallet must be used inside IcWalletProviderRoot")
  return context
}
