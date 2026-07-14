import { Actor, HttpAgent, type ActorSubclass } from "@dfinity/agent"
import { Principal } from "@dfinity/principal"
import type { _SERVICE } from "@/generated/bridge.did"
import { idlFactory } from "@/generated/bridge.idl"

export async function createBridgeActor(host: string, canisterId: string): Promise<ActorSubclass<_SERVICE>> {
  const agent = HttpAgent.createSync({ host })
  if (agent.isLocal()) await agent.fetchRootKey()
  return Actor.createActor<_SERVICE>(idlFactory, { agent, canisterId: Principal.fromText(canisterId) })
}
