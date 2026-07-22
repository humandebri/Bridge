import { describe, expect, it } from "vitest"
import { readFile } from "node:fs/promises"
import path from "node:path"

describe("Base Sepolia asset profile template", () => {
  it("is visibly test-only and cannot reference production IDs", async () => {
    const template = JSON.parse(await readFile(path.resolve(import.meta.dirname, "../../deployments/sepolia-staging/frontend-profile.template.json"), "utf8"))
    expect(template).toMatchObject({ environment: "sepolia-staging", testOnly: true, chainId: 84532, evmRpcCanisterId: "7hfb6-caaaa-aaaar-qadga-cai" })
    expect(JSON.stringify(template)).not.toContain("rlhjx-iyaaa-aaaaf-qcnyq-cai")
    expect(JSON.stringify(template)).not.toContain("73mez-iiaaa-aaaaq-aaasq-cai")
    expect(JSON.stringify(template)).not.toContain("7vojr-tyaaa-aaaaq-aaatq-cai")
  })

  it("requires the validated Sepolia build before publishing the test Worker", async () => {
    const manifest = JSON.parse(await readFile(path.resolve(import.meta.dirname, "../package.json"), "utf8"))
    expect(manifest.scripts["deploy:test"]).toBe("pnpm run build:sepolia && node scripts/check-sepolia-assets.mjs && wrangler deploy --name kinic-bridge-ui-test")
  })
})
