import { IDL as LegacyIDL } from "@dfinity/candid"
import type { IDL } from "@dfinity/candid"
import { Principal as LegacyPrincipal } from "@dfinity/principal"
import { IcrcWallet } from "@dfinity/oisy-wallet-signer/icrc-wallet"
import { base64ToUint8Array, uint8ArrayToBase64 } from "@dfinity/utils"
import type { ApproveParams } from "@icp-sdk/canisters/ledger/icrc"
import { AnonymousIdentity, Cbor, Certificate, HttpAgent, lookupResultToBuffer, requestIdOf } from "@icp-sdk/core/agent"
import { Principal } from "@icp-sdk/core/principal"
import type { _SERVICE, DepositReceipt } from "@/generated/bridge.did"
import { idlFactory } from "@/generated/bridge.idl"

const CALL_TIMEOUT_MS = 120_000
const OISY_SIGNER_URL = "https://oisy.com/sign"

export type IcWalletProvider = "oisy" | "plug"
export interface IcAccount { owner: string; subaccount?: Uint8Array }
export interface DepositCall { clientRequestId: Uint8Array; baseRecipient: Uint8Array; grossAmount: bigint; maxServiceFee: bigint }
export interface ApprovalCall { amount: bigint; currentAllowance: bigint; ledgerFee: bigint }

export interface IcWalletAdapter {
  readonly provider: IcWalletProvider
  connect(): Promise<IcAccount>
  getAccount(): Promise<IcAccount>
  disconnect(): Promise<void>
  approve(call: ApprovalCall): Promise<bigint>
  requestDeposit(call: DepositCall): Promise<DepositReceipt>
  notifyWithdrawal(transactionHash: Uint8Array): Promise<void>
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

export class OisyAdapter implements IcWalletAdapter {
  readonly provider = "oisy" as const
  #wallet?: BridgeIcrcWallet
  #account?: IcAccount
  constructor(private readonly host: string, private readonly ledgerCanisterId: string, private readonly bridgeCanisterId: string) {}

  async connect(): Promise<IcAccount> {
    this.#wallet = await BridgeIcrcWallet.connect({ url: OISY_SIGNER_URL, host: this.host, connectionOptions: { timeoutInMilliseconds: CALL_TIMEOUT_MS } })
    await this.#wallet.requestPermissionsNotGranted({ options: { timeoutInMilliseconds: CALL_TIMEOUT_MS } })
    const [account] = await this.#wallet.accounts({ options: { timeoutInMilliseconds: CALL_TIMEOUT_MS } })
    if (!account) throw new Error("OISY returned no ICRC account")
    this.#account = { owner: account.owner, subaccount: parseSubaccount(account.subaccount) }
    return this.#account
  }

  async disconnect(): Promise<void> {
    await this.#wallet?.disconnect()
    this.#wallet = undefined
    this.#account = undefined
  }

  async getAccount(): Promise<IcAccount> {
    const wallet = this.requiredWallet()
    const expected = this.requiredAccount()
    const accounts = await wallet.accounts({ options: { timeoutInMilliseconds: CALL_TIMEOUT_MS } })
    const current = accounts.find((account) => account.owner === expected.owner && sameOptionalBytes(parseSubaccount(account.subaccount), expected.subaccount))
    if (!current) throw new Error("OISY account changed; reconnect and review the transaction")
    return { owner: current.owner, subaccount: parseSubaccount(current.subaccount) }
  }

  async approve(call: ApprovalCall): Promise<bigint> {
    const wallet = this.requiredWallet()
    const account = this.requiredAccount()
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
  }

  async requestDeposit(call: DepositCall): Promise<DepositReceipt> {
    const wallet = this.requiredWallet()
    const account = this.requiredAccount()
    const argBytes = encodeDepositCall(call, account.subaccount)
    const arg = uint8ArrayToBase64(argBytes)
    const result = await wallet.callCanister({ canisterId: this.bridgeCanisterId, sender: account.owner, method: "request_deposit", arg })
    const reply = await verifyOisyReply({ host: this.host, canisterId: this.bridgeCanisterId, sender: account.owner, method: "request_deposit", arg, result })
    return decodeDepositReply(reply)
  }

  async notifyWithdrawal(transactionHash: Uint8Array): Promise<void> {
    const wallet = this.requiredWallet()
    const account = await this.getAccount()
    const arg = uint8ArrayToBase64(encodeNotifyWithdrawalCall(transactionHash))
    const result = await wallet.callCanister({ canisterId: this.bridgeCanisterId, sender: account.owner, method: "notify_withdrawal", arg })
    const reply = await verifyOisyReply({ host: this.host, canisterId: this.bridgeCanisterId, sender: account.owner, method: "notify_withdrawal", arg, result })
    decodeNotifyWithdrawalReply(reply)
  }

