import { createContext, useContext, useMemo, useState, type ReactNode } from "react"
import { ArrowRight, Check, LoaderCircle, LogOut, QrCode, Wallet } from "lucide-react"
import { useAccount, useConnect, useConnectors, useDisconnect, type Connector } from "wagmi"
import { toast } from "sonner"
import { Button } from "@/components/ui/button"
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from "@/components/ui/dialog"
import { deploymentProfile } from "@/config/profile"
import { useIcWallet } from "@/features/wallet/ic-wallet-provider"
import type { IcWalletProvider } from "@/lib/ic/wallet"
import baseLogo from "@/assets/base-square.svg"
import blueKinic from "@/assets/blue_kinic.png"
import icpLogo from "@/assets/icp-logo-mark.svg"
import coinbaseLogo from "@/assets/wallets/coinbase-wallet.svg"
import metamaskLogo from "@/assets/wallets/metamask-wallet.svg"
import oisyLogo from "@/assets/wallets/oisy-wallet.svg"
import plugLogo from "@/assets/wallets/plug-wallet.svg"

export type WalletSide = "ic" | "base"

interface WalletDialogValue {
  open: boolean
  target?: WalletSide
  openFor(target: WalletSide): void
  setOpen(open: boolean): void
}
const WalletDialogContext = createContext<WalletDialogValue | null>(null)

const icWalletBrands: Record<IcWalletProvider, { name: string; description: string; icon: string }> = {
  oisy: { name: "OISY Wallet", description: "Secure web wallet", icon: oisyLogo },
  plug: { name: "Plug", description: "Browser extension", icon: plugLogo },
}

export function WalletDialogProvider({ children }: { children: ReactNode }) {
  const [open, setOpen] = useState(false)
  const [target, setTarget] = useState<WalletSide>()
  const value = useMemo(() => ({
    open,
    target,
    openFor(nextTarget: WalletSide) { setTarget(nextTarget); setOpen(true) },
    setOpen,
  }), [open, target])
  return <WalletDialogContext.Provider value={value}>{children}</WalletDialogContext.Provider>
}

export function useWalletDialog() {
  const value = useContext(WalletDialogContext)
  if (!value) throw new Error("useWalletDialog must be used inside WalletDialogProvider")
  return value
}

function short(value: string) { return `${value.slice(0, 6)}…${value.slice(-4)}` }

export function visibleEvmConnectors(connectors: readonly Connector[]): Connector[] {
  const hasPlug = connectors.some(isPlug)
  const supportedConnectors = connectors.filter((connector) => !isPlug(connector))
  const hasNamedInjected = supportedConnectors.some((connector) => connector.type === "injected" && !isGenericInjected(connector))
  return [...connectors]
    .filter((connector) => !isPlug(connector))
    .filter((connector) => !((hasPlug || hasNamedInjected) && connector.type === "injected" && isGenericInjected(connector)))
    .sort((left, right) => {
      const leftWalletConnect = isWalletConnect(left)
      const rightWalletConnect = isWalletConnect(right)
      if (leftWalletConnect !== rightWalletConnect) return leftWalletConnect ? 1 : -1
      return left.name.localeCompare(right.name)
    })
}

function isGenericInjected(connector: Connector): boolean {
  return connector.name.trim().toLowerCase() === "injected"
}

function isWalletConnect(connector: Connector): boolean {
  return connector.id === "walletConnect" || connector.type === "walletConnect"
}

function isMetaMask(connector: Connector): boolean {
  return connector.id === "io.metamask"
}

function isCoinbaseWallet(connector: Connector): boolean {
  return connector.id === "coinbaseWalletSDK" || connector.type === "coinbaseWallet"
}

function isPlug(connector: Connector): boolean {
  return connector.name.trim().toLowerCase() === "plug"
    || connector.name.trim().toLowerCase() === "plug wallet"
    || connector.id.toLowerCase().includes("plug")
}

function connectorIcon(connector?: Connector): string | undefined {
  if (connector && isCoinbaseWallet(connector)) return coinbaseLogo
  if (connector && isMetaMask(connector)) return metamaskLogo
  return connector?.icon?.startsWith("data:image/") ? connector.icon : undefined
}

export function WalletCenter() {
  const dialog = useWalletDialog()
  const { address, connector, isConnected } = useAccount()
  const ic = useIcWallet()
  const icBrand = ic.provider ? icWalletBrands[ic.provider] : undefined
  return <>
    <div className="flex items-center gap-1.5 sm:gap-2" aria-label="Wallet connections">
      <WalletSummary side="ic" value={ic.account?.owner} walletName={icBrand?.name} icon={icBrand?.icon} onClick={() => dialog.openFor("ic")} />
      <WalletSummary side="base" value={isConnected ? address : undefined} walletName={isConnected ? connector?.name : undefined} icon={connectorIcon(connector)} walletConnect={isConnected && connector ? isWalletConnect(connector) : false} onClick={() => dialog.openFor("base")} />
    </div>
    <WalletDialog />
  </>
}

