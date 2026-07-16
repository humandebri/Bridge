import { spawnSync } from "node:child_process"
import { resolve } from "node:path"

const script = resolve(import.meta.dirname, "../../scripts/generate-candid-bindings.mjs")
const result = spawnSync(process.execPath, [script, ...process.argv.slice(2)], { stdio: "inherit" })
if (result.status !== 0) process.exit(result.status ?? 1)
