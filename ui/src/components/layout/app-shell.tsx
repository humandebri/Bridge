import { Link, Outlet } from "@tanstack/react-router"
import { BookOpen, History, Menu, ShieldCheck } from "lucide-react"
import { WalletCenter, WalletDialogProvider } from "@/features/wallet/wallet-controls"
import { SettlementConfirmationCoordinator } from "@/features/bridge/settlement-confirmation-coordinator"
import { BridgeProgressProvider } from "@/features/bridge/bridge-progress-provider"
import { DepositProgressCoordinator } from "@/features/bridge/deposit-progress-coordinator"
import { RiskAcknowledgementDialog } from "@/features/risk/risk-acknowledgement"
import blueKinic from "@/assets/blue_kinic.png"
import openChatLogo from "@/assets/openchat-logo.svg"
import { deploymentProfile } from "@/config/profile"

function XBrandIcon({ className }: { className?: string }) {
  return <svg aria-hidden="true" className={className} focusable="false" viewBox="0 0 24 24" fill="#000000">
    <path d="M18.244 2.25h3.308l-7.227 8.26 8.502 11.24h-6.657l-5.214-6.817-5.967 6.817H1.68l7.73-8.835L1.254 2.25h6.826l4.713 6.231 5.45-6.231Zm-1.161 17.52h1.833L7.084 4.126H5.117L17.083 19.77Z" />
  </svg>
}

export function AppShell() {
  return <WalletDialogProvider><BridgeProgressProvider><RiskAcknowledgementDialog /><DepositProgressCoordinator /><SettlementConfirmationCoordinator /><div className="flex min-h-screen flex-col">
    {deploymentProfile.testOnly ? <div role="status" aria-label="Test deployment" className="border-b border-amber-300 bg-amber-100 px-4 py-2 text-center text-xs font-bold tracking-[.08em] text-amber-950">
      IC MAINNET × BASE SEPOLIA TEST — TEST ASSETS ONLY
      {deploymentProfile.environmentMode === "short-delay-test-only"
        ? " — 5-MINUTE TIMELOCK"
        : null}
    </div> : null}
    <header className="relative z-20 mx-auto flex w-full max-w-[1155px] items-center gap-3 px-4 py-5 md:px-6 md:py-7">
      <Link to="/" search={{ direction: "deposit" }} className="group flex shrink-0 items-center gap-3 rounded-[13px] focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[var(--focus)] focus-visible:ring-offset-2" aria-label="KINIC Bridge home">
        <img src={blueKinic} alt="" className="size-11 rounded-[13px] object-cover shadow-[0_8px_24px_rgba(20,34,53,.12)] transition-transform duration-300 group-hover:-rotate-2 group-hover:scale-[1.03]" />
        <span className="hidden text-lg font-bold text-black sm:inline">KINIC Bridge</span>
      </Link>
      <div className="ml-auto flex min-w-0 items-center gap-2 md:gap-5">
        <nav className="hidden items-center gap-1 md:flex" aria-label="Secondary navigation">
          <Link to="/history" aria-label="Open history" className="rounded-xl px-3 py-2 text-sm font-semibold text-[var(--muted)] transition-colors hover:bg-[var(--panel)] hover:text-black focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[var(--focus)]"><span className="flex items-center gap-2"><History className="size-4" />History</span></Link>
          <Link to="/status" aria-label="Open status" className="rounded-xl px-3 py-2 text-sm font-semibold text-[var(--muted)] transition-colors hover:bg-[var(--panel)] hover:text-black focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[var(--focus)]"><span className="flex items-center gap-2"><ShieldCheck className="size-4" />Status</span></Link>
        </nav>
        <WalletCenter />
        <details className="menu-popover relative md:hidden">
          <summary className="grid size-11 cursor-pointer list-none place-items-center rounded-2xl border border-[var(--line)] bg-white transition duration-300 hover:-translate-y-[3px] hover:border-[var(--pink)] hover:bg-[var(--pink)] hover:text-white focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[var(--focus)]" aria-label="Open navigation menu"><Menu className="size-5" /></summary>
          <nav className="absolute right-0 top-14 w-48 rounded-2xl border border-[var(--line)] bg-white p-2 shadow-[0_12px_36px_rgba(0,0,0,.08)]" aria-label="Secondary navigation">
            <Link to="/history" className="flex items-center gap-3 rounded-xl px-3 py-3 text-sm font-medium hover:bg-[var(--panel)] hover:text-[var(--pink)]"><History className="size-4" />History</Link>
            <Link to="/status" className="flex items-center gap-3 rounded-xl px-3 py-3 text-sm font-medium hover:bg-[var(--panel)] hover:text-[var(--pink)]"><ShieldCheck className="size-4" />Status</Link>
          </nav>
        </details>
      </div>
    </header>
    <main className="mx-auto w-full max-w-[1155px] flex-1 px-4 md:px-6"><Outlet /></main>
    <footer className="mx-auto flex w-full max-w-[1155px] items-center justify-center border-t border-[var(--panel)] px-4 py-8 text-sm text-[var(--muted)] md:px-6">
      <nav aria-label="KINIC links" className="flex flex-wrap items-center justify-center gap-1">
        <a href="https://wiki.kinic.xyz/" target="_blank" rel="noopener noreferrer" className="flex min-h-10 items-center gap-2 rounded-xl px-3 py-2 font-semibold transition-colors hover:bg-[var(--pink-soft)] hover:text-[var(--pink)] focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[var(--focus)]" title="Wiki">
          <BookOpen aria-hidden="true" className="size-4" />
          <span>Wiki</span>
        </a>
        <a href="https://x.com/kinic_app" target="_blank" rel="noopener noreferrer" aria-label="KINIC on X" title="X" className="grid size-10 place-items-center rounded-xl transition-colors hover:bg-[var(--pink-soft)] hover:text-[var(--pink)] focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[var(--focus)]">
          <XBrandIcon className="size-4" />
        </a>
        <a href="https://oc.app/community/rqdzm-qaaaa-aaaar-ar3na-cai/channel/3004043573" target="_blank" rel="noopener noreferrer" aria-label="KINIC OpenChat community" title="OpenChat" className="grid size-10 place-items-center rounded-xl transition-colors hover:bg-[var(--pink-soft)] hover:text-[var(--pink)] focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[var(--focus)]">
          <img src={openChatLogo} alt="" aria-hidden="true" className="size-5" />
        </a>
      </nav>
    </footer>
  </div></BridgeProgressProvider></WalletDialogProvider>
}
