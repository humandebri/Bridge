import { createContext, useContext, useMemo, useState, type ReactNode } from "react"
import { Cable, Check, LogOut } from "lucide-react"
import { useAccount, useConnect, useDisconnect } from "wagmi"
import { toast } from "sonner"
import { Button } from "@/components/ui/button"
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from "@/components/ui/dialog"
import { useIcWallet } from "@/features/wallet/ic-wallet-provider"

export type WalletSide = "ic" | "base"

interface WalletDialogValue {
  open: boolean
  target?: WalletSide
  openFor(target: WalletSide): void
  setOpen(open: boolean): void
}
const WalletDialogContext = createContext<WalletDialogValue | null>(null)

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

export function WalletCenter() {
  const dialog = useWalletDialog()
  const { address, isConnected } = useAccount()
  const ic = useIcWallet()
  return <>
    <div className="flex items-center gap-1.5 sm:gap-2" aria-label="Wallet connections">
      <WalletSummary side="ic" value={ic.account?.owner} provider={ic.provider} onClick={() => dialog.openFor("ic")} />
      <WalletSummary side="base" value={isConnected ? address : undefined} onClick={() => dialog.openFor("base")} />
    </div>
    <WalletDialog />
  </>
}

function WalletSummary({ side, value, provider, onClick }: { side: WalletSide; value?: string; provider?: string; onClick: () => void }) {
  const connected = Boolean(value)
  const label = side === "ic" ? "IC wallet" : "Base wallet"
  const networkMark = side === "ic" ? "IC" : "B"
  const displayValue = value ? `${provider ? `${provider} · ` : ""}${short(value)}` : "Connect"
  return <button type="button" onClick={onClick} aria-label={connected ? `${label} connected as ${value}` : `Connect ${label}`} className="group relative flex h-12 min-w-[76px] items-center gap-2 rounded-2xl border border-[var(--line)] bg-white px-2 text-left transition duration-300 hover:-translate-y-0.5 hover:border-black focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[var(--focus)] sm:min-w-[142px] sm:px-3">
    <span className={`grid size-7 shrink-0 place-items-center rounded-full text-[10px] font-bold text-white ${side === "ic" ? "bg-[var(--pink)]" : "bg-[#0052ff]"}`}>{networkMark}</span>
    <span className="min-w-0 leading-none">
      <span className="block text-[9px] font-bold uppercase tracking-[.08em] text-[var(--muted)] sm:text-[10px]">{label}</span>
      <span className="mt-1 block max-w-[82px] truncate text-[11px] font-bold text-black sm:text-xs">{displayValue}</span>
    </span>
    {connected && <span className="absolute right-1.5 top-1.5 size-2 rounded-full border-2 border-white bg-[var(--success)]" aria-hidden="true" />}
  </button>
}

function WalletDialog() {
  const dialog = useWalletDialog()
  const { address, isConnected } = useAccount()
  const { connectors, connect, isPending } = useConnect()
  const { disconnect } = useDisconnect()
  const ic = useIcWallet()
  const connectIc = (provider: "oisy" | "plug") => void ic.connect(provider).catch((error: unknown) => toast.error(error instanceof Error ? error.message : String(error)))
  const target = dialog.target
  const title = target === "ic" ? "IC wallet" : target === "base" ? "Base wallet" : "Connect wallets"
  const description = target === "ic" ? "Connect OISY or Plug and verify the Principal used on Internet Computer." : target === "base" ? "Connect and verify the EVM address used on Base." : "Connect both sides of the bridge. You will verify them again before moving KINIC."
  return <Dialog open={dialog.open} onOpenChange={(open) => dialog.setOpen(open)}><DialogContent>
    <DialogHeader><div className="mb-3 flex items-center gap-3"><img src="/kinic-mark.png" alt="" className="size-10 object-contain" /><DialogTitle>{title}</DialogTitle></div><DialogDescription>{description}</DialogDescription></DialogHeader>
    <div className="mt-6 space-y-3">
      {(!target || target === "base") && <WalletRow label="Base wallet" value={isConnected && address ? short(address) : "Not connected"} connected={Boolean(isConnected && address)} action={isConnected ? <Button variant="ghost" size="icon" aria-label="Disconnect Base wallet" onClick={() => disconnect()}><LogOut className="size-4" /></Button> : <Button size="sm" disabled={isPending || !connectors[0]} onClick={() => connectors[0] && connect({ connector: connectors[0] })}><Cable className="size-4" />Connect Base</Button>} />}
      {(!target || target === "ic") && <WalletRow label="IC wallet" value={ic.account ? `${ic.provider} · ${short(ic.account.owner)}` : "Not connected"} connected={Boolean(ic.account)} action={ic.account ? <Button variant="ghost" size="icon" aria-label="Disconnect IC wallet" onClick={() => void ic.disconnect()}><LogOut className="size-4" /></Button> : <div className="flex gap-2"><Button variant="outline" size="sm" disabled={Boolean(ic.connecting)} onClick={() => connectIc("oisy")}>OISY</Button><Button variant="outline" size="sm" disabled={Boolean(ic.connecting)} onClick={() => connectIc("plug")}>Plug</Button></div>} />}
    </div>
  </DialogContent></Dialog>
}

function WalletRow({ label, value, connected, action }: { label: string; value: string; connected: boolean; action: ReactNode }) {
  return <div className="flex items-center justify-between gap-4 rounded-2xl bg-[var(--panel)] p-4"><div className="min-w-0"><p className="flex items-center gap-2 text-sm font-bold text-black">{label}{connected && <Check className="size-4 text-[var(--success)]" />}</p><p className="mt-1 truncate text-sm text-[var(--muted)]">{value}</p></div>{action}</div>
}
