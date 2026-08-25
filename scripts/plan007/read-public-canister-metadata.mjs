#!/usr/bin/env node

import { Certificate, HttpAgent, LookupPathStatus } from "@icp-sdk/core/agent"
import { Principal } from "@icp-sdk/core/principal"
import path from "node:path"
import { fileURLToPath } from "node:url"

const encoder = new TextEncoder()
const decoder = new TextDecoder("utf-8", { fatal: true })

export function publicMetadataPath(canisterId, metadataName) {
  if (typeof metadataName !== "string" || metadataName.length === 0) {
    throw new Error("metadata name must be nonempty")
  }
  return [
    encoder.encode("canister"),
    canisterId.toUint8Array(),
    encoder.encode("metadata"),
    encoder.encode(metadataName),
  ]
}

export function classifyMetadataLookup(result) {
  switch (result.status) {
    case LookupPathStatus.Found:
      return { status: "present", value: decoder.decode(result.value) }
    case LookupPathStatus.Absent:
      return { status: "absent" }
    case LookupPathStatus.Unknown:
      throw new Error("certified metadata lookup returned an unknown path")
    case LookupPathStatus.Error:
      throw new Error("certified metadata lookup returned an invalid path")
    default:
      throw new Error("certified metadata lookup returned an unsupported status")
  }
}

export async function readPublicCanisterMetadata(
  host,
  canisterText,
  metadataName,
  { agent: providedAgent, createCertificate = Certificate.create } = {},
) {
  const canisterId = Principal.fromText(canisterText)
  const lookupPath = publicMetadataPath(canisterId, metadataName)
  const agent = providedAgent ?? await HttpAgent.create({ host })
  const response = await agent.readState(canisterId, { paths: [lookupPath] })
  if (!(agent.rootKey instanceof Uint8Array)) {
    throw new Error("IC agent did not expose a root key")
  }
  const certificate = await createCertificate({
    certificate: response.certificate,
    rootKey: agent.rootKey,
    principal: { canisterId },
    agent,
  })
  return classifyMetadataLookup(certificate.lookup_path(lookupPath))
}

async function main() {
  const [, , host, canisterId, metadataName] = process.argv
  if (!host || !canisterId || !metadataName || process.argv.length !== 5) {
    throw new Error(
      "usage: read-public-canister-metadata.mjs <ic-host> <canister-id> <metadata-name>",
    )
  }
  const result = await readPublicCanisterMetadata(host, canisterId, metadataName)
  process.stdout.write(`${JSON.stringify(result)}\n`)
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  await main()
}
