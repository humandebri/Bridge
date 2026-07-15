import { Actor, HttpAgent, type ActorSubclass } from "@dfinity/agent"
import type { IDL } from "@dfinity/candid"
import { Principal } from "@dfinity/principal"

export interface LedgerAccount { owner: Principal; subaccount: [] | [Uint8Array] }
export interface LedgerAllowance { allowance: bigint; expires_at: [] | [bigint] }
export interface LedgerActor {
  icrc1_balance_of(account: LedgerAccount): Promise<bigint>
  icrc1_name(): Promise<string>
  icrc1_decimals(): Promise<number>
  icrc1_symbol(): Promise<string>
  icrc1_fee(): Promise<bigint>
  icrc2_allowance(args: { account: LedgerAccount; spender: LedgerAccount }): Promise<LedgerAllowance>
}

export const ledgerIdlFactory: IDL.InterfaceFactory = ({ IDL: I }) => {
  const account = I.Record({ owner: I.Principal, subaccount: I.Opt(I.Vec(I.Nat8)) })
  return I.Service({
    icrc1_balance_of: I.Func([account], [I.Nat], ["query"]),
    icrc1_name: I.Func([], [I.Text], ["query"]),
    icrc1_decimals: I.Func([], [I.Nat8], ["query"]),
    icrc1_symbol: I.Func([], [I.Text], ["query"]),
    icrc1_fee: I.Func([], [I.Nat], ["query"]),
    icrc2_allowance: I.Func([I.Record({ account, spender: account })], [I.Record({ allowance: I.Nat, expires_at: I.Opt(I.Nat64) })], ["query"]),
  })
}

export async function createLedgerActor(host: string, canisterId: string): Promise<ActorSubclass<LedgerActor>> {
  const agent = HttpAgent.createSync({ host })
  if (agent.isLocal()) await agent.fetchRootKey()
  return Actor.createActor<LedgerActor>(ledgerIdlFactory, { agent, canisterId: Principal.fromText(canisterId) })
}

export function ledgerAccount(owner: string, subaccount?: Uint8Array): LedgerAccount {
  return { owner: Principal.fromText(owner), subaccount: subaccount ? [subaccount] : [] }
}
