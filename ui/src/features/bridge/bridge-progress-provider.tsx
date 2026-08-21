import { Check, ChevronUp, Circle, LoaderCircle, Minus, TriangleAlert } from "lucide-react"
import { createContext, useCallback, useContext, useMemo, useRef, useState, type ReactNode } from "react"
import { Button } from "@/components/ui/button"
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog"
import {
  bridgeProgressDetail,
  bridgeProgressLabel,
  bridgeProgressSteps,
  createBridgeProgress,
  isDepositTransactionComplete,
  readLatestBridgeProgress,
  removeLatestBridgeProgress,
  saveLatestBridgeProgress,
  type BridgeProgressRecord,
  withdrawalFinalityProgress,
} from "@/lib/bridge-progress"

interface ProgressAction {
  label: string
  run: () => void | Promise<void>
  pending?: boolean
}

interface RegisteredProgressAction extends ProgressAction {
  progressId: string
}

interface BridgeProgressContextValue {
  progress?: BridgeProgressRecord
  start: (input: Parameters<typeof createBridgeProgress>[0]) => BridgeProgressRecord
  update: (id: string, patch: Partial<Omit<BridgeProgressRecord, "id" | "version" | "createdAt">>) => void
  minimize: () => void
  restore: () => void
  dismiss: () => void
  setAction: (progressId: string, action?: ProgressAction) => void
  completeWithdrawal: (input: {
    transactionHash: `0x${string}`
    owner: string
    withdrawalId?: `0x${string}`
  }) => boolean
}

const BridgeProgressContext = createContext<BridgeProgressContextValue | undefined>(undefined)

