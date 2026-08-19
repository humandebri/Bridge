#!/usr/bin/env node
import { createPrivateKey } from "node:crypto"
import { readFile } from "node:fs/promises"
import { IDL as LegacyIDL, lebEncode } from "@dfinity/candid"
import { Principal } from "@dfinity/principal"
import {
  Certificate,
  Cbor,
  IC_REQUEST_DOMAIN_SEPARATOR,
  HttpAgent,
  type Identity,
  type SignIdentity,
  type HttpAgentRequest,
  lookupResultToBuffer,
  requestIdOf,
} from "@icp-sdk/core/agent"
import { Ed25519KeyIdentity } from "@icp-sdk/core/identity"
import { Secp256k1KeyIdentity } from "@icp-sdk/core/identity/secp256k1"
import { idlFactory } from "../integration/generated/bridge.idl"
import { concatBytes } from "@noble/hashes/utils"
import { setTimeout as sleep } from "node:timers/promises"

const TE = new TextEncoder()
const BRIDGE_SERVICE = idlFactory({ IDL: LegacyIDL })
const HELP_TEXT = `usage: node manual-confirm-with-cycles.ts \\
  --host <ic-host> \\
  --canister <bridge-canister-id> \\
  --identity-pem <path-to-identity-pem> \\
  --operation-id <nat64> \\
  --tx-hash <0x.. or hex> \\
  --cycles <integer> [options]\n\noptional:
  --query-interval-ms <ms> (default: 4000)
  --max-polls <count> (default: 60)
  --method <method> (default: confirm_base_governance_transaction)
`

async function main(): Promise<void> {
  const args = parseArgs(process.argv.slice(2))

  const host = required(args, "host")
  const canisterId = required(args, "canister")
  const identityPemPath = required(args, "identity-pem")
  const operationId = BigInt(required(args, "operation-id"))
  const txHash = parseHexBytes(required(args, "tx-hash"))
  const cycles = BigInt(required(args, "cycles"))
  const method = args["method"] ?? "confirm_base_governance_transaction"
  const queryIntervalMs = Number(args["query-interval-ms"] ?? "4000")
  const maxPolls = Number(args["max-polls"] ?? "60")

  const identityPem = await readFile(identityPemPath, "utf8")
  const signer = await identityFromPem(identityPem)
  const cyclesAwareIdentity = new CyclesAwareIdentity(signer)

  const agent = HttpAgent.createSync({ host, identity: cyclesAwareIdentity })
  if (agent.isLocal()) await agent.fetchRootKey()

  const callArg = encodeConfirmArgs(operationId, txHash, method)
  const callArgs = {
    methodName: method,
    arg: callArg,
    callSync: false,
  }

  agent.addTransform("update", (request) => {
    const body = request.body
    if (!body || typeof body !== "object") return request
    return {
      ...request,
      body: {
        ...body,
        cycles: encodeCycles(cycles),
      },
    } as HttpAgentRequest
  }, 2)

  const before = await fetchBridgeStatus(agent, canisterId)
  logStatus("before", before)

  const submit = await agent.call(canisterId, callArgs)
  console.log("submit:", {
    request_id: toHex(submit.requestId),
    status: submit.response.status,
    ok: submit.response.ok,
    body: submit.response.body,
  })

  const result = await pollForUpdateResult(agent, canisterId, submit.requestId, {
    queryIntervalMs,
    maxPolls,
  })

  const after = await fetchBridgeStatus(agent, canisterId)
  logStatus("after", after)

  if (result.kind === "reply") {
    const value = decodeConfirmReply(result.data, method)
    console.log("update-result:", JSON.stringify(toJsonSafe(value), null, 2))
    return
  }

  console.log("update-result:", {
    reject: toJsonSafe(Cbor.decode(result.data)),
  })
}

function parseArgs(input: string[]): Record<string, string> {
  const values: Record<string, string> = {}
  for (let i = 0; i < input.length; i++) {
    const token = input[i]
    if (token === "--help") {
      throw new Error(HELP_TEXT)
    }
    if (!token.startsWith("--")) {
      throw new Error(`Unknown argument format: ${token}\n${HELP_TEXT}`)
    }
    const key = token.slice(2)
    const next = input[i + 1]
    if (!next || next.startsWith("--")) {
      throw new Error(`Missing value for --${key}`)
    }
    values[key] = next
    i++
  }
  if (Object.keys(values).length === 0) {
    throw new Error(HELP_TEXT)
  }
  return values
}

function required(input: Record<string, string>, key: string): string {
  const value = input[key]
  if (!value) throw new Error(`Missing required option: --${key}`)
  return value
}

function parseHexBytes(value: string): Uint8Array {
  const normalized = value.startsWith("0x") ? value.slice(2) : value
  if (normalized.length === 0 || normalized.length % 2 === 1 || !/^[0-9a-fA-F]+$/.test(normalized)) {
    throw new Error(`Invalid hex value: ${value}`)
  }
  const out = new Uint8Array(normalized.length / 2)
  for (let i = 0; i < out.length; i++) {
    out[i] = Number.parseInt(normalized.slice(i * 2, i * 2 + 2), 16)
  }
  return out
}

function encodeCycles(cycles: bigint): Uint8Array {
  if (cycles < 0n) throw new Error("cycles must be >= 0")
  return lebEncode(cycles)
}

function encodeConfirmArgs(operationId: bigint, transactionHash: Uint8Array, method: string): Uint8Array {
  const bridgeMethod = BRIDGE_SERVICE._fields.find(([name]) => name === method)?.[1]
  if (!bridgeMethod) throw new Error(`${method} is missing from generated bridge candid`)

  return new Uint8Array(LegacyIDL.encode(bridgeMethod.argTypes, [{ transaction_hash: transactionHash, operation_id: operationId }]))
}

