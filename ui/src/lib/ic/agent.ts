import { HttpAgent } from "@dfinity/agent"

export async function createIcAgent(host: string): Promise<HttpAgent> {
  const agent = HttpAgent.createSync({ host })
  if (agent.isLocal()) await agent.fetchRootKey()
  return agent
}
