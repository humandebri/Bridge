import { Actor, type ActorSubclass, type Identity } from "@icp-sdk/core/agent"
import { Principal } from "@icp-sdk/core/principal"
import type { _SERVICE } from "@/generated/bridge.did"
import { idlFactory } from "@/generated/bridge.idl"
import { createIcAgent } from "@/lib/ic/agent"

export async function createBridgeActor(
  host: string,
  canisterId: string,
  identity?: Identity,
): Promise<ActorSubclass<_SERVICE>> {
  const agent = await createIcAgent(host, identity)
  return Actor.createActor<_SERVICE>(idlFactory, {
    agent,
    canisterId: Principal.fromText(canisterId),
  })
}
