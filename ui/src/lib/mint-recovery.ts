import { Principal } from "@icp-sdk/core/principal"
import { decodeEventLog, hexToBytes, toHex, type Hex } from "viem"
import { z } from "zod"
import { deploymentProfile } from "@/config/profile"
import type { DepositView } from "@/generated/bridge.did"
import { bridgeAbi } from "@/generated/abi/bridge.generated"
import { browserLocalStorage, withBrowserLock } from "@/lib/browser-lock"
import { baseHistoryClients, firstSuccessfulHistoryClient } from "@/lib/evm/client"
import { createBridgeActor } from "@/lib/ic/bridge"
import { readBaseBlock } from "@/lib/base-transaction-observation"
import { observeMint, type MintObservation } from "@/lib/mint-observation"
import {
  readAllPendingMints,
  readPendingMint,
  removePendingMint,
  savePendingMint,
} from "@/lib/pending-confirmations"
import { exactMintReceiptFinalization } from "@/lib/deposit-mint-finalization"

import {
  MINT_RECOVERY_URL,
  recoveryPageSchema,
  recordMintRecoveryDiagnostic,
} from "./mint-recovery-api"

const hex32 = z.string().regex(/^0x[0-9a-f]{64}$/i)
const nat = z.string().regex(/^(0|[1-9][0-9]*)$/)
const searchSchema = z.object({
  hashes: z.array(hex32).max(100),
  seen: z.array(hex32),
  cursor: z.string().nullable(),
  nextSearchAt: z.number(),
})
const targetSchema = z.object({
  depositId: hex32,
  digest: hex32,
  owner: z.string().optional(),
  walletRequested: z.boolean(),
  search: searchSchema.optional(),
  conflict: z.boolean().optional(),
})
type RecoveryTarget = z.infer<typeof targetSchema>
const prefix = [
  "kinic.bridge.mint-recovery.v1",
  deploymentProfile.chainId,
  deploymentProfile.bridgeAddress?.toLowerCase(),
  deploymentProfile.bridgeCanisterId,
  deploymentProfile.deploymentInstanceId?.toLowerCase(),
  "",
].join(":")
const session = new Map<string, RecoveryTarget>()
const keyOf = (id: string, digest: string) => `${prefix}${id.toLowerCase()}:${digest.toLowerCase()}`
const hex = (bytes: Uint8Array | number[]) => toHex(Uint8Array.from(bytes))

export function readMintRecoveryTargets(): RecoveryTarget[] {
  const values = new Map(session)
  try {
    const storage = browserLocalStorage()
    for (let i = 0; i < storage.length; i++) {
      const key = storage.key(i)
      if (!key?.startsWith(prefix) || values.has(key)) continue
      try {
        const value = targetSchema.parse(JSON.parse(storage.getItem(key)!))
        if (key === keyOf(value.depositId, value.digest)) values.set(key, value)
      } catch {
        /* Ignore malformed or foreign recovery records. */
      }
    }
  } catch {
    /* The current session can still recover from IC history. */
  }
  return [...values.values()]
}

function persist(target: RecoveryTarget): boolean {
  const key = keyOf(target.depositId, target.digest)
  session.set(key, target)
  try {
    browserLocalStorage().setItem(key, JSON.stringify(target))
    session.delete(key)
    return true
  } catch {
    return false
  }
}

export function wasMintRequested(record: DepositView): boolean {
  const a = record.mint_authorization[0]
  return (
    !!a &&
    readMintRecoveryTargets().some(
      (t) =>
        t.depositId === hex(record.deposit_id) && t.digest === hex(a.digest) && t.walletRequested,
    )
  )
}

export async function rememberMintRecovery(
  record: DepositView,
  owner?: string,
  walletRequested = false,
): Promise<boolean> {
  const a = record.mint_authorization[0]
  if (!a) return false
  const depositId = hex(record.deposit_id),
    digest = hex(a.digest)
  return withBrowserLock(`kinic-mint-recovery:${keyOf(depositId, digest)}`, () => {
    const old = readMintRecoveryTargets().find(
      (t) => t.depositId === depositId && t.digest === digest,
    )
    return persist({
      ...old,
      depositId,
      digest,
      owner: owner ?? old?.owner,
      walletRequested: walletRequested || old?.walletRequested || false,
    })
  })
}