function WalletSummary({ side, value, walletName, icon, walletConnect, onClick }: {
  side: WalletSide
  value?: string
  walletName?: string
  icon?: string
  walletConnect?: boolean
  onClick: () => void
}) {
  const connected = Boolean(value)
  const label = side === "ic" ? "IC wallet" : "EVM wallet"
  const displayValue = value ? short(value) : "Connect"
  return <button type="button" onClick={onClick} aria-label={connected ? `${label} connected as ${value}` : `Connect ${label}`} className="group relative flex size-12 min-w-12 items-center justify-center gap-2 rounded-2xl border border-[var(--line)] bg-white px-0 text-left transition duration-300 hover:-translate-y-0.5 hover:border-black focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[var(--focus)] min-[360px]:h-12 min-[360px]:w-auto min-[360px]:min-w-[76px] min-[360px]:justify-start min-[360px]:px-2 sm:min-w-[142px] sm:px-3">
    <WalletGlyph side={side} name={walletName ?? label} icon={icon} walletConnect={walletConnect} compact />
    <span className="hidden min-w-0 leading-none min-[360px]:block">
      <span className="block text-[9px] font-bold uppercase tracking-[.08em] text-[var(--muted)] sm:text-[10px]">{label}</span>
      <span className="mt-1 block max-w-[82px] truncate text-[11px] font-bold text-black sm:text-xs">{displayValue}</span>
    </span>
    {connected && <span className="absolute right-1.5 top-1.5 size-2 rounded-full border-2 border-white bg-[var(--success)]" aria-hidden="true" />}
  </button>
}

function WalletDialog() {
  const dialog = useWalletDialog()
  const { address, connector, isConnected } = useAccount()
  const connectors = visibleEvmConnectors(useConnectors())
  const { connectAsync, isPending, variables } = useConnect()
  const { disconnect } = useDisconnect()
  const ic = useIcWallet()
  const connectIc = (provider: IcWalletProvider) => void ic.connect(provider).catch(showWalletError)
  const connectEvm = (nextConnector: Connector) => void connectAsync({
    connector: nextConnector,
    chainId: deploymentProfile.chainId,
  }).catch(showWalletError)
  const target = dialog.target
  const title = target === "ic" ? "IC wallet" : target === "base" ? "EVM wallet" : "Connect wallets"
  const description = target === "ic"
    ? ic.account
      ? "Review or disconnect the Internet Computer wallet connected to this bridge."
      : "Choose the wallet that owns the Internet Computer account."
    : target === "base"
      ? isConnected && address
        ? "Review or disconnect the EVM wallet connected to Base."
        : "Choose the EVM wallet that will sign transactions on Base."
      : "Connect both sides of the bridge. Verify every account again before moving KINIC."
  const headerLogo = target === "ic" ? icpLogo : target === "base" ? baseLogo : blueKinic
  const connectedBrand = ic.provider ? icWalletBrands[ic.provider] : undefined
  const pendingConnectorUid = variables?.connector && "uid" in variables.connector ? variables.connector.uid : undefined

  return <Dialog open={dialog.open} onOpenChange={(open) => dialog.setOpen(open)}><DialogContent className="max-h-[min(720px,calc(100vh-2rem))] overflow-y-auto">
    <DialogHeader><div className="mb-3 flex items-center gap-3"><span className="grid size-10 shrink-0 place-items-center overflow-hidden rounded-xl bg-white"><img src={headerLogo} alt="" data-dialog-network-logo={target} className={target === "ic" ? "w-9" : "size-10 object-cover"} /></span><DialogTitle>{title}</DialogTitle></div><DialogDescription>{description}</DialogDescription></DialogHeader>
    <div className="mt-6 space-y-6">
      {(!target || target === "base") && <WalletSection label="EVM wallet">
        {isConnected && address ? <ConnectedWallet
          name={connector?.name ?? "EVM wallet"}
          value={short(address)}
          icon={connectorIcon(connector)}
          walletConnect={connector ? isWalletConnect(connector) : false}
          side="base"
          onDisconnect={() => disconnect()}
        /> : connectors.length ? <div className="grid gap-2">
          {connectors.map((nextConnector) => <WalletOption
            key={nextConnector.uid}
            name={isGenericInjected(nextConnector) ? "Browser wallet" : nextConnector.name}
            description={isWalletConnect(nextConnector) ? "Scan with a mobile wallet" : isCoinbaseWallet(nextConnector) ? "Coinbase app or smart wallet" : isMetaMask(nextConnector) ? "Browser extension" : isGenericInjected(nextConnector) ? "Use an installed extension" : "Detected in this browser"}
            icon={connectorIcon(nextConnector)}
            walletConnect={isWalletConnect(nextConnector)}
            side="base"
            disabled={isPending}
            busy={isPending && pendingConnectorUid === nextConnector.uid}
            onClick={() => connectEvm(nextConnector)}
          />)}
        </div> : <WalletEmpty />}
      </WalletSection>}

      {(!target || target === "ic") && <WalletSection label="IC wallet">
        {ic.account && connectedBrand ? <ConnectedWallet
          name={connectedBrand.name}
          value={short(ic.account.owner)}
          icon={connectedBrand.icon}
          side="ic"
          onDisconnect={() => void ic.disconnect().catch(showWalletError)}
        /> : <div className="grid gap-2">
          {(Object.entries(icWalletBrands) as [IcWalletProvider, (typeof icWalletBrands)[IcWalletProvider]][]).map(([provider, brand]) => <WalletOption
            key={provider}
            name={brand.name}
            description={brand.description}
            icon={brand.icon}
            side="ic"
            disabled={Boolean(ic.connecting)}
            busy={ic.connecting === provider}
            onClick={() => connectIc(provider)}
          />)}
        </div>}
      </WalletSection>}
    </div>
  </DialogContent></Dialog>
}