  private requiredWallet(): BridgeIcrcWallet { if (!this.#wallet) throw new Error("Connect OISY first"); return this.#wallet }
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
    await this.assertConnectedPrincipal()
    const actor = await requiredPlug().createActor<_SERVICE>({ canisterId: this.bridgeCanisterId, interfaceFactory: idlFactory })
    const result = await actor.request_deposit({
      client_request_id: call.clientRequestId,
      base_recipient: call.baseRecipient,
      from_subaccount: [],
      gross_amount: call.grossAmount,
      max_service_fee: call.maxServiceFee,
    })
    if ("Err" in result) throw new Error(`Bridge rejected deposit: ${stringify(result.Err)}`)
    return result.Ok
  }

  async notifyWithdrawal(transactionHash: Uint8Array): Promise<void> {
    await this.assertConnectedPrincipal()
    const actor = await requiredPlug().createActor<_SERVICE>({ canisterId: this.bridgeCanisterId, interfaceFactory: idlFactory })
    const result = await actor.notify_withdrawal({ transaction_hash: transactionHash })
    if ("Err" in result) throw new Error(`Bridge rejected withdrawal notification: ${stringify(result.Err)}`)
  }

  private async assertConnectedPrincipal(): Promise<IcAccount> {
    if (!this.#account) throw new Error("Connect Plug first")
    const current = (await requiredPlug().agent.getPrincipal()).toText()
    if (current !== this.#account.owner) throw new Error("Plug account changed; reconnect and review the transaction")
    return this.#account
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

function encodeDepositCall(call: DepositCall, subaccount?: Uint8Array): Uint8Array {
  const type = LegacyIDL.Record({ client_request_id: LegacyIDL.Vec(LegacyIDL.Nat8), base_recipient: LegacyIDL.Vec(LegacyIDL.Nat8), from_subaccount: LegacyIDL.Opt(LegacyIDL.Vec(LegacyIDL.Nat8)), gross_amount: LegacyIDL.Nat, max_service_fee: LegacyIDL.Nat })
  return new Uint8Array(LegacyIDL.encode([type], [{ client_request_id: call.clientRequestId, base_recipient: call.baseRecipient, from_subaccount: subaccount ? [subaccount] : [], gross_amount: call.grossAmount, max_service_fee: call.maxServiceFee }]))
}

function encodeNotifyWithdrawalCall(transactionHash: Uint8Array): Uint8Array {
  const type = LegacyIDL.Record({ transaction_hash: LegacyIDL.Vec(LegacyIDL.Nat8) })
  return new Uint8Array(LegacyIDL.encode([type], [{ transaction_hash: transactionHash }]))
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
  const resultType = LegacyIDL.Variant({ Ok: LegacyIDL.Record({ deposit_id: LegacyIDL.Vec(LegacyIDL.Nat8), state: LegacyIDL.Text }), Err: LegacyIDL.Variant({ BaseObservationUnavailable: LegacyIDL.Null, ReserveUnavailable: LegacyIDL.Null, DepositsPaused: LegacyIDL.Null, Rejected: LegacyIDL.Text, InvalidRequest: LegacyIDL.Text, LedgerFeeUnavailable: LegacyIDL.Null, StorageFailure: LegacyIDL.Null, RateLimited: LegacyIDL.Record({ retry_after_seconds: LegacyIDL.Nat64 }) }) })
  const decodedValues: unknown = LegacyIDL.decode([resultType], reply)
  if (!Array.isArray(decodedValues)) throw new Error("Wallet reply has an invalid shape")
  const decoded: unknown = decodedValues[0]
  if (!isObject(decoded)) throw new Error("Wallet reply has an invalid shape")
  if ("Err" in decoded) throw new Error(`Bridge rejected deposit: ${stringify(Reflect.get(decoded, "Err"))}`)
  const ok: unknown = Reflect.get(decoded, "Ok")
  if (!isObject(ok) || typeof Reflect.get(ok, "state") !== "string") throw new Error("Wallet reply has an invalid deposit receipt")
  const id: unknown = Reflect.get(ok, "deposit_id")
  if (!(id instanceof Uint8Array) && !Array.isArray(id)) throw new Error("Wallet reply has an invalid deposit ID")
  const state: unknown = Reflect.get(ok, "state")
  if (typeof state !== "string") throw new Error("Wallet reply has an invalid deposit state")
  return { deposit_id: id, state }
}

function decodeNotifyWithdrawalReply(reply: Uint8Array): void {
  const resultType = LegacyIDL.Variant({
    Ok: LegacyIDL.Variant({ Queued: LegacyIDL.Null, Duplicate: LegacyIDL.Null }),
    Err: LegacyIDL.Variant({
      QueueFull: LegacyIDL.Null,
      RateLimited: LegacyIDL.Record({ retry_after_seconds: LegacyIDL.Nat64 }),
      InvalidTransactionHash: LegacyIDL.Null,
      StorageFailure: LegacyIDL.Null,
      AnonymousCaller: LegacyIDL.Null,
    }),
  })
  const decodedValues: unknown = LegacyIDL.decode([resultType], reply)
  if (!Array.isArray(decodedValues) || !isObject(decodedValues[0])) throw new Error("Wallet reply has an invalid notification result")
  const decoded = decodedValues[0]
  if ("Err" in decoded) throw new Error(`Bridge rejected withdrawal notification: ${stringify(Reflect.get(decoded, "Err"))}`)
}

function bytes(value: unknown, label: string): Uint8Array { if (value instanceof Uint8Array) return value; throw new Error(`Wallet response ${label} mismatch`) }
function sameBytes(left: Uint8Array, right: Uint8Array): boolean { return left.length === right.length && left.every((byte, index) => byte === right[index]) }
function sameOptionalBytes(left?: Uint8Array, right?: Uint8Array): boolean {
  if (!left || !right) return left === right
  return sameBytes(left, right)
}
function isObject(value: unknown): value is object { return typeof value === "object" && value !== null }
function stringify(value: unknown): string { return JSON.stringify(value, (_key, item: unknown) => typeof item === "bigint" ? item.toString() : item) }