export function isRecoverableMint(record: DepositView): boolean {
  return (
    !!record.mint_authorization[0]?.signature[0] &&
    !record.mint_receipt.length &&
    ("AuthorizationAvailable" in record.state || "RefundAvailable" in record.state)
  )
}

/** One IC page per turn; the caller retains its cursor until all older records were inspected. */
export async function discoverMintRecovery(
  owner: string,
  before?: bigint,
): Promise<bigint | undefined> {
  const actor = await createBridgeActor(
    deploymentProfile.icHost,
    deploymentProfile.bridgeCanisterId!,
  )
  const page = await actor.list_deposit_ids({
    owner: Principal.fromText(owner),
    before_cursor: before === undefined ? [] : [before],
    limit: 20,
  })
  if ("Err" in page) throw new Error("Deposit recovery history is unavailable")
  for (const id of page.Ok.deposit_ids) {
    const record = (await actor.get_deposit(id))[0]
    if (record && isRecoverableMint(record)) await rememberMintRecovery(record, owner)
  }
  return page.Ok.next_cursor[0]
}

export async function recoverMint(target: RecoveryTarget): Promise<MintObservation> {
  return withBrowserLock(
    `kinic-mint-recovery:${keyOf(target.depositId, target.digest)}`,
    async () => {
      const current = readMintRecoveryTargets().find(
        (t) => t.depositId === target.depositId && t.digest === target.digest,
      )
      if (!current) return { status: "unsubmitted", finalized: false, recorded: false }
      const actor = await createBridgeActor(
        deploymentProfile.icHost,
        deploymentProfile.bridgeCanisterId!,
      )
      const record = (await actor.get_deposit(hexToBytes(target.depositId as Hex)))[0]
      if (!record) throw new Error("Deposit recovery record is unavailable")
      const a = record.mint_authorization[0]
      if (!a || hex(a.digest) !== target.digest)
        return { status: "conflict", finalized: false, recorded: false }
      if (record.mint_receipt.length || "Refunded" in record.state) {
        session.delete(keyOf(target.depositId, target.digest))
        browserLocalStorage().removeItem(keyOf(target.depositId, target.digest))
        return {
          status: record.mint_receipt.length ? "success" : "unsubmitted",
          finalized: true,
          recorded: !!record.mint_receipt.length,
        }
      }
      if (!isRecoverableMint(record))
        return { status: "unsubmitted", finalized: false, recorded: false }
      const expected = {
        depositId: hex(record.deposit_id),
        authorizationDigest: hex(a.digest),
        recipient: hex(a.recipient),
        grossAmount: a.gross_amount,
        serviceFee: a.charged_service_fee,
        mintedAmount: a.gross_amount - a.charged_service_fee,
      }
      const pendingExpected = {
        depositId: expected.depositId,
        authorizationDigest: expected.authorizationDigest,
        recipient: expected.recipient,
        grossAmount: expected.grossAmount.toString(),
        chargedServiceFee: expected.serviceFee.toString(),
        mintedAmount: expected.mintedAmount.toString(),
      }
      const pending = readPendingMint(pendingExpected)
      if (pending) return observeMint(pending)
      if (current.conflict) return { status: "conflict", finalized: false, recorded: false }
      if (
        deploymentProfile.chainId !== 8453 ||
        deploymentProfile.mintRecoveryUrl !== MINT_RECOVERY_URL
      )
        return { status: "unsubmitted", finalized: false, recorded: false }
      let search = current.search ?? { hashes: [], seen: [], cursor: null, nextSearchAt: 0 }
      if (!search.hashes.length) {
        if (Date.now() < search.nextSearchAt)
          return { status: "unsubmitted", finalized: false, recorded: false }
        if (!search.cursor) search = { ...search, seen: [] }
        const response = await fetch(MINT_RECOVERY_URL, {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          signal: AbortSignal.timeout(20_000),
          body: JSON.stringify({
            depositId: target.depositId,
            ...(search.cursor ? { cursor: search.cursor } : {}),
          }),
        })
        if (response.status === 410) {
          persist({
            ...current,
            search: { hashes: [], seen: [], cursor: null, nextSearchAt: Date.now() + 10_000 },
          })
          return { status: "unsubmitted", finalized: false, recorded: false }
        }
        if (!response.ok) throw new Error("Mint recovery service is unavailable")
        const page = recoveryPageSchema.parse(await response.json())
        if (
          page.deploymentInstanceId.toLowerCase() !==
            deploymentProfile.deploymentInstanceId?.toLowerCase() ||
          page.depositId.toLowerCase() !== target.depositId ||
          page.authorizationDigest.toLowerCase() !== target.digest
        )
          throw new Error("Mint recovery deployment binding mismatch")
        recordMintRecoveryDiagnostic("page", page.hashes.length)
        search = {
          ...search,
          hashes: [...new Set(page.hashes.map((h) => h.toLowerCase()))].filter(
            (h) => !search.seen.includes(h),
          ),
          cursor: page.cursor,
          nextSearchAt: 0,
        }
        persist({ ...current, search })
      }
      for (const hash of search.hashes.slice(0, 4)) {
        const receipt = await firstSuccessfulHistoryClient(baseHistoryClients, (client) =>
          client.getTransactionReceipt({ hash: hash as Hex }),
        )
        const sameDeposit = receipt.logs.some((log) => {
          if (log.address.toLowerCase() !== deploymentProfile.bridgeAddress?.toLowerCase())
            return false
          try {
            const event = decodeEventLog({
              abi: bridgeAbi,
              eventName: "DepositMinted",
              data: log.data,
              topics: log.topics,
              strict: true,
            })
            return event.args.depositId.toLowerCase() === target.depositId
          } catch {
            return false
          }
        })
        if (sameDeposit) {
          const finalized = await readBaseBlock("finalized")
          if (finalized.number === null) throw new Error("Base finality is unavailable")
          const canonical = await firstSuccessfulHistoryClient(baseHistoryClients, (client) =>
            client.getBlock({ blockNumber: receipt.blockNumber }),
          )
          const result = exactMintReceiptFinalization({
            expected,
            expectedBridgeAddress: deploymentProfile.bridgeAddress as Hex,
            receipt,
            finalizedBlockNumber: finalized.number,
            canonicalReceiptBlockHash: canonical.hash,
          })
          if (result === "pending") throw new Error("Mint candidate is awaiting canonical finality")
          if (result !== "finalized") {
            recordMintRecoveryDiagnostic("conflict")
            persist({ ...current, search, conflict: true })
            return { status: "conflict", finalized: false, recorded: false }
          }
          const recovered = { ...pendingExpected, transactionHash: hash as Hex }
          await savePendingMint(recovered)
          recordMintRecoveryDiagnostic("verified", 1)
          return observeMint(recovered)
        }
        search = {
          ...search,
          hashes: search.hashes.filter((h) => h !== hash),
          seen: [...search.seen, hash],
        }
        persist({ ...current, search })
      }
      if (!search.hashes.length && !search.cursor) search.nextSearchAt = Date.now() + 60_000
      persist({ ...current, search })
      return { status: "unsubmitted", finalized: false, recorded: false }
    },
  )
}

