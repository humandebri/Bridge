import { IDL as LegacyIDL } from "@dfinity/candid"
import type { IDL } from "@dfinity/candid"
import { Principal as LegacyPrincipal } from "@dfinity/principal"
import { IcrcWallet } from "@dfinity/oisy-wallet-signer/icrc-wallet"
import { base64ToUint8Array, uint8ArrayToBase64 } from "@dfinity/utils"
import type { ApproveParams } from "@icp-sdk/canisters/ledger/icrc"
import { AnonymousIdentity, Cbor, Certificate, HttpAgent, lookupResultToBuffer, requestIdOf } from "@icp-sdk/core/agent"
import { Principal } from "@icp-sdk/core/principal"
import type { _SERVICE, DepositReceipt, NotifyDepositMintReceipt, NotifyWithdrawalError, NotifyWithdrawalReceipt, SettlementActionError, SettlementActionResult } from "@/generated/bridge.did"
import { idlFactory } from "@/generated/bridge.idl"
import { isDepositPhase, isSettlementActionResult } from "@/lib/settlement-phase"

const CALL_TIMEOUT_MS = 120_000
const OISY_SIGNER_URL = "https://oisy.com/sign"
const BRIDGE_SERVICE = idlFactory({ IDL: LegacyIDL }) as IDL.ServiceClass

type BridgeWalletMethod = "request_deposit" | "notify_deposit_mint" | "notify_withdrawal" | "continue_deposit" | "continue_withdrawal"

export type IcWalletProvider = "oisy" | "plug"
export interface IcAccount { owner: string; subaccount?: Uint8Array }
export interface DepositCall { ownerSequence: bigint; baseRecipient: Uint8Array; grossAmount: bigint; maxServiceFee: bigint }
export interface ApprovalCall { amount: bigint; currentAllowance: bigint; ledgerFee: bigint }

export type SettlementActionErrorCode = SettlementActionError extends infer Variant
  ? Variant extends Record<string, unknown> ? keyof Variant : never
  : never

export type NotifyWithdrawalErrorCode = NotifyWithdrawalError extends infer Variant
  ? Variant extends Record<string, unknown> ? keyof Variant : never
  : never

export class SettlementActionCallError extends Error {
  constructor(
    readonly code: SettlementActionErrorCode,
    message: string,
    readonly retryAt?: number,
  ) {
    super(message)
    this.name = "SettlementActionCallError"
  }
}

export class NotifyWithdrawalCallError extends Error {
  constructor(
    readonly code: NotifyWithdrawalErrorCode,
    message: string,
  ) {
    super(message)
    this.name = "NotifyWithdrawalCallError"
  }
}

export interface IcWalletAdapter {
  readonly provider: IcWalletProvider
  readonly requiresUserGesture: boolean
  connect(): Promise<IcAccount>
  getAccount(): Promise<IcAccount>
  prepare(): Promise<() => Promise<void>>
  disconnect(): Promise<void>
  approve(call: ApprovalCall): Promise<bigint>
  requestDeposit(call: DepositCall): Promise<DepositReceipt>
  notifyDepositMint(depositId: Uint8Array, transactionHash: Uint8Array): Promise<NotifyDepositMintReceipt>
  notifyWithdrawal(transactionHash: Uint8Array): Promise<NotifyWithdrawalReceipt>
  continueDeposit(depositId: Uint8Array): Promise<SettlementActionResult>
  continueWithdrawal(withdrawalId: Uint8Array): Promise<SettlementActionResult>
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
  ) {}

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

  async notifyWithdrawal(transactionHash: Uint8Array): Promise<NotifyWithdrawalReceipt> {
    return unwrapNotifyWithdrawalResult(await this.bridgeCall("notify_withdrawal", () => [{ transaction_hash: transactionHash }]))
  }

  async notifyDepositMint(depositId: Uint8Array, transactionHash: Uint8Array): Promise<NotifyDepositMintReceipt> {
    return unwrapNotifyDepositMintResult(await this.bridgeCall("notify_deposit_mint", () => [{
      deposit_id: depositId,
      transaction_hash: transactionHash,
    }]))
  }

  async continueDeposit(depositId: Uint8Array): Promise<SettlementActionResult> {
    return this.continueSettlement("continue_deposit", depositId)
  }

  async continueWithdrawal(withdrawalId: Uint8Array): Promise<SettlementActionResult> {
    return this.continueSettlement("continue_withdrawal", withdrawalId)
  }

  private async continueSettlement(method: "continue_deposit" | "continue_withdrawal", id: Uint8Array): Promise<SettlementActionResult> {
    return unwrapSettlementResult(await this.bridgeCall(method, () => [id]))
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
  requestConnect(options: { whitelist: string[]; host: string }): Promise<boolean>
  disconnect(): Promise<void>
  agent: { getPrincipal(): Promise<LegacyPrincipal> }
  createActor<T>(options: { canisterId: string; interfaceFactory: IDL.InterfaceFactory }): Promise<T>
}

