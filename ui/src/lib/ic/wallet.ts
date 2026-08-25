import { IDL as LegacyIDL } from "@dfinity/candid"
import type { IDL } from "@dfinity/candid"
import { Principal as LegacyPrincipal } from "@dfinity/principal"
import { IcrcWallet } from "@dfinity/oisy-wallet-signer/icrc-wallet"
import { base64ToUint8Array, uint8ArrayToBase64 } from "@dfinity/utils"
import type { ApproveParams } from "@icp-sdk/canisters/ledger/icrc"
import { AnonymousIdentity, Cbor, Certificate, HttpAgent, lookupResultToBuffer, requestIdOf } from "@icp-sdk/core/agent"
import { Principal } from "@icp-sdk/core/principal"
import type { _SERVICE, DepositReceipt, DepositView } from "@/generated/bridge.did"
import { idlFactory } from "@/generated/bridge.idl"
import { isDepositPhase } from "@/lib/settlement-phase"

const CALL_TIMEOUT_MS = 120_000
const OISY_SIGNER_URL = "https://oisy.com/sign"
const BRIDGE_SERVICE = idlFactory({ IDL: LegacyIDL })

type BridgeWalletMethod = "request_deposit" | "request_deposit_refund"

export type IcWalletProvider = "oisy" | "plug"
export interface IcAccount { owner: string; subaccount?: Uint8Array }
export interface DepositCall { ownerSequence: bigint; baseRecipient: Uint8Array; grossAmount: bigint; maxServiceFee: bigint }
export interface ApprovalCall { amount: bigint; currentAllowance: bigint; ledgerFee: bigint }

export interface IcWalletAdapter {
  readonly provider: IcWalletProvider
  readonly requiresUserGesture: boolean
  connect(): Promise<IcAccount>
  getAccount(): Promise<IcAccount>
  prepare(): Promise<() => Promise<void>>
  disconnect(): Promise<void>
  approve(call: ApprovalCall): Promise<bigint>
  requestDeposit(call: DepositCall): Promise<DepositReceipt>
  requestDepositRefund(depositId: Uint8Array): Promise<DepositView>
}

type IcrcCallCanisterRequestParams = { canisterId: string; sender: string; method: string; arg: string; nonce?: string }
type IcrcCallCanisterResult = { contentMap: string; certificate: string }
type OisyOptions = { origin: string; popup: Window; onDisconnect?: () => void; host?: string }

class BridgeIcrcWallet extends IcrcWallet {
  constructor(options: OisyOptions) { super(options) }
  static override async connect({ onDisconnect, host, ...rest }: Parameters<typeof IcrcWallet.connect>[0]): Promise<BridgeIcrcWallet> {
    return BridgeIcrcWallet.connectSigner({ options: rest, init: (params) => new BridgeIcrcWallet({ ...params, onDisconnect, host }) })
  }
  callCanister(params: IcrcCallCanisterRequestParams): Promise<IcrcCallCanisterResult> {
    return this.call({ params, options: { timeoutInMilliseconds: CALL_TIMEOUT_MS } })
  }
}

type OisyWalletSession = Pick<BridgeIcrcWallet, "accounts" | "approve" | "callCanister" | "disconnect" | "requestPermissionsNotGranted">

export class OisyAdapter implements IcWalletAdapter {
  readonly provider = "oisy" as const
  readonly requiresUserGesture = true
  #account?: IcAccount
  #session?: Promise<{ wallet: OisyWalletSession; account: IcAccount }>
  constructor(
    private readonly host: string,
    private readonly ledgerCanisterId: string,
    private readonly bridgeCanisterId: string,
    private readonly connectWallet: () => Promise<OisyWalletSession> = () => BridgeIcrcWallet.connect({ url: OISY_SIGNER_URL, host, connectionOptions: { timeoutInMilliseconds: CALL_TIMEOUT_MS } }),
    restoredAccount?: IcAccount,
  ) {
    this.#account = restoredAccount ? copyAccount(restoredAccount) : undefined
  }

  async connect(): Promise<IcAccount> {
    const wallet = await this.openWallet()
    try {
      const account = await this.readAccount(wallet)
      this.#account = account
      return copyAccount(account)
    } finally {
      await wallet.disconnect()
    }
  }