const cycleSchema = z.object({
  nextRunAt: z.number().finite().nonnegative(),
  failures: z.number().int().min(0).max(10),
  lastTarget: z.string().optional(),
  finalizedReverts: z.array(hex32).default([]),
  owner: z.string().optional(),
  before: nat.optional(),
  nextHistoryAt: z.number().finite().nonnegative().optional(),
})
const cycleKey = `${prefix}cycle`
let sessionCycle: z.infer<typeof cycleSchema> = { nextRunAt: 0, failures: 0, finalizedReverts: [] }

/** One shared schedule processes one deposit and up to four candidate receipts per turn. */
export async function runMintRecoveryCycle(
  owner?: string,
): Promise<{ depositId: string; observation: MintObservation } | undefined> {
  return withBrowserLock(`kinic-deposit-owner:mint-recovery:${prefix}`, async () => {
    let cycle = sessionCycle
    try {
      cycle = cycleSchema.parse(JSON.parse(browserLocalStorage().getItem(cycleKey) ?? "null"))
    } catch {
      /* Session scheduling remains available. */
    }
    if (Date.now() < cycle.nextRunAt) return
    const saveCycle = () => {
      sessionCycle = cycle
      try {
        browserLocalStorage().setItem(cycleKey, JSON.stringify(cycle))
        return true
      } catch {
        return false
      }
    }
    cycle.nextRunAt = Date.now() + 10_000
    if (!saveCycle()) throw new Error("Shared recovery scheduling storage is unavailable")
    let depositId: string | undefined
    try {
      if (owner && (owner !== cycle.owner || Date.now() >= (cycle.nextHistoryAt ?? 0))) {
        try {
          const before = await discoverMintRecovery(
            owner,
            owner === cycle.owner && cycle.before ? BigInt(cycle.before) : undefined,
          )
          cycle.owner = owner
          cycle.before = before?.toString()
          cycle.nextHistoryAt = Date.now() + (before === undefined ? 60_000 : 10_000)
        } catch {
          if (cycle.owner !== owner) cycle.before = undefined
          cycle.owner = owner
          cycle.nextHistoryAt = Date.now() + 60_000
          // A history outage must not prevent tracking already saved transactions.
        }
      }
      const pending = readAllPendingMints().filter(
        (p) => !cycle.finalizedReverts.includes(p.transactionHash),
      )
      const revertedIds = new Set(
        readAllPendingMints()
          .filter((p) => cycle.finalizedReverts.includes(p.transactionHash))
          .map((p) => p.depositId),
      )
      const targets = readMintRecoveryTargets().filter((t) => !revertedIds.has(t.depositId as Hex))
      const ids = [
        ...new Set([...pending.map((p) => p.depositId), ...targets.map((t) => t.depositId)]),
      ].sort()
      depositId = ids.find((id) => id > (cycle.lastTarget ?? "")) ?? ids[0]
      if (!depositId) {
        cycle.failures = 0
        return
      }
      cycle.lastTarget = depositId
      const saved = pending.find((p) => p.depositId === depositId)
      const observation = saved
        ? await observeMint(saved)
        : await recoverMint(targets.find((t) => t.depositId === depositId)!)
      if (observation.recorded) {
        const tracked = readAllPendingMints().find((p) => p.depositId === depositId)
        if (tracked) await removePendingMint(tracked)
        for (const target of targets.filter((t) => t.depositId === depositId)) {
          const key = keyOf(target.depositId, target.digest)
          session.delete(key)
          browserLocalStorage().removeItem(key)
        }
      } else if (saved && observation.finalized && observation.status === "reverted") {
        cycle.finalizedReverts.push(saved.transactionHash)
      }
      if (observation.unavailable || observation.notificationError) {
        cycle.failures = Math.min(10, cycle.failures + 1)
        cycle.nextRunAt = Date.now() + Math.min(300_000, 30_000 * 2 ** (cycle.failures - 1))
      } else cycle.failures = 0
      return { depositId, observation }
    } catch (error) {
      recordMintRecoveryDiagnostic("unavailable")
      cycle.failures = Math.min(10, cycle.failures + 1)
      cycle.nextRunAt = Date.now() + Math.min(300_000, 30_000 * 2 ** (cycle.failures - 1))
      if (depositId)
        return {
          depositId,
          observation: {
            status: "unsubmitted",
            finalized: false,
            recorded: false,
            unavailable: true,
          },
        }
      throw error
    } finally {
      saveCycle()
    }
  })
}
