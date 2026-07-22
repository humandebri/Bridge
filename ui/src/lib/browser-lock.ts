const localQueues = new Map<string, Promise<void>>()
const LOCK_PREFIX = "kinic.bridge.browser-lock.v1"
const LOCK_TTL_MS = 30_000
const LOCK_RENEW_MS = 10_000
const LOCK_RETRY_MS = 25

interface BrowserLease {
  ownerId: string
  expiresAt: number
  fencingToken: number
}

class BrowserLockStorageError extends Error {
  constructor(cause: unknown) {
    super("Browser lock storage is unavailable", { cause })
    this.name = "BrowserLockStorageError"
  }
}

/** Serializes wallet prompts and browser-storage read/modify/write operations. */
export async function withBrowserLock<T>(name: string, action: () => Promise<T> | T): Promise<T> {
  if (typeof navigator !== "undefined" && navigator.locks) {
    return navigator.locks.request(name, {}, () => action())
  }
  if (requiresCrossTabLock(name)) {
    throw new Error("Web Locks are required for this cross-tab operation")
  }
  if (typeof window === "undefined") return withLocalQueue(name, action)
  return withLocalStorageLock(name, action)
}

async function withLocalStorageLock<T>(name: string, action: () => Promise<T> | T): Promise<T> {
  const key = `${LOCK_PREFIX}:${name}`
  const ownerId = typeof crypto.randomUUID === "function"
    ? crypto.randomUUID()
    : `${Date.now()}-${Math.random()}`
  let lease: BrowserLease | undefined

  try {
    while (!lease) {
      const now = Date.now()
      const current = readLease(key)
      if (current === null) throw new Error("Browser lock storage is malformed")
      if (current && current.expiresAt > now) {
        await delay(LOCK_RETRY_MS)
        continue
      }
      const candidate: BrowserLease = {
        ownerId,
        expiresAt: now + LOCK_TTL_MS,
        fencingToken: Math.max(now, (current?.fencingToken ?? 0) + 1),
      }
      setLease(key, candidate)
      await delay(LOCK_RETRY_MS)
      if (ownsLease(readLease(key), candidate)) lease = candidate
    }
  } catch (error) {
    if (!(error instanceof BrowserLockStorageError)) throw error
    return withLocalQueue(name, action)
  }

  let lost = false
  const assertCurrent = () => {
    if (lost || !ownsLease(readLease(key), lease)) {
      lost = true
      throw new Error("Browser lock ownership was lost")
    }
  }
  const storageListener = (event: StorageEvent) => {
    if (event.key === key && !ownsLease(readLease(key), lease)) lost = true
  }
  window.addEventListener("storage", storageListener)
  const renewal = window.setInterval(() => {
    try {
      assertCurrent()
      lease.expiresAt = Date.now() + LOCK_TTL_MS
      setLease(key, lease)
    } catch {
      lost = true
    }
  }, LOCK_RENEW_MS)

  try {
    assertCurrent()
    const result = await action()
    assertCurrent()
    return result
  } finally {
    window.clearInterval(renewal)
    window.removeEventListener("storage", storageListener)
    if (ownsLease(readLease(key), lease)) removeLease(key)
  }
}

function requiresCrossTabLock(name: string): boolean {
  return name.startsWith("kinic-wallet-prompt:") || name.startsWith("kinic-deposit-owner:")
}

function readLease(key: string): BrowserLease | undefined | null {
  let raw: string | null
  try {
    raw = window.localStorage.getItem(key)
  } catch (error) {
    throw new BrowserLockStorageError(error)
  }
  if (raw === null) return undefined
  try {
    const value: unknown = JSON.parse(raw)
    if (typeof value !== "object" || value === null) return null
    const item = value as Record<string, unknown>
    return typeof item.ownerId === "string"
      && typeof item.expiresAt === "number"
      && Number.isSafeInteger(item.expiresAt)
      && typeof item.fencingToken === "number"
      && Number.isSafeInteger(item.fencingToken)
      ? { ownerId: item.ownerId, expiresAt: item.expiresAt, fencingToken: item.fencingToken }
      : null
  } catch {
    return null
  }
}

function ownsLease(current: BrowserLease | undefined | null, expected: BrowserLease): boolean {
  return current !== undefined
    && current !== null
    && current.ownerId === expected.ownerId
    && current.fencingToken === expected.fencingToken
    && current.expiresAt > Date.now()
}

function setLease(key: string, lease: BrowserLease): void {
  try {
    window.localStorage.setItem(key, JSON.stringify(lease))
  } catch (error) {
    throw new BrowserLockStorageError(error)
  }
}

function removeLease(key: string): void {
  try {
    window.localStorage.removeItem(key)
  } catch (error) {
    throw new BrowserLockStorageError(error)
  }
}

function delay(ms: number): Promise<void> {
  return new Promise((resolve) => window.setTimeout(resolve, ms))
}

async function withLocalQueue<T>(name: string, action: () => Promise<T> | T): Promise<T> {
  const previous = localQueues.get(name)
  if (!previous) {
    let release!: () => void
    const current = new Promise<void>((resolve) => { release = resolve })
    localQueues.set(name, current)
    try {
      return await action()
    } finally {
      release()
      if (localQueues.get(name) === current) localQueues.delete(name)
    }
  }
  let release!: () => void
  const current = new Promise<void>((resolve) => { release = resolve })
  const queued = previous.then(() => current)
  localQueues.set(name, queued)
  await previous
  try {
    return await action()
  } finally {
    release()
    if (localQueues.get(name) === queued) localQueues.delete(name)
  }
}
