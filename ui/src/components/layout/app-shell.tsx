import { Link, Outlet } from "@tanstack/react-router"
import { History, Menu, ShieldCheck } from "lucide-react"
import { Badge } from "@/components/ui/badge"
import { deploymentProfile } from "@/config/profile"
import { WalletCenter, WalletDialogProvider } from "@/features/wallet/wallet-controls"

export function AppShell() {
  return <WalletDialogProvider><div className="min-h-screen">
    <header className="relative z-20 mx-auto flex max-w-[1155px] items-center justify-between px-4 py-5 md:px-6 md:py-6">
      <Link to="/" search={{ direction: "deposit" }} className="group flex items-center gap-3" aria-label="KINIC Bridge home">
        <img src="/kinic-mark.png" alt="" className="size-11 object-contain transition-transform duration-300 group-hover:scale-96" />
        <span className="hidden text-lg font-bold tracking-[-.02em] text-black sm:inline">KINIC Bridge</span>
      </Link>
      <div className="flex items-center gap-2">
        {!deploymentProfile.writeEnabled && <Badge tone="warn" className="hidden sm:inline-flex">Read-only</Badge>}
        <WalletCenter />
        <details className="menu-popover relative md:hidden">
          <summary className="grid size-11 cursor-pointer list-none place-items-center rounded-2xl border border-[var(--line)] bg-white transition duration-300 hover:-translate-y-[3px] hover:border-[var(--pink)] hover:bg-[var(--pink)] hover:text-white focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[var(--focus)]" aria-label="Open navigation menu"><Menu className="size-5" /></summary>
          <nav className="absolute right-0 top-14 w-48 rounded-2xl border border-[var(--line)] bg-white p-2 shadow-[0_12px_36px_rgba(0,0,0,.08)]" aria-label="Secondary navigation">
            <Link to="/history" search={{ tab: "deposit" }} className="flex items-center gap-3 rounded-xl px-3 py-3 text-sm font-medium hover:bg-[var(--panel)] hover:text-[var(--pink)]"><History className="size-4" />History</Link>
            <Link to="/status" className="flex items-center gap-3 rounded-xl px-3 py-3 text-sm font-medium hover:bg-[var(--panel)] hover:text-[var(--pink)]"><ShieldCheck className="size-4" />Status</Link>
          </nav>
        </details>
      </div>
    </header>
    <main className="mx-auto max-w-[1155px] px-4 pb-20 md:px-6"><Outlet /></main>
    <footer className="mx-auto flex max-w-[1155px] flex-col gap-2 border-t border-[var(--panel)] px-4 py-8 text-sm text-[var(--muted)] sm:flex-row sm:justify-between md:px-6">
      <span>KINIC moves 1:1 across IC and Base.</span><span>Verify every account, amount, and wallet prompt.</span>
    </footer>
  </div></WalletDialogProvider>
}
