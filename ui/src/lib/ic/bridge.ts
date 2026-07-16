import { Actor, type ActorSubclass } from "@dfinity/agent"
import { Principal } from "@dfinity/principal"
import type { _SERVICE } from "@/generated/bridge.did"
import { idlFactory } from "@/generated/bridge.idl"
import { createIcAgent } from "@/lib/ic/agent"

export async function createBridgeActor(host: string, canisterId: string): Promise<ActorSubclass<_SERVICE>> {
  const agent = await createIcAgent(host)
  return Actor.createActor<_SERVICE>(idlFactory, { agent, canisterId: Principal.fromText(canisterId) })
}
