import { Actor, HttpAgent, type ActorSubclass } from "@dfinity/agent"
import type { IDL } from "@dfinity/candid"
import { Principal } from "@dfinity/principal"
import type { LedgerAccount } from "@/lib/ic/ledger"

export interface IndexActor {
  ledger_id(): Promise<Principal>
  status(): Promise<{ num_blocks_synced: bigint }>
  icrc1_balance_of(account: LedgerAccount): Promise<bigint>
}

export const indexIdlFactory: IDL.InterfaceFactory = ({ IDL: I }) => {
  const account = I.Record({ owner: I.Principal, subaccount: I.Opt(I.Vec(I.Nat8)) })
  return I.Service({
    ledger_id: I.Func([], [I.Principal], ["query"]),
    status: I.Func([], [I.Record({ num_blocks_synced: I.Nat })], ["query"]),
    icrc1_balance_of: I.Func([account], [I.Nat], ["query"]),
  })
}

export async function createIndexActor(host: string, canisterId: string): Promise<ActorSubclass<IndexActor>> {
  const agent = HttpAgent.createSync({ host })
  if (agent.isLocal()) await agent.fetchRootKey()
  return Actor.createActor<IndexActor>(indexIdlFactory, { agent, canisterId: Principal.fromText(canisterId) })
}
