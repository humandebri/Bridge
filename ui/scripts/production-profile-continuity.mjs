export const productionProfileUrl = "https://bridge.kinic.xyz/deployment-profile.js"

const maxBootstrapBytes = 256 * 1024
const bootstrapPattern =
  /^globalThis\.__KINIC_DEPLOYMENT_PROFILE_JSON__ = ("(?:[^"\\]|\\[\s\S])*");\n?$/

/** @param {string} publicRaw */
export function deploymentProfileBootstrap(publicRaw) {
  return `globalThis.__KINIC_DEPLOYMENT_PROFILE_JSON__ = ${JSON.stringify(publicRaw)};\n`
}

/** @param {string} script */
export function parseProductionProfileBootstrap(script) {
  const match = bootstrapPattern.exec(script)
  if (!match) throw new Error("Production deployment profile bootstrap has an unexpected shape")
  const raw = JSON.parse(match[1])
  const profile = JSON.parse(raw)
  if (!profile || Array.isArray(profile) || typeof profile !== "object") {
    throw new Error("Production deployment profile bootstrap does not contain an object")
  }
  return raw
}

/** @param {string} expectedPublicRaw @param {typeof fetch} [fetchImpl] */
export async function requireUnchangedProductionProfile(expectedPublicRaw, fetchImpl = fetch) {
  const response = await fetchImpl(productionProfileUrl, {
    headers: {
      accept: "application/javascript",
      "cache-control": "no-cache",
      pragma: "no-cache",
    },
    redirect: "manual",
  })
  if (response.status !== 200 || response.redirected || response.url !== productionProfileUrl) {
    throw new Error("Production deployment profile could not be read without a redirect")
  }
  const declaredLength = Number(response.headers.get("content-length") ?? 0)
  if (declaredLength > maxBootstrapBytes) {
    throw new Error("Production deployment profile bootstrap is too large")
  }
  if (!response.body) throw new Error("Production deployment profile bootstrap is empty")
  const reader = response.body.getReader()
  const chunks = []
  let length = 0
  while (true) {
    const { done, value } = await reader.read()
    if (done) break
    length += value.byteLength
    if (length > maxBootstrapBytes) {
      await reader.cancel()
      throw new Error("Production deployment profile bootstrap is too large")
    }
    chunks.push(value)
  }
  const bytes = new Uint8Array(length)
  let offset = 0
  for (const chunk of chunks) {
    bytes.set(chunk, offset)
    offset += chunk.byteLength
  }
  const currentPublicRaw = parseProductionProfileBootstrap(
    new TextDecoder("utf-8", { fatal: true }).decode(bytes),
  )
  if (currentPublicRaw !== expectedPublicRaw) {
    throw new Error("Asset-only deploy cannot change the production deployment profile")
  }
}
