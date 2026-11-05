import { mkdir, readFile, writeFile } from "node:fs/promises"
import path from "node:path"
import { fileURLToPath } from "node:url"
import { createRequire } from "node:module"
import { build } from "vite"

// This diagnostic uses the same public build switches as production-assets.mjs.
// It uses a dummy public project ID and never connects to a wallet or publishes assets.
const uiRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..")
const destination = process.argv[2]
if (!destination || !path.isAbsolute(destination) || destination.startsWith(uiRoot + path.sep)) {
  throw new Error("Supply an absolute output directory outside the UI source tree")
}
process.env.KINIC_GENERIC_PRODUCTION_UI_BUILD = "1"
process.env.VITE_DEPLOYMENT_PROFILE_JSON = ""
process.env.VITE_WALLETCONNECT_PROJECT_ID = "00000000000000000000000000000000"
/** @type {{id:string, renderedLength:number, importers:string[], dynamicImporters:string[]}[]} */
const modules = []
/** @type {import('vite').Plugin} */
const trace = {
  name: "review-wallet-dependency-boundary",
  generateBundle(_options, bundle) {
    for (const item of Object.values(bundle)) {
      if (item.type !== "chunk") continue
      for (const [id, details] of Object.entries(item.modules)) {
        const info = this.getModuleInfo(id)
        modules.push({
          id,
          renderedLength: details.renderedLength,
          importers: info?.importers ?? [],
          dynamicImporters: info?.dynamicImporters ?? [],
        })
      }
    }
  },
}
await mkdir(destination, { recursive: true })
await build({
  root: uiRoot,
  configFile: path.join(uiRoot, "vite.config.ts"),
  plugins: [trace],
  build: { outDir: path.join(destination, "assets"), sourcemap: true, emptyOutDir: false },
})
const rendered = modules.filter((item) => item.renderedLength > 0)
await writeFile(path.join(destination, "modules.json"), JSON.stringify(modules, null, 2) + "\n")
const frame = rendered.find((item) => item.id.endsWith("/src/W3mFrame.js"))
const zod3 = rendered.filter((item) => /\/zod@3\.22\.4\//.test(item.id))
const cdp = rendered.filter((item) => item.id.includes("/node_modules/@coinbase/cdp-sdk/"))
const zod325 = rendered.filter((item) => /\/zod@3\.25\.76\//.test(item.id))
const anyZod3 = rendered.filter((item) => /\/zod@3\.[^/]+\/node_modules\/zod\//.test(item.id))
const walletConnect = rendered.some((item) =>
  item.id.includes("/node_modules/@walletconnect/ethereum-provider/dist/"),
)
if (!walletConnect) throw new Error("WalletConnect provider is missing from the diagnostic build")
const localRequire = createRequire(import.meta.url)
const wcRequire = createRequire(localRequire.resolve("@walletconnect/ethereum-provider"))
const appkitRequire = createRequire(wcRequire.resolve("@reown/appkit/core"))
const framePath = path.resolve(
  path.dirname(appkitRequire.resolve("@reown/appkit-wallet")),
  "../src/W3mFrame.js",
)
const frameSource = await readFile(framePath, "utf8")
if (
  !frameSource.includes("event.source === this.iframe.contentWindow") ||
  !frameSource.includes("event.origin === new URL(SECURE_SITE_SDK).origin")
) {
  throw new Error("Rendered Reown frame module is missing the reviewed source/origin boundary")
}
if (cdp.length || zod325.length)
  throw new Error("Node-only CDP/Zod3.25 entered the browser build; review its input boundary")
if (frame || anyZod3.length)
  throw new Error(
    "The production core-only wallet graph now includes the auth parser; review its input boundary",
  )
const summary = {
  walletconnect_enabled: true,
  reown_frame_rendered: Boolean(frame),
  reviewed_frame_source_guard: true,
  zod_3_22_rendered_modules: zod3.length,
  cdp_rendered_modules: cdp.length,
  zod_3_25_rendered_modules: zod325.length,
  all_zod_3_rendered_modules: anyZod3.length,
  remaining_assumption:
    "The core-only wallet graph excludes both Zod3 parser roots. The dormant frame patch trusts its configured Reown iframe and is not a general Zod resource bound.",
}
await writeFile(path.join(destination, "modules.json"), JSON.stringify(modules, null, 2) + "\n")
await writeFile(path.join(destination, "summary.json"), JSON.stringify(summary, null, 2) + "\n")
console.log(JSON.stringify(summary))
