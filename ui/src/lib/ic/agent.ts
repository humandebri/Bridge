import { HttpAgent, type Identity } from "@dfinity/agent"

export async function createIcAgent(host: string, identity?: Identity): Promise<HttpAgent> {
  const agent = HttpAgent.createSync({ host, identity })
  if (agent.isLocal()) await agent.fetchRootKey()
  return agent
}