export function BridgeProgressProvider({ children }: { children: ReactNode }) {
  const [restored] = useState(() => readLatestBridgeProgress())
  const [progress, setProgress] = useState<BridgeProgressRecord | undefined>(restored)
  const progressRef = useRef(progress)
  const [minimized, setMinimized] = useState(Boolean(restored))
  const [action, setAction] = useState<RegisteredProgressAction>()

  const setProgressAction = useCallback<BridgeProgressContextValue["setAction"]>((progressId, nextAction) => {
    setAction((current) => {
      if (!nextAction) return current?.progressId === progressId ? undefined : current
      if (progressRef.current?.id !== progressId) return current
      return { ...nextAction, progressId }
    })
  }, [])

  const start = useCallback<BridgeProgressContextValue["start"]>((input) => {
    if (progressRef.current) throw new Error("Complete or close the current transfer before starting another one")
    const next = createBridgeProgress(input)
    saveLatestBridgeProgress(next)
    progressRef.current = next
    setProgress(next)
    setMinimized(false)
    setAction(undefined)
    return next
  }, [])
  const update = useCallback<BridgeProgressContextValue["update"]>((id, patch) => {
    if (patch.phase === "complete" || patch.phase === "attention") setProgressAction(id, undefined)
    setProgress((current) => {
      if (!current || current.id !== id) return current
      const unchanged = Object.entries(patch).every(([key, value]) => current[key as keyof BridgeProgressRecord] === value)
      if (unchanged) return current
      const nextPhase = patch.phase ?? current.phase
      const attentionPhase = nextPhase === "attention"
        ? patch.attentionPhase ?? (current.phase === "attention" ? current.attentionPhase : current.phase === "complete" ? undefined : current.phase)
        : undefined
      const next = { ...current, ...patch, attentionPhase, updatedAt: Date.now() }
      saveLatestBridgeProgress(next)
      progressRef.current = next
      return next
    })
  }, [setProgressAction])
  const minimize = useCallback(() => setMinimized(true), [])
  const restore = useCallback(() => setMinimized(false), [])
  const dismiss = useCallback(() => {
    setProgress((current) => {
      if (current) removeLatestBridgeProgress(current.id)
      progressRef.current = undefined
      return undefined
    })
    setAction(undefined)
    setMinimized(false)
  }, [])
  const completeWithdrawal = useCallback<BridgeProgressContextValue["completeWithdrawal"]>((input) => {
    const current = progressRef.current
    if (current?.direction !== "withdraw"
      || current.transactionHash?.toLowerCase() !== input.transactionHash.toLowerCase()) return false
    if (current.phase !== "complete") update(current.id, {
      phase: "complete",
      withdrawal: { owner: input.owner, withdrawalId: input.withdrawalId },
      completionMessage: `${current.receiveAmount} ${current.receiveSymbol} was paid to ${shortDestination(current.destination)}.`,
    })
    return true
  }, [update])

  const value = useMemo<BridgeProgressContextValue>(() => ({
    progress,
    start,
    update,
    minimize,
    restore,
    dismiss,
    setAction: setProgressAction,
    completeWithdrawal,
  }), [completeWithdrawal, dismiss, minimize, progress, restore, setProgressAction, start, update])

  return <BridgeProgressContext.Provider value={value}>
    {children}
    {progress && minimized && <button
      type="button"
      onClick={() => setMinimized(false)}
      className="fixed bottom-5 right-4 z-50 flex max-w-[calc(100vw-2rem)] items-center gap-3 rounded-2xl border border-[#bfd7ff] bg-white px-4 py-3 text-left shadow-[0_18px_55px_rgba(20,34,53,.2)] transition hover:-translate-y-0.5 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[var(--focus)] sm:right-6"
      aria-label={`Open transfer progress: ${bridgeProgressLabel(progress)}`}
    >
      <span className={`grid size-9 shrink-0 place-items-center rounded-full ${progress.phase === "attention" ? "bg-[#fff0ec] text-[#b42318]" : progress.phase === "complete" ? "bg-[#eaf8ef] text-[#157347]" : "bg-[var(--pink-soft)] text-[var(--pink)]"}`}>
        {progress.phase === "attention" ? <TriangleAlert className="size-4" /> : progress.phase === "complete" ? <Check className="size-4" /> : <LoaderCircle className="size-4 animate-spin" />}
      </span>
      <span className="min-w-0"><span className="block text-xs font-semibold text-[var(--muted)]">{progress.direction === "deposit" ? "Bridge to Base" : "Bridge to IC"}</span><span className="block truncate text-sm font-bold text-black">{bridgeProgressLabel(progress)}</span></span>
      <ChevronUp className="size-4 shrink-0 text-[var(--muted)]" />
    </button>}
    {progress && !minimized && <ProgressDialog progress={progress} action={action} onMinimize={() => setMinimized(true)} onDismiss={value.dismiss} />}
  </BridgeProgressContext.Provider>
}

export function useBridgeProgress(): BridgeProgressContextValue {
  const value = useContext(BridgeProgressContext)
  if (!value) throw new Error("useBridgeProgress must be used inside BridgeProgressProvider")
  return value
}

