import { IcrcIndexCanister } from "@icp-sdk/canisters/ledger/icrc"
import { Principal } from "@icp-sdk/core/principal"
import { createIcAgent } from "@/lib/ic/agent"

export interface IndexActor {
  ledger_id(): Promise<Principal>
  status(): Promise<{ num_blocks_synced: bigint }>
}

export async function createIndexActor(host: string, canisterId: string): Promise<IndexActor> {
  const index = IcrcIndexCanister.create({
    agent: await createIcAgent(host),
    canisterId: Principal.fromText(canisterId),
  })
  return {
    ledger_id: () => index.ledgerId({ certified: false }),
    status: () => index.status({ certified: false }),
  }
}
