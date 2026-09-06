import { describe, expect, it } from "vitest"
import { readFile } from "node:fs/promises"
import path from "node:path"

describe("Base Sepolia asset profile template", () => {
  it("is visibly test-only and cannot reference production IDs", async () => {
    const template = JSON.parse(
      await readFile(
        path.resolve(
          import.meta.dirname,
          "../../deployments/sepolia-staging/frontend-profile.template.json",
        ),
        "utf8",
      ),
    )
    expect(template).toMatchObject({
      environment: "sepolia-staging",
      testOnly: true,
      chainId: 84532,
      evmRpcCanisterId: "7hfb6-caaaa-aaaar-qadga-cai",
    })
    expect(JSON.stringify(template)).not.toContain("73mez-iiaaa-aaaaq-aaasq-cai")
    expect(JSON.stringify(template)).not.toContain("7vojr-tyaaa-aaaaq-aaatq-cai")
  })

  it("requires the validated Sepolia build before publishing the test Worker", async () => {
    const manifest = JSON.parse(
      await readFile(path.resolve(import.meta.dirname, "../package.json"), "utf8"),
    )
    expect(manifest.scripts["deploy:test"]).toBe(
      "pnpm run build:sepolia && node scripts/check-sepolia-assets.mjs && wrangler deploy --name kinic-bridge-ui-test",
    )
  })

  it("can publish the staging Worker from a frozen artifact receipt without rebuilding", async () => {
    const manifest = JSON.parse(
      await readFile(path.resolve(import.meta.dirname, "../package.json"), "utf8"),
    )
    expect(manifest.scripts["artifact:test"]).toBe(
      'scripts/run-staging-assets.sh generate "$BRIDGE_STAGING_UI_RECEIPT"',
    )
    expect(manifest.scripts["artifact:test:verify"]).toBe(
      'scripts/run-staging-assets.sh verify "$BRIDGE_STAGING_UI_RECEIPT"',
    )
    expect(manifest.scripts["deploy:test:artifact"]).toBe(
      'scripts/run-staging-assets.sh deploy "$BRIDGE_STAGING_UI_RECEIPT"',
    )
    expect(manifest.scripts["deploy:test:artifact"]).not.toContain("build:sepolia")
  })

  it("deploys production only from the Gate B UI artifact receipt", async () => {
    const manifest = JSON.parse(
      await readFile(path.resolve(import.meta.dirname, "../package.json"), "utf8"),
    )
    expect(manifest.scripts.deploy).toContain("production-assets.mjs deploy")
    expect(manifest.scripts.deploy).toContain("$BRIDGE_RELEASE_BUNDLE/ui-assets.json")
    expect(manifest.scripts.deploy).toContain("$BRIDGE_UI_RUNTIME_PROFILE_FILE")
    expect(manifest.scripts.deploy).not.toContain("pnpm run build && wrangler deploy")
  })
})
