import { mintRecoveryDiagnostics } from "./mint-recovery-api"
import { browserLocalStorage } from "./browser-lock"

export type MintExecutionPhase =
  | "idle"
  | "checking-ic"
  | "checking-base"
  | "simulating"
  | "wallet"
  | "submitted"
  | "failed"
  | "rejected"
  | "unknown"
export interface MintExecutionState {
  phase: MintExecutionPhase
  attemptId?: string
  message?: string
  transactionHash?: `0x${string}`
}
export interface MintExecutionContext {
  signal: AbortSignal
  check: () => void
  stage: (phase: "checking-ic" | "checking-base" | "simulating") => void
}
export interface MintExecutionRequest<T> {
  key: string
  wallet: string
  source: "automatic" | "manual"
  connected: () => boolean
  readPending: () => `0x${string}` | undefined
  prepare: (context: MintExecutionContext) => Promise<T>
  beforeWallet: (prepared: T) => void
  send: (prepared: T) => Promise<`0x${string}`>
  save: (hash: `0x${string}`) => Promise<void>
}
interface Entry {
  state: MintExecutionState
  promise?: Promise<void>
  controller?: AbortController
}
interface Diagnostic {
  attemptId: string
  source: "automatic" | "manual"
  stage: MintExecutionPhase
  event: "start" | "end"
  at: number
  elapsedMs: number
  reason?: string
}
const idle: MintExecutionState = { phase: "idle" }
const prefix = "kinic.bridge.mint-attempt.v1:"
const entries = new Map<string, Entry>()
const listeners = new Set<() => void>()
const diagnostics: Diagnostic[] = []
const automaticAttempts = new Set<string>()
const emit = () => listeners.forEach((listener) => listener())

export function subscribeMintExecution(listener: () => void): () => void {
  listeners.add(listener)
  return () => {
    listeners.delete(listener)
  }
}
export function mintExecutionSnapshot(key: string): MintExecutionState {
  return entries.get(key)?.state ?? idle
}
export function mintExecutionDiagnostics(): string {
  return JSON.stringify({ execution: diagnostics, recovery: mintRecoveryDiagnostics() }, null, 2)
}
export function mintExecutionBusy(state: MintExecutionState): boolean {
  return ["checking-ic", "checking-base", "simulating", "wallet"].includes(state.phase)
}
export function mintExecutionMessage(state: MintExecutionState): string | undefined {
  const labels: Partial<Record<MintExecutionPhase, string>> = {
    "checking-ic": "Checking IC state…",
    "checking-base": "Checking mint on Base…",
    simulating: "Simulating mint on Base…",
    wallet: "Confirm in your Base wallet (Rabby if connected).",
    unknown:
      "Submission result unknown. Check your wallet for the transaction status. No automatic retry will be sent.",
  }
  return state.message ?? labels[state.phase]
}
export function recordRecoveredMint(key: string, transactionHash: `0x${string}`): void {
  const entry = entries.get(key)
  if (entry?.promise || entry?.state.transactionHash === transactionHash) return
  const state: MintExecutionState = { phase: "submitted", transactionHash }
  entries.set(key, { state })
  try {
    persist(key, state)
  } catch {
    /* Canonical pending storage already holds the hash. */
  }
  emit()
}

/** Called only after the existing finalized-revert recovery has authorized a retry. */
export function releaseFinalizedMintAttempt(key: string): void {
  if (entries.get(key)?.promise) throw new Error("Mint operation is still running.")
  browserLocalStorage().removeItem(prefix + key)
  entries.delete(key)
  automaticAttempts.delete(key)
  emit()
}

export function cancelMintPreflight(key: string): void {
  entries.get(key)?.controller?.abort(new Error("Preflight cancelled. No wallet request was sent."))
}

/** Restore only before starting; snapshots must stay referentially stable for React. */
export function restoreMintExecution(key: string): void {
  if (entries.has(key)) return
  try {
    const raw = browserLocalStorage().getItem(prefix + key)
    if (raw === null) return
    const value: unknown = JSON.parse(raw)
    if (
      typeof value === "object" &&
      value !== null &&
      "phase" in value &&
      value.phase === "rejected"
    )
      return
    const hash =
      typeof value === "object" && value !== null && "transactionHash" in value
        ? value.transactionHash
        : undefined
    const state: MintExecutionState =
      typeof hash === "string" && /^0x[0-9a-fA-F]{64}$/.test(hash)
        ? { phase: "submitted", transactionHash: hash as `0x${string}` }
        : { phase: "unknown" }
    entries.set(key, { state })
  } catch {
    entries.set(key, {
      state: {
        phase: "unknown",
        message: "Mint recovery storage is unavailable. Restore access before continuing.",
      },
    })
  }
  emit()
}
function persist(key: string, state: MintExecutionState): void {
  browserLocalStorage().setItem(prefix + key, JSON.stringify(state))
}
function rejectedByUser(error: unknown): boolean {
  const seen = new Set<unknown>()
  let value = error
  while (value && typeof value === "object" && !seen.has(value)) {
    seen.add(value)
    if ("code" in value && value.code === 4001) return true
    value = "cause" in value ? value.cause : undefined
  }
  return false
}