function decodeConfirmReply(raw: Uint8Array, method: string): unknown {
  const bridgeMethod = BRIDGE_SERVICE._fields.find(([name]) => name === method)?.[1]
  if (!bridgeMethod) throw new Error(`${method} is missing from generated bridge candid`)
  return LegacyIDL.decode(bridgeMethod.retTypes, raw)[0]
}

async function fetchBridgeStatus(agent: HttpAgent, canisterId: string): Promise<Record<string, unknown>> {
  const method = BRIDGE_SERVICE._fields.find(([name]) => name === "get_bridge_status")?.[1]
  if (!method) throw new Error("get_bridge_status is missing from generated bridge candid")
  const query = await agent.query(canisterId, {
    methodName: "get_bridge_status",
    arg: new Uint8Array(),
  })
  if (query.status !== "replied") {
    throw new Error(`query get_bridge_status rejected: ${query.reject_code} / ${query.reject_message}`)
  }
  return LegacyIDL.decode(method.retTypes, query.reply.arg)[0] as Record<string, unknown>
}

function logStatus(prefix: string, status: Record<string, unknown>): void {
  const lastBaseBlock = status.last_finalized_base_block
  const observedNs = status.last_finalized_observation_ns
  const requiredCycles = status.last_observation_required_cycles ?? status.required_cycles
  console.log(`${prefix} status:`, {
    last_finalized_base_block: lastBaseBlock,
    last_finalized_observation_ns: observedNs,
    required_cycles: requiredCycles,
  })
}

async function pollForUpdateResult(
  agent: HttpAgent,
  canisterId: string,
  requestId: Uint8Array,
  options: { queryIntervalMs: number; maxPolls: number },
): Promise<{ kind: "reply"; data: Uint8Array } | { kind: "reject"; data: Uint8Array }> {
  if (!agent.rootKey) {
    await agent.fetchRootKey()
  }

  const paths = {
    status: [TE.encode("request_status"), requestId, TE.encode("status")],
    reply: [TE.encode("request_status"), requestId, TE.encode("reply")],
    reject: [TE.encode("request_status"), requestId, TE.encode("reject")],
  } as const

  for (let i = 0; i < options.maxPolls; i++) {
    const readState = await agent.readState(canisterId, {
      paths: Object.values(paths),
    })
    const certificate = await Certificate.create({
      certificate: readState.certificate,
      rootKey: agent.rootKey!,
      canisterId: Principal.fromText(canisterId),
    })

    const reply = lookupResultToBuffer(certificate.lookup_path(paths.reply))
    if (reply) {
      return { kind: "reply", data: reply }
    }

    const reject = lookupResultToBuffer(certificate.lookup_path(paths.reject))
    if (reject) {
      return { kind: "reject", data: reject }
    }

    const status = lookupResultToBuffer(certificate.lookup_path(paths.status))
    if (status) {
      const statusText = new TextDecoder().decode(status)
      console.log(`poll #${i + 1}: request status = ${statusText}`)
    }

    await sleep(options.queryIntervalMs)
  }

  throw new Error("Timed out waiting for canister update result")
}

async function identityFromPem(pem: string): Promise<SignIdentity> {
  const key = createPrivateKey(pem)
  const jwk = key.export({ format: "jwk" }) as { crv?: string; d?: string; [k: string]: unknown }

  if (!jwk.d) throw new Error("PEM must contain a private key")
  const secretKey = new Uint8Array(Buffer.from(jwk.d, "base64url"))

  if (key.asymmetricKeyType === "ec" && jwk.crv === "secp256k1") {
    return Secp256k1KeyIdentity.fromSecretKey(secretKey)
  }
  if (key.asymmetricKeyType === "ed25519" && jwk.crv === "Ed25519") {
    return Ed25519KeyIdentity.fromSecretKey(secretKey)
  }
  throw new Error(`Unsupported PEM key type: ${jwk.crv ?? key.asymmetricKeyType}`)
}

class CyclesAwareIdentity implements Identity {
  public constructor(private readonly inner: SignIdentity) {}

  public getPrincipal() {
    return this.inner.getPrincipal()
  }

  public async transformRequest(request: HttpAgentRequest): Promise<unknown> {
    if (!request || typeof request !== "object" || !("body" in request) || !request.body || typeof request.body !== "object") {
      return request
    }

    const bodyForSigning = { ...request.body }
    delete (bodyForSigning as { cycles?: Uint8Array }).cycles
    const requestId = requestIdOf(bodyForSigning as Record<string, unknown>)

    return {
      ...request,
      body: {
        content: request.body,
        sender_pubkey: this.inner.getPublicKey().toDer(),
        sender_sig: await this.inner.sign(concatBytes(IC_REQUEST_DOMAIN_SEPARATOR, requestId)),
      },
    }
  }
}

function toHex(value: Uint8Array): string {
  return Array.from(value)
    .map((byte) => byte.toString(16).padStart(2, "0"))
    .join("")
}

function toJsonSafe(value: unknown): unknown {
  return JSON.parse(JSON.stringify(value, (_key, item) => {
    return typeof item === "bigint" ? item.toString() : item
  }))
}

main().catch((error) => {
  const message = error instanceof Error ? error.message : `${error}`
  if (message.includes("usage: node")) {
    console.log(HELP_TEXT)
    process.exit(0)
  }
  console.error(error)
  process.exit(1)
})