function showWalletError(error: unknown) {
  toast.error(error instanceof Error ? error.message : String(error))
}

function WalletSection({ label, children }: { label: string; children: ReactNode }) {
  return <section aria-label={label}>
    <p className="mb-2 text-[10px] font-bold uppercase tracking-[.13em] text-[var(--muted)]">{label}</p>
    {children}
  </section>
}

function WalletOption({ name, description, icon, walletConnect, side, disabled, busy, onClick }: {
  name: string
  description: string
  icon?: string
  walletConnect?: boolean
  side: WalletSide
  disabled: boolean
  busy: boolean
  onClick: () => void
}) {
  return <button type="button" aria-label={`Connect ${name}`} disabled={disabled} onClick={onClick} className="group flex min-h-16 w-full items-center gap-3 rounded-2xl border border-[var(--line)] bg-white p-3 text-left transition duration-300 hover:-translate-y-0.5 hover:border-black hover:shadow-[0_10px_30px_rgba(20,34,53,.08)] focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[var(--focus)] disabled:cursor-not-allowed disabled:opacity-55 disabled:hover:translate-y-0 disabled:hover:border-[var(--line)] disabled:hover:shadow-none">
    <WalletGlyph side={side} name={name} icon={icon} walletConnect={walletConnect} />
    <span className="min-w-0 flex-1">
      <strong className="block truncate text-sm font-bold text-black">{name}</strong>
      <span className="mt-0.5 block truncate text-xs text-[var(--muted)]">{busy ? "Waiting for wallet…" : description}</span>
    </span>
    {busy ? <LoaderCircle className="size-4 shrink-0 animate-spin text-[var(--pink)]" aria-hidden="true" /> : <ArrowRight className="size-4 shrink-0 text-[var(--line-strong)] transition-transform group-hover:translate-x-0.5 group-hover:text-black" aria-hidden="true" />}
  </button>
}

function ConnectedWallet({ name, value, icon, walletConnect, side, onDisconnect }: {
  name: string
  value: string
  icon?: string
  walletConnect?: boolean
  side: WalletSide
  onDisconnect: () => void
}) {
  return <div className="flex items-center gap-3 rounded-2xl bg-[var(--panel)] p-3">
    <WalletGlyph side={side} name={name} icon={icon} walletConnect={walletConnect} />
    <div className="min-w-0 flex-1">
      <p className="flex items-center gap-2 text-sm font-bold text-black">{name}<Check className="size-4 text-[var(--success)]" aria-label="Connected" /></p>
      <p className="mt-0.5 truncate text-xs text-[var(--muted)]">{value}</p>
    </div>
    <Button variant="ghost" size="icon" aria-label={`Disconnect ${name}`} onClick={onDisconnect}><LogOut className="size-4" /></Button>
  </div>
}

function WalletEmpty() {
  return <div className="rounded-2xl bg-[var(--panel)] px-4 py-5 text-center">
    <Wallet className="mx-auto size-5 text-[var(--muted)]" aria-hidden="true" />
    <p className="mt-2 text-sm font-bold">No EVM wallet found</p>
    <p className="mt-1 text-xs leading-5 text-[var(--muted)]">Install a browser wallet or enable WalletConnect for this environment.</p>
  </div>
}

function WalletGlyph({ side, name, icon, walletConnect, compact }: {
  side: WalletSide
  name: string
  icon?: string
  walletConnect?: boolean
  compact?: boolean
}) {
  const size = compact ? "size-7" : "size-10"
  if (icon) return <span className={`${size} grid shrink-0 place-items-center overflow-hidden rounded-xl bg-white`}><img src={icon} alt={`${name} logo`} className={`${compact ? "size-7" : "size-9"} object-contain`} /></span>
  if (walletConnect) return <span className={`${size} grid shrink-0 place-items-center rounded-xl bg-[#3b99fc] text-white`}><QrCode className={compact ? "size-4" : "size-5"} aria-hidden="true" /></span>
  const networkName = side === "ic" ? "Internet Computer" : "Base"
  return <span className={`${size} grid shrink-0 place-items-center overflow-hidden rounded-xl bg-white`}><img src={side === "ic" ? icpLogo : baseLogo} alt={`${networkName} logo`} data-network-logo={side} className={side === "ic" ? `${compact ? "w-7" : "w-9"} h-auto` : `${compact ? "size-7" : "size-9"} object-contain`} /></span>
}
