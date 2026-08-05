import { Check, ChevronUp, Circle, LoaderCircle, Minus, TriangleAlert } from "lucide-react"
import { createContext, useCallback, useContext, useMemo, useRef, useState, type ReactNode } from "react"
import { Button } from "@/components/ui/button"
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog"
import {
  bridgeProgressDetail,
  bridgeProgressLabel,
  bridgeProgressSteps,
  createBridgeProgress,
  readLatestBridgeProgress,
  removeLatestBridgeProgress,
  saveLatestBridgeProgress,
  type BridgeProgressRecord,
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
      const next = { ...current, ...patch, updatedAt: Date.now() }
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

  const value = useMemo<BridgeProgressContextValue>(() => ({
    progress,
    start,
    update,
    minimize,
    restore,
    dismiss,
    setAction: setProgressAction,
  }), [dismiss, minimize, progress, restore, setProgressAction, start, update])

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
  const terminal = progress.phase === "complete" || progress.phase === "attention"
  const steps = bridgeProgressSteps(progress)
  return <Dialog open onOpenChange={(open) => { if (!open && terminal) onDismiss() }}>
    <DialogContent dismissible={terminal} className="max-h-[min(760px,calc(100vh-2rem))] max-w-[560px] overflow-y-auto">
      <DialogHeader>
        <div className="flex items-start justify-between gap-4 pr-7">
          <div>
            <DialogTitle>{progress.direction === "deposit" ? "Bridge to Base" : "Bridge to IC"}</DialogTitle>
            <DialogDescription>{terminal ? "Review the result below." : "Keep this window open, or minimize it while the transfer continues."}</DialogDescription>
          </div>
          {!terminal && <Button size="sm" variant="ghost" onClick={onMinimize}><Minus className="size-4" />Minimize</Button>}
        </div>
      </DialogHeader>
      <div className={`mt-5 rounded-2xl border p-4 ${progress.phase === "attention" ? "border-[#ffbdad] bg-[#fff0ec]" : progress.phase === "complete" ? "border-[#9ed8b3] bg-[#eaf8ef]" : "border-[#bfd7ff] bg-[#eef5ff]"}`} role="status" aria-live="polite">
        <p className="font-bold text-black">{bridgeProgressLabel(progress)}</p>
        <p className="mt-1 text-sm leading-6 text-[var(--muted)]">{bridgeProgressDetail(progress)}</p>
        {progress.transactionHash && <p className="mt-2 break-all font-mono text-xs text-[#335f9d]">{progress.transactionHash}</p>}
      </div>
      <ol className="mt-5 space-y-1" aria-label="Transfer progress">
        {steps.map((step, index) => <li key={step.label} className="relative flex min-h-11 items-start gap-3">
          {index < steps.length - 1 && <span aria-hidden="true" className="absolute left-[15px] top-8 h-[calc(100%-1rem)] w-px bg-[var(--line)]" />}
          <span className={`relative z-10 grid size-8 shrink-0 place-items-center rounded-full border ${step.status === "complete" ? "border-[#9ed8b3] bg-[#eaf8ef] text-[#157347]" : step.status === "current" ? "border-[var(--pink)] bg-[var(--pink-soft)] text-[var(--pink)]" : "border-[var(--line)] bg-white text-[var(--muted)]"}`}>
            {step.status === "complete" ? <Check className="size-4" /> : step.status === "current" ? <LoaderCircle className="size-4 animate-spin" /> : <Circle className="size-3" />}
          </span>
          <span className={`pt-1 text-sm font-bold ${step.status === "waiting" ? "text-[var(--muted)]" : "text-black"}`}>{step.label}</span>
        </li>)}
      </ol>
      <div className="mt-5 grid gap-3 rounded-2xl bg-[var(--panel)] p-4 text-sm sm:grid-cols-2">
        <ProgressFact label="Send" value={`${progress.sendAmount} ${progress.sendSymbol}`} />
        <ProgressFact label="Receive" value={`${progress.receiveAmount} ${progress.receiveSymbol}`} />
        <ProgressFact label="From" value={progress.source} />
        <ProgressFact label="To" value={progress.destination} />
      </div>
      <DialogFooter>
        {action && <Button disabled={action.pending} onClick={() => void action.run()}>{action.pending ? "Working…" : action.label}</Button>}
        {terminal && <Button onClick={onDismiss}>Close</Button>}
      </DialogFooter>
    </DialogContent>
  </Dialog>
}

function ProgressFact({ label, value }: { label: string; value: string }) {
  return <div className="min-w-0"><p className="text-xs text-[var(--muted)]">{label}</p><p className="mt-1 break-all font-bold text-black">{value}</p></div>
}
