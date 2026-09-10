import { Actor, HttpAgent } from "@icp-sdk/core/agent"
import { hexToBytes, toHex } from "viem"
import { z } from "zod"
import { idlFactory } from "../src/generated/bridge.idl"
import type { _SERVICE } from "../src/generated/bridge.did"
import {
  recoveryHash,
  recoveryRequestSchema,
  recoveryPageSchema,
} from "../src/lib/mint-recovery-api"

const origin = "https://bridge.kinic.xyz"
const profileSchema = z.object({
  chainId: z.literal(8453),
  testOnly: z.literal(false),
  icHost: z.literal("https://icp-api.io"),
  canisterSchemaVersion: z.literal(36),
  bridgeCanisterId: z.string().min(1),
  bridgeAddress: z.string().regex(/^0x[0-9a-f]{40}$/i),
  bsnsAddress: z.string().regex(/^0x[0-9a-f]{40}$/i),
  deploymentInstanceId: recoveryHash,
  deploymentBlock: z.coerce.bigint().nonnegative(),
})
const cursorSchema = z
  .object({
    binding: z.string(),
    fromBlock: z.string().regex(/^0x[0-9a-f]+$/),
    toBlock: z.string().regex(/^0x[0-9a-f]+$/),
    resumeFromBlock: z.string().regex(/^0x[0-9a-f]+$/),
    pageKey: z.string().min(1).max(2000),
    pageKeyExpiresAt: z.number().int(),
  })
  .strict()
const transfersSchema = z.object({
  transfers: z
    .array(z.object({ hash: recoveryHash, blockNum: z.string().regex(/^0x[0-9a-f]+$/) }))
    .max(100),
  pageKey: z.string().max(2000).optional(),
})
class ApiError extends Error {
  constructor(
    readonly status: number,
    readonly code: string,
  ) {
    super(code)
  }
}
const encode = (bytes: Uint8Array) =>
  btoa(String.fromCharCode(...bytes))
    .replaceAll("+", "-")
    .replaceAll("/", "_")
    .replace(/=+$/, "")
const decode = (text: string) =>
  Uint8Array.from(atob(text.replaceAll("-", "+").replaceAll("_", "/")), (c) => c.charCodeAt(0))
async function cursorKey(secret: string) {
  if (secret.length < 32) throw new ApiError(503, "configuration_unavailable")
  return crypto.subtle.importKey(
    "raw",
    new TextEncoder().encode(secret),
    { name: "HMAC", hash: "SHA-256" },
    false,
    ["sign", "verify"],
  )
}
async function signCursor(value: z.infer<typeof cursorSchema>, secret: string) {
  const payload = new TextEncoder().encode(JSON.stringify(value))
  return `${encode(payload)}.${encode(new Uint8Array(await crypto.subtle.sign("HMAC", await cursorKey(secret), payload)))}`
}
async function openCursor(token: string, secret: string, binding: string) {
  try {
    const parts = token.split(".")
    if (parts.length !== 2) throw new Error()
    const payload = decode(parts[0]!),
      signature = decode(parts[1]!)
    if (!(await crypto.subtle.verify("HMAC", await cursorKey(secret), signature, payload)))
      throw new Error()
    const value = cursorSchema.parse(JSON.parse(new TextDecoder().decode(payload)))
    if (value.binding !== binding) throw new Error()
    return value
  } catch (error) {
    if (error instanceof ApiError) throw error
    throw new ApiError(400, "invalid_cursor")
  }
}
async function limitedJson(body: ReadableStream<Uint8Array> | null, maximum: number) {
  if (!body) throw new ApiError(400, "invalid_body")
  const reader = body.getReader(),
    chunks: Uint8Array[] = []
  let size = 0
  try {
    while (true) {
      const part = await reader.read()
      if (part.done) break
      size += part.value.byteLength
      if (size > maximum) {
        await reader.cancel()
        throw new ApiError(413, "body_too_large")
      }
      chunks.push(part.value)
    }
    const bytes = new Uint8Array(size)
    let offset = 0
    for (const chunk of chunks) {
      bytes.set(chunk, offset)
      offset += chunk.length
    }
    return JSON.parse(new TextDecoder().decode(bytes)) as unknown
  } finally {
    reader.releaseLock()
  }
}