declare global { interface Window { ic?: { plug?: PlugApi } } }

export class PlugAdapter implements IcWalletAdapter {
  readonly provider = "plug" as const
  readonly requiresUserGesture = false
  #account?: IcAccount
  constructor(private readonly host: string, private readonly ledgerCanisterId: string, private readonly bridgeCanisterId: string) {}

  async connect(): Promise<IcAccount> {
    const plug = requiredPlug()
    const connected = await plug.requestConnect({ whitelist: [this.ledgerCanisterId, this.bridgeCanisterId], host: this.host })
    if (!connected) throw new Error("Plug connection was rejected")
    const principal = await plug.agent.getPrincipal()
    this.#account = { owner: principal.toText() }
    return this.#account
  }

  async disconnect(): Promise<void> { await requiredPlug().disconnect(); this.#account = undefined }

  prepare(): Promise<() => Promise<void>> { return Promise.resolve(() => Promise.resolve()) }

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
    if (account.owner !== this.#account?.owner) throw new Error("Plug account changed during approval")
    return result.Ok
  }

  async requestDeposit(call: DepositCall): Promise<DepositReceipt> {
    const actor = await this.bridgeActor()
    const result = await actor.request_deposit({
      owner_sequence: call.ownerSequence,
      base_recipient: call.baseRecipient,
      from_subaccount: [],
      gross_amount: call.grossAmount,
      max_service_fee: call.maxServiceFee,
    })
    return unwrapDepositResult(result)
  }

  async notifyWithdrawal(transactionHash: Uint8Array): Promise<NotifyWithdrawalReceipt> {
    const actor = await this.bridgeActor()
    const result = await actor.notify_withdrawal({ transaction_hash: transactionHash })
    return unwrapNotifyWithdrawalResult(result)
  }

  async notifyDepositMint(depositId: Uint8Array, transactionHash: Uint8Array): Promise<NotifyDepositMintReceipt> {
    const actor = await this.bridgeActor()
    const result = await actor.notify_deposit_mint({ deposit_id: depositId, transaction_hash: transactionHash })
    return unwrapNotifyDepositMintResult(result)
  }

  async continueDeposit(depositId: Uint8Array): Promise<SettlementActionResult> {
    return this.continueSettlement("continue_deposit", depositId)
  }

  async continueWithdrawal(withdrawalId: Uint8Array): Promise<SettlementActionResult> {
    return this.continueSettlement("continue_withdrawal", withdrawalId)
  }

  private async continueSettlement(method: "continue_deposit" | "continue_withdrawal", id: Uint8Array): Promise<SettlementActionResult> {
    const actor = await this.bridgeActor()
    const result = await actor[method](id)
    return unwrapSettlementResult(result)
  }

  private async assertConnectedPrincipal(): Promise<IcAccount> {
    if (!this.#account) throw new Error("Connect Plug first")
    const current = (await requiredPlug().agent.getPrincipal()).toText()
    if (current !== this.#account.owner) throw new Error("Plug account changed; reconnect and review the transaction")
    return this.#account
  }

  private async bridgeActor(): Promise<_SERVICE> {
    await this.assertConnectedPrincipal()
    return requiredPlug().createActor<_SERVICE>({ canisterId: this.bridgeCanisterId, interfaceFactory: idlFactory })
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

export function decodeNotifyWithdrawalReply(reply: Uint8Array): NotifyWithdrawalReceipt {
  return unwrapNotifyWithdrawalResult(decodeBridgeReply("notify_withdrawal", reply))
}

function unwrapNotifyDepositMintResult(result: unknown): NotifyDepositMintReceipt {
  if (!isObject(result)) throw new Error("Wallet reply has an invalid mint notification result")
  if ("Err" in result) throw new Error(`Mint confirmation failed: ${stringify(Reflect.get(result, "Err"))}`)
  const receipt: unknown = Reflect.get(result, "Ok")
  if (!isObject(receipt)) throw new Error("Wallet reply has an invalid mint notification receipt")
  const key = Object.keys(receipt)[0]
  if (Object.keys(receipt).length !== 1 || (key !== "Minted" && key !== "Duplicate")) {
    throw new Error("Wallet reply has an invalid mint notification receipt")
  }
  return receipt as NotifyDepositMintReceipt
}

function unwrapNotifyWithdrawalResult(result: unknown): NotifyWithdrawalReceipt {
  if (!isObject(result)) throw new Error("Wallet reply has an invalid notification result")
  const decoded = result as Record<string, unknown>
  if ("Err" in decoded) {
    const error = decoded.Err
    const code = notifyWithdrawalErrorCode(error)
    if (!code) throw new Error("Wallet reply has an invalid withdrawal notification error")
    throw new NotifyWithdrawalCallError(code, notifyWithdrawalErrorMessage(error as NotifyWithdrawalError))
  }
  const receipt: unknown = decoded.Ok
  if (!isObject(receipt)) throw new Error("Wallet reply has an invalid notification receipt")
  const receiptKeys = Object.keys(receipt)
  const receiptKey = receiptKeys[0]
  if (receiptKeys.length !== 1 || (receiptKey !== "Ingested" && receiptKey !== "Duplicate")) throw new Error("Wallet reply has an invalid notification receipt")
  const payload: unknown = (receipt as Record<string, unknown>)[receiptKey]
  if (!isObject(payload)) throw new Error("Wallet reply has an invalid notification receipt")
  const payloadRecord = payload as Record<string, unknown>
  const withdrawalId: unknown = payloadRecord.withdrawal_id
  if (!(withdrawalId instanceof Uint8Array) && !Array.isArray(withdrawalId)) throw new Error("Wallet reply has an invalid withdrawal ID")
  if (receiptKey === "Ingested" && typeof payloadRecord.finalized_head_block_number !== "bigint") throw new Error("Wallet reply has an invalid finalized block")
  return receipt as NotifyWithdrawalReceipt
}

function unwrapSettlementResult(result: unknown): SettlementActionResult {
  if (!isObject(result)) throw new Error("Wallet reply has an invalid settlement result")
  const decoded = result as Record<string, unknown>
  if (decoded.Err !== undefined) throw settlementActionCallError(decoded.Err)
  if (decoded.Ok === undefined || !isSettlementActionResult(decoded.Ok)) throw new Error("Wallet reply has an invalid settlement result")
  return decoded.Ok
}

function settlementActionCallError(error: unknown): Error {
  const code = settlementActionErrorCode(error)
  if (!code) return new Error("Wallet reply has an invalid settlement error")
  const message = settlementActionErrorMessage(error)
  const rateLimited: unknown = isObject(error) ? Reflect.get(error, "RateLimited") as unknown : undefined
  if (code === "RateLimited" && isObject(rateLimited)) {
    const retryAfter: unknown = Reflect.get(rateLimited, "retry_after_seconds")
    if (typeof retryAfter === "bigint") return new SettlementActionCallError(code, message, Date.now() + Number(retryAfter) * 1_000)
  }
  const automaticProgress: unknown = isObject(error) ? Reflect.get(error, "AutomaticProgressPending") as unknown : undefined
  if (code === "AutomaticProgressPending" && isObject(automaticProgress)) {
    const nextRun: unknown = Reflect.get(automaticProgress, "next_run_at_ns")
    const nextCheck: unknown = Array.isArray(nextRun) ? (nextRun.at(0) as unknown) : undefined
    if (typeof nextCheck === "bigint") return new SettlementActionCallError(code, message, Number(nextCheck / 1_000_000n))
  }
  return new SettlementActionCallError(code, message)
}

function settlementActionErrorCode(error: unknown): SettlementActionErrorCode | undefined {
  if (!isObject(error)) return undefined
  const code = Object.keys(error)[0]
  if (code && ["AutomaticProgressPending", "InvalidId", "Busy", "WrongState", "NotFound", "Unauthorized", "RateLimited", "StorageFailure", "AnonymousCaller"].includes(code)) return code as SettlementActionErrorCode
  return undefined
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

export function notifyWithdrawalErrorMessage(error: NotifyWithdrawalError): string {
  const key = Object.keys(error)[0]
  const messages: Record<string, string> = {
    LedgerFeeExceedsServiceFee: "Withdrawals were stopped because the ledger fee exceeded the charged service fee. Contact the bridge operator before resuming from History.",
    RpcUnavailable: "Base RPC is unavailable. Retry the notification manually later.",
    RpcInconsistent: "Base RPC providers disagreed. Retry the notification manually later.",
    InvalidBaseResponse: "Base returned an invalid withdrawal response.",
    TransactionNotFound: "The Base withdrawal transaction was not found.",
    TransactionNotConfirmed: "The Base withdrawal transaction has not reached the finalized head yet.",
    TransactionReverted: "The Base withdrawal transaction reverted.",
    BaseStateMismatch: "The finalized Bridge withdrawal state does not match its creation event.",
    BridgeSignerMismatch: "The finalized Bridge signer does not match the configured canister signer.",
    WithdrawalConflict: "A different withdrawal payload already uses this withdrawal ID.",
    InvalidTransactionHash: "The withdrawal transaction hash is invalid.",
    StorageFailure: "The Bridge could not save the withdrawal.",
    AnonymousCaller: "Connect an IC wallet before notifying the withdrawal.",
    Busy: "This withdrawal notification or withdrawal record is already being processed. Check History before trying again manually.",
    RateLimited: "Withdrawal notifications are temporarily rate limited. Retry later.",
    InsufficientCycles: "The Bridge Canister does not have enough cycles to process this notification.",
  }
  const message = key === undefined ? undefined : messages[key]
  return message ?? `Bridge rejected withdrawal notification: ${stringify(error)}`
}

function notifyWithdrawalErrorCode(error: unknown): NotifyWithdrawalErrorCode | undefined {
  if (!isObject(error)) return undefined
  const code = Object.keys(error)[0]
  if (code && [
    "LedgerFeeExceedsServiceFee", "Busy", "RpcUnavailable", "TransactionNotConfirmed",
    "WithdrawalConflict", "RpcInconsistent", "InvalidTransactionHash",
    "TransactionReverted", "StorageFailure", "BaseStateMismatch",
    "TransactionNotFound", "BridgeSignerMismatch", "AnonymousCaller", "InvalidBaseResponse",
    "RateLimited", "InsufficientCycles",
  ].includes(code)) return code as NotifyWithdrawalErrorCode
  return undefined
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
