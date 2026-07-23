import { Link, Outlet } from "@tanstack/react-router"
import { History, Menu, ShieldCheck } from "lucide-react"
import { WalletCenter, WalletDialogProvider } from "@/features/wallet/wallet-controls"
import { SettlementConfirmationCoordinator } from "@/features/bridge/settlement-confirmation-coordinator"
import blueKinic from "@/assets/blue_kinic.png"
import { deploymentProfile } from "@/config/profile"

export function AppShell() {
  return <WalletDialogProvider><SettlementConfirmationCoordinator /><div className="min-h-screen">
    {deploymentProfile.testOnly ? <div role="status" aria-label="Test deployment" className="border-b border-amber-300 bg-amber-100 px-4 py-2 text-center text-xs font-bold tracking-[.08em] text-amber-950">
      IC MAINNET × BASE SEPOLIA TEST — TEST ASSETS ONLY
    </div> : null}
    <header className="relative z-20 mx-auto flex max-w-[1155px] items-center gap-3 px-4 py-5 md:px-6 md:py-7">
      <Link to="/" search={{ direction: "deposit" }} className="group flex shrink-0 items-center gap-3 rounded-[13px] focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[var(--focus)] focus-visible:ring-offset-2" aria-label="KINIC Bridge home">
        <img src={blueKinic} alt="" className="size-11 rounded-[13px] object-cover shadow-[0_8px_24px_rgba(20,34,53,.12)] transition-transform duration-300 group-hover:-rotate-2 group-hover:scale-[1.03]" />
        <span className="hidden items-baseline gap-1.5 text-black sm:inline-flex"><strong className="text-lg font-bold tracking-[-.04em]">KINIC</strong><span className="text-[11px] font-bold uppercase tracking-[.16em] text-[var(--muted)]">Bridge</span></span>
      </Link>
      <div className="ml-auto flex min-w-0 items-center gap-2 md:gap-5">
        <nav className="hidden items-center gap-1 md:flex" aria-label="Secondary navigation">
          <Link to="/history" search={{ tab: "deposit" }} aria-label="Open history" className="rounded-xl px-3 py-2 text-sm font-semibold text-[var(--muted)] transition-colors hover:bg-[var(--panel)] hover:text-black focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[var(--focus)]"><span className="flex items-center gap-2"><History className="size-4" />History</span></Link>
          <Link to="/status" aria-label="Open status" className="rounded-xl px-3 py-2 text-sm font-semibold text-[var(--muted)] transition-colors hover:bg-[var(--panel)] hover:text-black focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[var(--focus)]"><span className="flex items-center gap-2"><ShieldCheck className="size-4" />Status</span></Link>
        </nav>
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