function ProgressDialog({ progress, action, onMinimize, onDismiss }: { progress: BridgeProgressRecord; action?: ProgressAction; onMinimize: () => void; onDismiss: () => void }) {
  const canonicalTerminal = progress.phase === "complete" || progress.phase === "attention"
  const depositTransactionComplete = isDepositTransactionComplete(progress)
  const dismissible = canonicalTerminal || depositTransactionComplete
  const handleOutsidePointerDown = dismissible ? onDismiss : onMinimize
  const steps = bridgeProgressSteps(progress)
  const finalityProgress = withdrawalFinalityProgress(progress)
  return <Dialog open onOpenChange={(open) => { if (!open && dismissible) onDismiss() }}>
    <DialogContent
      dismissible={dismissible}
      aria-describedby={canonicalTerminal ? "bridge-progress-description" : undefined}
      onOverlayPointerDown={handleOutsidePointerDown}
      onPointerDownOutside={(event) => {
        event.preventDefault()
        handleOutsidePointerDown()
      }}
      className="max-h-[min(760px,calc(100vh-2rem))] max-w-[560px] overflow-y-auto"
    >
      <DialogHeader>
        <div className="flex items-start justify-between gap-4 pr-7">
          <div>
            <DialogTitle>{progress.direction === "deposit" ? "Bridge to Base" : "Bridge to IC"}</DialogTitle>
            {canonicalTerminal && <DialogDescription id="bridge-progress-description">Review the result below.</DialogDescription>}
          </div>
          {!dismissible && <Button size="sm" variant="ghost" onClick={onMinimize}><Minus className="size-4" />Minimize</Button>}
        </div>
      </DialogHeader>
      {progress.phase === "attention" && <div className="mt-5 rounded-2xl border border-[#ffbdad] bg-[#fff0ec] p-4" role="alert">
        <p className="font-bold text-black">{bridgeProgressLabel(progress)}</p>
        <p className="mt-1 text-sm leading-6 text-[var(--muted)]">{bridgeProgressDetail(progress)}</p>
      </div>}
      {progress.phase === "complete" && <div className="mt-5 rounded-2xl border border-[#9ed8b3] bg-[#eaf8ef] p-4" role="status">
        <p className="font-bold text-black">{bridgeProgressLabel(progress)}</p>
        <p className="mt-1 text-sm leading-6 text-[var(--muted)]">{bridgeProgressDetail(progress)}</p>
      </div>}
      <ol className="mt-5 space-y-1" aria-label="Transfer progress">
        {steps.map((step, index) => <li key={step.label} aria-current={step.status === "current" ? "step" : undefined} aria-label={depositTransactionComplete && step.label === "Base mint transaction" ? "Base mint transaction complete" : undefined} className="relative flex min-h-11 items-start gap-3">
          {index < steps.length - 1 && <span aria-hidden="true" className="absolute left-[15px] top-8 h-[calc(100%-1rem)] w-px bg-[var(--line)]" />}
          <span className={`relative z-10 grid size-8 shrink-0 place-items-center rounded-full border ${step.status === "complete" ? "border-[#9ed8b3] bg-[#eaf8ef] text-[#157347]" : step.status === "current" ? "border-[var(--pink)] bg-[var(--pink-soft)] text-[var(--pink)]" : "border-[var(--line)] bg-white text-[var(--muted)]"}`}>
            {step.status === "complete" ? <Check className="size-4" /> : step.status === "current" ? <LoaderCircle className="size-4 animate-spin" /> : <Circle className="size-3" />}
          </span>
          <span className={`min-w-0 pt-1 text-sm font-bold ${step.status === "waiting" ? "text-[var(--muted)]" : "text-black"}`}>
            <span className="block">{step.label}</span>
            {step.note && <span className="mt-0.5 block text-xs font-normal leading-5 text-[var(--muted)]">{step.note}</span>}
            {step.label === "Base finality" && step.status === "current" && <span className="mt-1 block text-xs font-normal leading-5 text-[var(--muted)]">
              <span className="block">Usually takes about 20 minutes.</span>
              {finalityProgress
                ? <>
                    <span className="block">Finalized block #{finalityProgress.finalizedBlockNumber} / Target block #{finalityProgress.targetBlockNumber}</span>
                    <span className="block">{finalityProgress.remainingBlocks} blocks remaining</span>
                  </>
                : <span className="block">Checking finalized block…</span>}
            </span>}
          </span>
        </li>)}
      </ol>
      <DialogFooter>
        {action && <Button disabled={action.pending} onClick={() => void action.run()}>{action.pending ? "Working…" : action.label}</Button>}
        {dismissible && <Button onClick={onDismiss}>Close</Button>}
      </DialogFooter>
    </DialogContent>
  </Dialog>
}

function shortDestination(value: string): string {
  return value.length > 18 ? `${value.slice(0, 10)}…${value.slice(-6)}` : value
}
