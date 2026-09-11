import { HttpAgent, type Identity } from "@icp-sdk/core/agent"

export async function createIcAgent(
  host: string,
  identity?: Identity,
  signal?: AbortSignal,
): Promise<HttpAgent> {
  const agent = HttpAgent.createSync({
    host,
    identity,
    ...(signal ? { fetchOptions: { signal }, retryTimes: 0 } : {}),
  })
  if (agent.isLocal()) await agent.fetchRootKey()
  return agent
}