/** One promise per authorization; Web Locks prevent overlapping prompts across tabs. */
export function startMintExecution<T>(request: MintExecutionRequest<T>): Promise<void> {
  restoreMintExecution(request.key)
  const existing = entries.get(request.key)
  if (existing?.promise) return existing.promise
  if (existing?.state.phase === "unknown" || existing?.state.phase === "submitted")
    return Promise.resolve()
  if (request.source === "automatic" && automaticAttempts.has(request.key)) return Promise.resolve()
  automaticAttempts.add(request.key)
  const attemptId = crypto.randomUUID()
  const start = performance.now()
  const entry: Entry = { state: { phase: "checking-ic", attemptId } }
  entries.set(request.key, entry)
  function record(stage: MintExecutionPhase, event: "start" | "end", reason?: string) {
    diagnostics.push({
      attemptId,
      source: request.source,
      stage,
      event,
      at: Date.now(),
      elapsedMs: Math.round(performance.now() - start),
      reason,
    })
    if (diagnostics.length > 100) diagnostics.splice(0, diagnostics.length - 100)
  }
  function update(state: MintExecutionState, reason?: string) {
    record(entry.state.phase, "end", reason)
    entry.state = { ...state, attemptId }
    record(state.phase, "start")
    emit()
  }
  record("checking-ic", "start")
  // Defer execution until the shared promise is installed, including synchronous failures.
  entry.promise = Promise.resolve()
    .then(async () => {
      if (typeof navigator === "undefined" || !navigator.locks)
        throw new Error("Web Locks are required for minting.")
      await navigator.locks.request(
        `kinic-wallet-prompt:base:${request.wallet.toLowerCase()}`,
        { ifAvailable: true },
        async (lock) => {
          if (!lock)
            throw new Error("Another wallet operation is in progress. Retry after it finishes.")
          await navigator.locks.request(
            `kinic-mint-attempt:${request.key}`,
            { ifAvailable: true },
            async (depositLock) => {
              if (!depositLock)
                throw new Error("This deposit is already being minted in another tab.")
              const savedHash = request.readPending()
              if (savedHash) {
                update({ phase: "submitted", transactionHash: savedHash })
                return
              }
              // A different tab may have submitted or lost its wallet response before releasing the lock.
              const stored = browserLocalStorage().getItem(prefix + request.key)
              if (stored !== null) {
                const saved = JSON.parse(stored) as MintExecutionState
                if (saved.phase !== "rejected") {
                  update(
                    saved.phase === "submitted" &&
                      /^0x[0-9a-fA-F]{64}$/.test(saved.transactionHash ?? "")
                      ? saved
                      : { phase: "unknown" },
                  )
                  return
                }
              }
              const controller = new AbortController()
              const expiresAt = performance.now() + 30_000
              entry.controller = controller
              const check = () => {
                if (performance.now() >= expiresAt)
                  controller.abort(
                    new Error("Preflight timed out after 30 seconds. No wallet request was sent."),
                  )
                controller.signal.throwIfAborted()
                if (entries.get(request.key) !== entry || !request.connected())
                  throw new Error(
                    "Wallet connection changed. Retry with the intended wallet on Base.",
                  )
              }
              let abortListener: () => void = () => {}
              const aborted = new Promise<never>((_, reject) => {
                abortListener = () => reject(controller.signal.reason)
                controller.signal.addEventListener("abort", abortListener, { once: true })
              })
              // A synchronous preflight rejection can happen before Promise.race subscribes.
              void aborted.catch(() => {})
              const timer = setTimeout(
                () =>
                  controller.abort(
                    new Error(
                      `Preflight timed out after 30 seconds (${entry.state.phase}). No wallet request was sent.`,
                    ),
                  ),
                30_000,
              )
              let prepared: T
              try {
                check()
                prepared = await Promise.race([
                  request.prepare({
                    signal: controller.signal,
                    check,
                    stage: (phase) => {
                      check()
                      update({ phase })
                    },
                  }),
                  aborted,
                ])
                check()
                request.beforeWallet(prepared)
                persist(request.key, { phase: "wallet", attemptId })
                check()
              } catch (error) {
                controller.abort(error)
                throw error
              } finally {
                clearTimeout(timer)
                controller.signal.removeEventListener("abort", abortListener)
                entry.controller = undefined
              }
              update({ phase: "wallet" })
              try {
                const hash = await request.send(prepared)
                const submitted: MintExecutionState = {
                  phase: "submitted",
                  transactionHash: hash,
                  attemptId,
                }
                let storedHash = false
                try {
                  persist(request.key, submitted)
                  storedHash = true
                } catch {
                  /* Try the canonical pending record too. */
                }
                try {
                  await request.save(hash)
                  storedHash = true
                } catch {
                  /* Keep the hash in the shared state. */
                }
                update(submitted)
                if (!storedHash)
                  update(
                    {
                      ...entry.state,
                      message:
                        "Transaction sent, but storage failed. Copy the transaction hash before leaving this page.",
                    },
                    "storage-failed",
                  )
              } catch (error) {
                if (rejectedByUser(error)) {
                  try {
                    persist(request.key, { phase: "rejected", attemptId })
                    update(
                      {
                        phase: "rejected",
                        message: "Wallet request rejected. You can retry manually.",
                      },
                      "user-rejected",
                    )
                  } catch {
                    update({ phase: "unknown" }, "storage-failed")
                  }
                } else {
                  update({ phase: "unknown" }, "wallet-response-unknown")
                }
              }
            },
          )
        },
      )
    })
    .catch((error) => {
      update(
        {
          phase: "failed",
          message:
            error instanceof Error ? error.message : "Mint preflight failed. Retry manually.",
        },
        "preflight-failed",
      )
    })
    .finally(() => {
      record(entry.state.phase, "end")
      entry.promise = undefined
      emit()
    })
  emit()
  return entry.promise
}
