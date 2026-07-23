import { readFile, writeFile } from "node:fs/promises"
import { resolve } from "node:path"

const root = resolve(import.meta.dirname, "../..")
const outputDirectory = resolve(root, "ui/src/generated/abi")
const inputs = [
  ["Bridge", "bridgeAbi"],
  ["BSNS", "bsnsAbi"],
]
const check = process.argv.includes("--check")

for (const [contract, exportName] of inputs) {
  const source = await readFile(resolve(root, `contracts/abi/${contract}.json`), "utf8")
  const generated = `/* Generated from contracts/abi/${contract}.json. Do not edit. */\nexport const ${exportName} = ${JSON.stringify(JSON.parse(source), null, 2)} as const\n`
  const destination = resolve(outputDirectory, `${contract.toLowerCase()}.generated.ts`)
  if (check) {
    const existing = await readFile(destination, "utf8").catch(() => "")
    if (existing !== generated) throw new Error(`${destination} is stale; run pnpm codegen:abi`)
  } else {
    await writeFile(destination, generated)
  }
}