export async function handleMintRecovery(request: Request, env: Env): Promise<Response> {
  const started = Date.now()
  let status = 200
  const headers = {
    "Access-Control-Allow-Origin": origin,
    Vary: "Origin",
    "Cache-Control": "no-store",
  }
  try {
    if (request.headers.get("Origin") !== origin) throw new ApiError(403, "origin_denied")
    if (new URL(request.url).pathname !== "/v1/mint-recovery") throw new ApiError(404, "not_found")
    if (request.method === "OPTIONS")
      return new Response(null, {
        status: 204,
        headers: {
          ...headers,
          "Access-Control-Allow-Methods": "POST",
          "Access-Control-Allow-Headers": "Content-Type",
        },
      })
    if (request.method !== "POST") throw new ApiError(405, "method_not_allowed")
    if (env.RECOVERY_ENABLED !== "true") throw new ApiError(503, "recovery_disabled")
    const ip = request.headers.get("CF-Connecting-IP")
    if (!ip || !(await env.IP_LIMIT.limit({ key: ip })).success)
      throw new ApiError(429, "rate_limited")
    if (!request.headers.get("Content-Type")?.startsWith("application/json"))
      throw new ApiError(415, "content_type_required")
    let input: unknown
    try {
      input = await limitedJson(request.body, 4096)
    } catch (error) {
      if (error instanceof ApiError) throw error
      throw new ApiError(400, "invalid_json")
    }
    const parsed = recoveryRequestSchema.safeParse(input)
    if (!parsed.success) throw new ApiError(400, "invalid_request")
    const depositId = parsed.data.depositId.toLowerCase()
    if (!(await env.DEPOSIT_LIMIT.limit({ key: depositId })).success)
      throw new ApiError(429, "rate_limited")
    const profile = profileSchema.parse(JSON.parse(env.BRIDGE_PROFILE_JSON))
    const controller = new AbortController()
    const timer = setTimeout(() => controller.abort(), 15_000)
    const work = async () => {
      const rpc = async (method: string, params: unknown[]) => {
        const response = await fetch(
          `https://base-mainnet.g.alchemy.com/v2/${env.ALCHEMY_API_KEY}`,
          {
            method: "POST",
            headers: { "Content-Type": "application/json" },
            signal: controller.signal,
            body: JSON.stringify({ jsonrpc: "2.0", id: 1, method, params }),
          },
        )
        if (!response.ok)
          throw new ApiError(response.status === 429 ? 429 : 502, "upstream_unavailable")
        const data = z
          .object({ result: z.unknown(), error: z.unknown().optional() })
          .parse(await limitedJson(response.body, 512_000))
        if (data.error || data.result === undefined) throw new ApiError(502, "upstream_unavailable")
        return data.result
      }
      const agent = HttpAgent.createSync({
        host: profile.icHost,
        fetchOptions: { signal: controller.signal },
        retryTimes: 0,
      })
      const actor = Actor.createActor<_SERVICE>(idlFactory, {
        agent,
        canisterId: profile.bridgeCanisterId,
      })
      const runtime = await actor.get_runtime_binding()
      if (
        runtime.schema_version !== 36 ||
        runtime.base_chain_id !== 8453n ||
        toHex(Uint8Array.from(runtime.deployment_instance_id)).toLowerCase() !==
          profile.deploymentInstanceId.toLowerCase() ||
        toHex(Uint8Array.from(runtime.bridge_contract)).toLowerCase() !==
          profile.bridgeAddress.toLowerCase()
      )
        throw new ApiError(409, "runtime_binding_mismatch")
      const record = (await actor.get_deposit(hexToBytes(depositId as `0x${string}`)))[0]
      const authorization = record?.mint_authorization[0]
      if (!record || !authorization?.signature[0]) throw new ApiError(404, "deposit_unavailable")
      const bytesHex = (bytes: Uint8Array | number[]) => toHex(Uint8Array.from(bytes))
      if (
        bytesHex(record.deposit_id) !== depositId ||
        authorization.chain_id !== 8453n ||
        bytesHex(authorization.verifying_contract).toLowerCase() !==
          profile.bridgeAddress.toLowerCase()
      )
        throw new ApiError(409, "deposit_binding_mismatch")
      const digest = bytesHex(authorization.digest),
        recipient = bytesHex(authorization.recipient)
      const from =
        authorization.finalized_block_number > profile.deploymentBlock
          ? authorization.finalized_block_number
          : profile.deploymentBlock
      const binding = JSON.stringify([
        profile.deploymentInstanceId.toLowerCase(),
        profile.bridgeCanisterId,
        profile.bridgeAddress.toLowerCase(),
        profile.bsnsAddress.toLowerCase(),
        depositId,
        digest,
        recipient,
        from.toString(),
      ])
      const cursor = parsed.data.cursor
        ? await openCursor(parsed.data.cursor, env.CURSOR_KEY, binding)
        : undefined
      const toBlock =
        cursor?.toBlock ??
        z
          .string()
          .regex(/^0x[0-9a-f]+$/)
          .parse(await rpc("eth_blockNumber", []))
      if (BigInt(toBlock) < from) throw new ApiError(503, "head_unavailable")
      if (
        cursor &&
        (BigInt(cursor.fromBlock) < from ||
          BigInt(cursor.resumeFromBlock) < BigInt(cursor.fromBlock) ||
          BigInt(cursor.resumeFromBlock) > BigInt(toBlock))
      )
        throw new ApiError(400, "invalid_cursor_range")
      // The signed block checkpoint survives pageKey expiry. Replay the boundary
      // block inclusively because its transfers may span more than one page.
      const pageKey = cursor && cursor.pageKeyExpiresAt > Date.now() ? cursor.pageKey : undefined
      const fromBlock = cursor
        ? pageKey
          ? cursor.fromBlock
          : cursor.resumeFromBlock
        : `0x${from.toString(16)}`
      const result = transfersSchema.parse(
        await rpc("alchemy_getAssetTransfers", [
          {
            fromBlock,
            toBlock,
            toAddress: recipient,
            contractAddresses: [profile.bsnsAddress],
            category: ["erc20"],
            order: "asc",
            maxCount: "0x64",
            excludeZeroValue: true,
            ...(pageKey ? { pageKey } : {}),
          },
        ]),
      )
      let lastBlock = BigInt(pageKey && cursor ? cursor.resumeFromBlock : fromBlock)
      for (const transfer of result.transfers) {
        const block = BigInt(transfer.blockNum)
        if (block < lastBlock || block > BigInt(toBlock))
          throw new ApiError(502, "invalid_transfer_range")
        lastBlock = block
      }
      if (result.pageKey && !result.transfers.length) throw new ApiError(502, "empty_transfer_page")
      return recoveryPageSchema.parse({
        deploymentInstanceId: profile.deploymentInstanceId,
        depositId,
        authorizationDigest: digest,
        hashes: [...new Set(result.transfers.map((entry) => entry.hash.toLowerCase()))],
        cursor: result.pageKey
          ? await signCursor(
              {
                binding,
                fromBlock,
                toBlock,
                resumeFromBlock: `0x${lastBlock.toString(16)}`,
                pageKey: result.pageKey,
                pageKeyExpiresAt: Date.now() + 540_000,
              },
              env.CURSOR_KEY,
            )
          : null,
      })
    }
    try {
      const timeout = new Promise<never>((_, reject) =>
        controller.signal.addEventListener(
          "abort",
          () => reject(new ApiError(504, "upstream_timeout")),
          { once: true },
        ),
      )
      return Response.json(await Promise.race([work(), timeout]), { headers })
    } finally {
      clearTimeout(timer)
    }
  } catch (error) {
    status = error instanceof ApiError ? error.status : 502
    return Response.json(
      { error: error instanceof ApiError ? error.code : "recovery_unavailable" },
      { status, headers: { ...headers, ...(status === 429 ? { "Retry-After": "60" } : {}) } },
    )
  } finally {
    console.log(JSON.stringify({ event: "mint_recovery", status, elapsedMs: Date.now() - started }))
  }
}
export default { fetch: handleMintRecovery }
