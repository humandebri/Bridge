import { createHash } from "node:crypto"
import { readFile } from "node:fs/promises"
import path from "node:path"
import { fileURLToPath } from "node:url"

function deployedBytecode(artifact) {
  const object = artifact?.deployedBytecode?.object
  if (typeof object !== "string" || !/^0x[0-9a-fA-F]*$/.test(object) || object.length % 2 !== 0) {
    throw new Error("artifact has invalid deployed bytecode")
  }
  return Buffer.from(object.slice(2), "hex")
}

function immutableRanges(artifact, byteLength) {
  const references = artifact?.deployedBytecode?.immutableReferences
  if (!references || typeof references !== "object" || Array.isArray(references)) {
    throw new Error("artifact has no immutable reference metadata")
  }
  const ranges = Object.values(references).flat()
  for (const range of ranges) {
    if (
      !range
      || !Number.isSafeInteger(range.start)
      || !Number.isSafeInteger(range.length)
      || range.start < 0
      || range.length <= 0
      || range.start + range.length > byteLength
    ) {
      throw new Error("artifact has an invalid immutable reference range")
    }
  }
  return ranges
}

function normalizedTemplate(artifact, runtimeHex) {
  const template = deployedBytecode(artifact)
  const ranges = immutableRanges(artifact, template.length)
  const runtime = runtimeHex === undefined
    ? Buffer.from(template)
    : runtimeBytes(runtimeHex, template.length)
  const mutable = new Uint8Array(template.length)
  for (const { start, length } of ranges) mutable.fill(1, start, start + length)
  for (let index = 0; index < template.length; index += 1) {
    if (mutable[index] === 0 && runtime[index] !== template[index]) {
      throw new Error(`deployed runtime differs outside immutable references at byte ${index}`)
    }
  }
  for (const { start, length } of ranges) runtime.fill(0, start, start + length)
  return runtime
}

function runtimeBytes(value, expectedLength) {
  if (typeof value !== "string" || !/^0x[0-9a-fA-F]*$/.test(value) || value.length % 2 !== 0) {
    throw new Error("deployed runtime must be 0x-prefixed hex")
  }
  const runtime = Buffer.from(value.slice(2), "hex")
  if (runtime.length !== expectedLength) {
    throw new Error(`deployed runtime length ${runtime.length} differs from artifact ${expectedLength}`)
  }
  return runtime
}

export function runtimeTemplateSha256(artifact, runtimeHex) {
  return `0x${createHash("sha256").update(normalizedTemplate(artifact, runtimeHex)).digest("hex")}`
}

export async function runtimeTemplateSha256FromFile(artifactPath, runtimeHex) {
  const artifact = JSON.parse(await readFile(artifactPath, "utf8"))
  return runtimeTemplateSha256(artifact, runtimeHex)
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const [, , artifactPath, runtimeHex] = process.argv
  if (!artifactPath || !runtimeHex) {
    throw new Error("usage: runtime-template-hash.mjs <artifact.json> <deployed-runtime-hex>")
  }
  process.stdout.write(`${await runtimeTemplateSha256FromFile(artifactPath, runtimeHex)}\n`)
}