  async disconnect(): Promise<void> {
    this.#account = undefined
    const session = this.#session
    this.#session = undefined
    if (!session) return
    try {
      const { wallet } = await session
      await wallet.disconnect()
    } catch {
      // A failed session already closes its popup while unwinding prepare().
    }
  }

  getAccount(): Promise<IcAccount> {
    return Promise.resolve(copyAccount(this.requiredAccount()))
  }

  async prepare(): Promise<() => Promise<void>> {
    if (this.#session) throw new Error("An OISY wallet action is already in progress")
    const expected = this.requiredAccount()
    const walletPromise = this.openWallet()
    const session = walletPromise.then(async (wallet) => {
      try {
        const account = await this.readAccount(wallet)
        this.assertExpectedAccount(expected, account)
        return { wallet, account }
      } catch (error) {
        await wallet.disconnect()
        throw error
      }
    })
    this.#session = session
    try {
      await session
    } catch (error) {
      if (this.#session === session) this.#session = undefined
      throw error
    }
    return async () => {
      if (this.#session !== session) return
      this.#session = undefined
      const { wallet } = await session
      await wallet.disconnect()
    }
  }

  async approve(call: ApprovalCall): Promise<bigint> {
    return this.withWallet(async (wallet, account) => {
      const expiresAt = BigInt(Date.now() + 30 * 60 * 1000) * 1_000_000n
      const params: ApproveParams = {
        amount: call.amount,
        expected_allowance: call.currentAllowance,
        expires_at: expiresAt,
        fee: call.ledgerFee,
        from_subaccount: account.subaccount,
        spender: { owner: Principal.fromText(this.bridgeCanisterId), subaccount: [] },
      }
      return wallet.approve({ owner: account.owner, ledgerCanisterId: this.ledgerCanisterId, params })
    })
  }

  async requestDeposit(call: DepositCall): Promise<DepositReceipt> {
    return unwrapDepositResult(await this.bridgeCall("request_deposit", (account) => [{ owner_sequence: call.ownerSequence, base_recipient: call.baseRecipient, from_subaccount: account.subaccount ? [account.subaccount] : [], gross_amount: call.grossAmount, max_service_fee: call.maxServiceFee }]))
  }

  async requestDepositRefund(depositId: Uint8Array): Promise<DepositView> {
    return unwrapRequestDepositRefundResult(await this.bridgeCall("request_deposit_refund", () => [depositId]))
  }

  private async bridgeCall(method: BridgeWalletMethod, createArgs: (account: IcAccount) => unknown[] = () => []): Promise<unknown> {
    return this.withWallet(async (wallet, account) => {
      const arg = uint8ArrayToBase64(encodeBridgeCall(method, createArgs(account)))
      const result = await wallet.callCanister({ canisterId: this.bridgeCanisterId, sender: account.owner, method, arg })
      const reply = await verifyOisyReply({ host: this.host, canisterId: this.bridgeCanisterId, sender: account.owner, method, arg, result })
      return decodeBridgeReply(method, reply)
    })
  }

  private openWallet(): Promise<OisyWalletSession> {
    return this.connectWallet()
  }

  private async readAccount(wallet: OisyWalletSession): Promise<IcAccount> {
    await wallet.requestPermissionsNotGranted({ options: { timeoutInMilliseconds: CALL_TIMEOUT_MS } })
    const [account] = await wallet.accounts({ options: { timeoutInMilliseconds: CALL_TIMEOUT_MS } })
    if (!account) throw new Error("OISY returned no ICRC account")
    return { owner: account.owner, subaccount: parseSubaccount(account.subaccount) }
  }

  private async withWallet<T>(operation: (wallet: OisyWalletSession, account: IcAccount) => Promise<T>): Promise<T> {
    const expected = this.requiredAccount()
    const prepared = this.#session
    if (prepared) {
      const { wallet, account } = await prepared
      this.assertExpectedAccount(expected, account)
      return operation(wallet, account)
    }
    const wallet = await this.openWallet()
    try {
      const current = await this.readAccount(wallet)
      this.assertExpectedAccount(expected, current)
      return await operation(wallet, current)
    } finally {
      await wallet.disconnect()
    }
  }

  private assertExpectedAccount(expected: IcAccount, current: IcAccount): void {
    if (current.owner !== expected.owner || !sameOptionalBytes(current.subaccount, expected.subaccount)) {
      throw new Error("OISY account changed; reconnect and review the transaction")
    }
  }

  private requiredAccount(): IcAccount { if (!this.#account) throw new Error("Connect OISY first"); return this.#account }
}

interface PlugLedgerActor { icrc2_approve(args: Record<string, unknown>): Promise<{ Ok: bigint } | { Err: unknown }> }
interface PlugApi {
  isConnected(): Promise<boolean>
  requestConnect(options: { whitelist: string[]; host: string }): Promise<boolean>
  disconnect(): Promise<void>
  agent?: { getPrincipal(): Promise<LegacyPrincipal> }
  createActor<T>(options: { canisterId: string; interfaceFactory: IDL.InterfaceFactory }): Promise<T>
}

declare global { interface Window { ic?: { plug?: PlugApi } } }

export class PlugAdapter implements IcWalletAdapter {
  readonly provider = "plug" as const
  #requiresUserGesture: boolean
  #account?: IcAccount
  constructor(
    private readonly host: string,
    private readonly ledgerCanisterId: string,
    private readonly bridgeCanisterId: string,
    restoredAccount?: IcAccount,
  ) {
    this.#account = restoredAccount ? copyAccount(restoredAccount) : undefined
    this.#requiresUserGesture = restoredAccount !== undefined
  }

  get requiresUserGesture(): boolean { return this.#requiresUserGesture }

  async connect(): Promise<IcAccount> {
    const plug = requiredPlug()
    const connected = await plug.requestConnect({ whitelist: [this.ledgerCanisterId, this.bridgeCanisterId], host: this.host })
    if (!connected) throw new Error("Plug connection was rejected")
    const agent = plug.agent
    if (!agent) throw new Error("Plug did not initialize its Agent; reconnect and try again")
    const principal = await agent.getPrincipal()
    this.#account = { owner: principal.toText() }
    this.#requiresUserGesture = false
    return this.#account
  }

  async disconnect(): Promise<void> { await requiredPlug().disconnect(); this.#account = undefined }

  async prepare(): Promise<() => Promise<void>> {
    const plug = requiredPlug()
    let connected = false
    try {
      connected = await plug.isConnected()
    } catch {
      // A failed session probe requires an explicit reconnect below.
    }
    if (!connected || !plug.agent) {
      const accepted = await plug.requestConnect({ whitelist: [this.ledgerCanisterId, this.bridgeCanisterId], host: this.host })
      if (!accepted) throw new Error("Plug connection was rejected")
    }
    await this.assertConnectedPrincipal()
    this.#requiresUserGesture = false
    return () => Promise.resolve()
  }

  async getAccount(): Promise<IcAccount> { return this.assertConnectedPrincipal() }

  async approve(call: ApprovalCall): Promise<bigint> {
    const account = await this.assertConnectedPrincipal()
    const actor = await requiredPlug().createActor<PlugLedgerActor>({ canisterId: this.ledgerCanisterId, interfaceFactory: plugLedgerIdlFactory })
    const result = await actor.icrc2_approve({
      from_subaccount: [],
      spender: { owner: LegacyPrincipal.fromText(this.bridgeCanisterId), subaccount: [] },
      amount: call.amount,
      expected_allowance: [call.currentAllowance],
      expires_at: [BigInt(Date.now() + 30 * 60 * 1000) * 1_000_000n],
      fee: [call.ledgerFee], memo: [], created_at_time: [],
    })
    if ("Err" in result) throw new Error(`Plug approval failed: ${stringify(result.Err)}`)
    await this.assertSameConnectedAccount(account, "approval")
    return result.Ok
  }

  async requestDeposit(call: DepositCall): Promise<DepositReceipt> {
    const account = await this.assertConnectedPrincipal()
    const actor = await requiredPlug().createActor<_SERVICE>({ canisterId: this.bridgeCanisterId, interfaceFactory: idlFactory })
    const result = await actor.request_deposit({
      owner_sequence: call.ownerSequence,
      base_recipient: call.baseRecipient,
      from_subaccount: [],
      gross_amount: call.grossAmount,
      max_service_fee: call.maxServiceFee,
    })
    await this.assertSameConnectedAccount(account, "deposit")
    return unwrapDepositResult(result)
  }

  async requestDepositRefund(depositId: Uint8Array): Promise<DepositView> {
    const account = await this.assertConnectedPrincipal()
    const actor = await requiredPlug().createActor<_SERVICE>({ canisterId: this.bridgeCanisterId, interfaceFactory: idlFactory })
    const result = await actor.request_deposit_refund(depositId)
    await this.assertSameConnectedAccount(account, "refund")
    return unwrapRequestDepositRefundResult(result)
  }

  private async assertConnectedPrincipal(): Promise<IcAccount> {
    if (!this.#account) throw new Error("Connect Plug first")
    const agent = requiredPlug().agent
    if (!agent) throw new Error("Plug Agent is unavailable; reconnect and review the transaction")
    const current = (await agent.getPrincipal()).toText()
    if (current !== this.#account.owner) throw new Error("Plug account changed; reconnect and review the transaction")
    return copyAccount(this.#account)
  }

  private async assertSameConnectedAccount(expected: IcAccount, operation: string): Promise<void> {
    const current = await this.assertConnectedPrincipal()
    if (current.owner !== expected.owner || !sameOptionalBytes(current.subaccount, expected.subaccount)) {
      throw new Error(`Plug account changed during ${operation}; reconnect and review the transaction`)
    }
  }
}

const plugLedgerIdlFactory: IDL.InterfaceFactory = ({ IDL: I }) => {
  const account = I.Record({ owner: I.Principal, subaccount: I.Opt(I.Vec(I.Nat8)) })
  const approveError = I.Variant({
    BadFee: I.Record({ expected_fee: I.Nat }), InsufficientFunds: I.Record({ balance: I.Nat }),
    AllowanceChanged: I.Record({ current_allowance: I.Nat }), Expired: I.Record({ ledger_time: I.Nat64 }),
    TooOld: I.Null, CreatedInFuture: I.Record({ ledger_time: I.Nat64 }), Duplicate: I.Record({ duplicate_of: I.Nat }),
    TemporarilyUnavailable: I.Null, GenericError: I.Record({ error_code: I.Nat, message: I.Text }),
  })
  return I.Service({ icrc2_approve: I.Func([I.Record({ from_subaccount: I.Opt(I.Vec(I.Nat8)), spender: account, amount: I.Nat, expected_allowance: I.Opt(I.Nat), expires_at: I.Opt(I.Nat64), fee: I.Opt(I.Nat), memo: I.Opt(I.Vec(I.Nat8)), created_at_time: I.Opt(I.Nat64) })], [I.Variant({ Ok: I.Nat, Err: approveError })], []) })
}

function requiredPlug(): PlugApi { const plug = window.ic?.plug; if (!plug) throw new Error("Plug extension is not installed"); return plug }

function parseSubaccount(value?: string): Uint8Array | undefined {
  if (!value) return undefined
  const hex = value.replace(/^0x/, "")
  if (!/^[0-9a-fA-F]{64}$/.test(hex)) throw new Error("OISY returned an invalid subaccount")
  return Uint8Array.from(hex.match(/../g) ?? [], (byte) => Number.parseInt(byte, 16))
}

function bridgeMethod(method: BridgeWalletMethod): IDL.FuncClass {
  const codec = BRIDGE_SERVICE._fields.find(([name]) => name === method)?.[1]
  if (!codec) throw new Error(`Generated Bridge Candid does not define ${method}`)
  return codec
}

function encodeBridgeCall(method: BridgeWalletMethod, args: unknown[]): Uint8Array {
  return new Uint8Array(LegacyIDL.encode(bridgeMethod(method).argTypes, args))
}

function decodeBridgeReply(method: BridgeWalletMethod, reply: Uint8Array): unknown {
  const decoded = LegacyIDL.decode(bridgeMethod(method).retTypes, reply)
  if (decoded.length !== 1) throw new Error(`Wallet reply for ${method} has an invalid result count`)
  return decoded[0]
}

async function verifyOisyReply(input: { host: string; canisterId: string; sender: string; method: string; arg: string; result: IcrcCallCanisterResult }): Promise<Uint8Array> {
  const contentMap = Cbor.decode<Record<string, unknown>>(base64ToUint8Array(input.result.contentMap))
  if (contentMap.method_name !== input.method) throw new Error("Wallet response method mismatch")
  const canister = bytes(contentMap.canister_id, "canister")
  if (Principal.fromUint8Array(canister).toText() !== Principal.fromText(input.canisterId).toText()) throw new Error("Wallet response canister mismatch")
  const sender = bytes(contentMap.sender, "sender")
  if (Principal.fromUint8Array(sender).toText() !== Principal.fromText(input.sender).toText()) throw new Error("Wallet response sender mismatch")
  if (!sameBytes(bytes(contentMap.arg, "argument"), base64ToUint8Array(input.arg))) throw new Error("Wallet response argument mismatch")
  const agent = HttpAgent.createSync({ identity: new AnonymousIdentity(), host: input.host })
  if (agent.isLocal()) await agent.fetchRootKey()
  if (!agent.rootKey) throw new Error("IC root key is unavailable")
  const certificate = await Certificate.create({ certificate: base64ToUint8Array(input.result.certificate), rootKey: agent.rootKey, canisterId: Principal.fromText(input.canisterId) })
  const reply = lookupResultToBuffer(certificate.lookup_path([new TextEncoder().encode("request_status"), requestIdOf(contentMap), "reply"]))
  if (!reply) throw new Error("Certified wallet reply is unavailable")
  return reply
}

export function decodeDepositReply(reply: Uint8Array): DepositReceipt {
  return unwrapDepositResult(decodeBridgeReply("request_deposit", reply))
}

function unwrapDepositResult(result: unknown): DepositReceipt {
  if (!isObject(result)) throw new Error("Wallet reply has an invalid shape")
  if ("Err" in result) throw new Error(depositErrorMessage(Reflect.get(result, "Err")))
  const ok: unknown = Reflect.get(result, "Ok")
  if (!isObject(ok) || !isDepositPhase(Reflect.get(ok, "state"))) throw new Error("Wallet reply has an invalid deposit receipt")
  const id: unknown = Reflect.get(ok, "deposit_id")
  if (!(id instanceof Uint8Array) && !Array.isArray(id)) throw new Error("Wallet reply has an invalid deposit ID")
  const state: unknown = Reflect.get(ok, "state")
  if (!isDepositPhase(state)) throw new Error("Wallet reply has an invalid deposit state")
  const ownerSequence: unknown = Reflect.get(ok, "owner_sequence")
  if (typeof ownerSequence !== "bigint") throw new Error("Wallet reply has an invalid owner sequence")
  if (Object.keys(ok).length !== 3) throw new Error("Wallet reply has an invalid deposit receipt")
  return { deposit_id: id, owner_sequence: ownerSequence, state }
}

function depositErrorMessage(error: unknown): string {
  if (!isObject(error)) return "Bridge rejected deposit"
  if (Reflect.has(error, "ReserveUnavailable")) {
    return "Bridge cycles reserve is temporarily insufficient. Retry this deposit after the bridge is replenished"
  }
  const maintenance = Reflect.get(error, "ReservationMaintenance") as unknown
  const maintenanceRetryAfter: unknown = isObject(maintenance)
    ? (Reflect.get(maintenance, "retry_after_seconds") as unknown)
    : undefined
  if (typeof maintenanceRetryAfter === "bigint") {
    return `Bridge reservation maintenance is in progress. Retry the same deposit in ${maintenanceRetryAfter.toString()} seconds`
  }
  const unavailable = Reflect.get(error, "FundingUnavailable") as unknown
  const retryAfter: unknown = isObject(unavailable)
    ? (Reflect.get(unavailable, "retry_after_seconds") as unknown)
    : undefined
  if (typeof retryAfter === "bigint") {
    return `Deposit funding is temporarily unavailable. Retry the same deposit in ${retryAfter.toString()} seconds`
  }
  const rejected = Reflect.get(error, "FundingRejected") as unknown
  if (isObject(rejected)) {
    const allowance = Reflect.get(rejected, "InsufficientAllowance") as unknown
    const allowanceAmount: unknown = isObject(allowance)
      ? (Reflect.get(allowance, "allowance") as unknown)
      : undefined
    if (typeof allowanceAmount === "bigint") {
      return `Deposit funding was rejected because the ledger allowance is insufficient (current ${allowanceAmount.toString()})`
    }
    const funds = Reflect.get(rejected, "InsufficientFunds") as unknown
    const balance: unknown = isObject(funds)
      ? (Reflect.get(funds, "balance") as unknown)
      : undefined
    if (typeof balance === "bigint") {
      return `Deposit funding was rejected because the ledger balance is insufficient (current ${balance.toString()})`
    }
    const badFee = Reflect.get(rejected, "BadFee") as unknown
    const expectedFee: unknown = isObject(badFee)
      ? (Reflect.get(badFee, "expected_fee") as unknown)
      : undefined
    if (typeof expectedFee === "bigint") {
      return `Deposit funding was rejected because the ledger fee changed (expected ${expectedFee.toString()})`
    }
    const badBurn = Reflect.get(rejected, "BadBurn") as unknown
    const minimum: unknown = isObject(badBurn)
      ? (Reflect.get(badBurn, "minimum") as unknown)
      : undefined
    if (typeof minimum === "bigint") {
      return `Deposit funding was rejected because the amount is below the ledger minimum (${minimum.toString()})`
    }
  }
  return `Bridge rejected deposit: ${stringify(error)}`
}

function unwrapRequestDepositRefundResult(result: unknown): DepositView {
  if (!isObject(result)) throw new Error("Wallet reply has an invalid refund claim result")
  if ("Err" in result) {
    const error = Reflect.get(result, "Err")
    throw new Error(requestDepositRefundErrorMessage(error))
  }
  const record: unknown = Reflect.get(result, "Ok")
  if (!isObject(record) || !isDepositPhase(Reflect.get(record, "state"))) {
    throw new Error("Wallet reply has an invalid refund claim receipt")
  }
  return record as unknown as DepositView
}

export function requestDepositRefundErrorMessage(error: unknown): string {
  if (isObject(error) && ("AutomaticProgressPending" in error || "RateLimited" in error)) {
    return settlementActionErrorMessage(error)
  }
  return `Refund claim failed: ${stringify(error)}`
}

function settlementActionErrorMessage(error: unknown): string {
  if (isObject(error) && "WrongState" in error) return "This settlement is not waiting for this action."
  if (isObject(error) && "AutomaticProgressPending" in error && isObject(error.AutomaticProgressPending) && "next_run_at_ns" in error.AutomaticProgressPending) {
    const nextRun = error.AutomaticProgressPending.next_run_at_ns
    const nextCheck: unknown = Array.isArray(nextRun) ? (nextRun.at(0) as unknown) : undefined
    if (typeof nextCheck === "bigint") return `Settlement is progressing automatically. Next confirmation check: ${new Date(Number(nextCheck / 1_000_000n)).toLocaleString()}.`
    return "Settlement is progressing automatically."
  }
  if (isObject(error) && "RateLimited" in error && isObject(error.RateLimited) && "retry_after_seconds" in error.RateLimited) {
    const retryAfter = error.RateLimited.retry_after_seconds
    if (typeof retryAfter === "bigint") {
      const retryAt = new Date(Date.now() + Number(retryAfter) * 1_000)
      return `Too many settlement retries. Retry after ${retryAt.toLocaleString()}.`
    }
  }
  return `Bridge rejected settlement action: ${stringify(error)}`
}

function bytes(value: unknown, label: string): Uint8Array { if (value instanceof Uint8Array) return value; throw new Error(`Wallet response ${label} mismatch`) }
function copyAccount(account: IcAccount): IcAccount { return { owner: account.owner, subaccount: account.subaccount?.slice() } }
function sameBytes(left: Uint8Array, right: Uint8Array): boolean { return left.length === right.length && left.every((byte, index) => byte === right[index]) }
function sameOptionalBytes(left?: Uint8Array, right?: Uint8Array): boolean {
  if (!left || !right) return left === right
  return sameBytes(left, right)
}
function isObject(value: unknown): value is object { return typeof value === "object" && value !== null }
function stringify(value: unknown): string { return JSON.stringify(value, (_key, item: unknown) => typeof item === "bigint" ? item.toString() : item) }
