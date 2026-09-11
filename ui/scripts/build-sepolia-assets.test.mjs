import { describe, expect, it } from "vitest"
import { spawnSync } from "node:child_process"
import { readFile } from "node:fs/promises"
import path from "node:path"

describe("Base Sepolia asset profile template", () => {
  it("keeps the recovery release config valid for JSON.parse", async () => {
    const config = JSON.parse(
      await readFile(
        path.resolve(import.meta.dirname, "../recovery-worker/wrangler.jsonc"),
        "utf8",
      ),
    )
    expect(config.name).toBe("kinic-bridge-mint-recovery")
  })

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

  it("deploys production from a standalone UI receipt and live v36 upgrade evidence", async () => {
    const manifest = JSON.parse(
      await readFile(path.resolve(import.meta.dirname, "../package.json"), "utf8"),
    )
    const productionAssets = await readFile(
      path.resolve(import.meta.dirname, "production-assets.mjs"),
      "utf8",
    )
    expect(manifest.scripts.deploy).toContain("production-assets.mjs deploy")
    expect(manifest.scripts.deploy).toContain("$BRIDGE_UI_ASSET_RECEIPT")
    expect(manifest.scripts.deploy).toContain("$BRIDGE_UI_RUNTIME_PROFILE_FILE")
    expect(manifest.scripts.deploy).not.toContain("pnpm run deploy:check")
    expect(manifest.scripts["deploy:check"]).toContain("$BRIDGE_UI_ASSET_RECEIPT")
    expect(manifest.scripts.deploy).not.toContain("pnpm run build && wrangler deploy")
    expect(manifest.scripts["deploy:preactivation"]).toBeUndefined()
    expect(manifest.scripts["deploy:preactivation:check"]).toBeUndefined()
    expect(productionAssets).toContain('const modes = ["generate", "verify", "deploy"]')
    expect(productionAssets).not.toContain("verify-preactivation")
    expect(productionAssets).not.toContain("deploy-preactivation")
    expect(productionAssets).not.toContain('deployArgs.push("--dry-run")')
    expect(productionAssets).toContain('"verify-production-checkpoint-ui-live"')
    expect(productionAssets).toContain("BRIDGE_CHECKPOINT_EVIDENCE")
    expect(productionAssets).not.toContain("BRIDGE_RELEASE_BUNDLE")
    expect(productionAssets).not.toContain("BRIDGE_OPERATIONAL_CONFIG_SEAL_RECEIPT")
    expect(productionAssets).not.toContain("BRIDGE_CONTROLLER_SCHEDULE_RECEIPT")
    expect(productionAssets).not.toContain("BRIDGE_CONTROLLER_EXECUTE_RECEIPT")
    expect(productionAssets).toContain("BRIDGE_PRODUCTION_INSTALLER_IDENTITY")
    expect(productionAssets).not.toContain("BRIDGE_RELEASE_INPUTS_MANIFEST")
    expect(productionAssets).toContain("readOrdinaryFile(profileFile)")
    expect(productionAssets).toContain("releaseProfileSchema.parse(JSON.parse(raw))")
    expect(productionAssets).toContain("const manifestSha256 = verifyProductionUiLive(profileFile)")
    expect(productionAssets).toContain('{ cwd: sourceRoot, encoding: "utf8" }')
    expect(productionAssets).toContain("assertProductionUiProfile(releaseProfile, manifestSha256)")
    expect(productionAssets).toContain(
      "await deployFrozenAssets(receipt, raw, releaseProfile, profileFile, identity)",
    )
    expect(productionAssets).toContain("await requireUnchangedSourceIdentity(identity)")
    expect(productionAssets).toContain("walletconnect_project_id: projectId")
    expect(productionAssets).toContain(
      "receipt.walletconnect_project_id?.toLowerCase() !== projectId",
    )
    expect(productionAssets).toContain("VITE_WALLETCONNECT_PROJECT_ID: projectId")
    expect(productionAssets).toContain('"HEAD:ui/wrangler.production.jsonc"')
    expect(productionAssets).toMatch(/"--config",\s*frozenConfig/)
    expect(productionAssets).not.toContain("installRuntimeProfile(frozen, profileFile)")
  })

  it.each(["verify-preactivation", "deploy-preactivation"])(
    "rejects the removed %s production asset mode",
    (mode) => {
      const result = spawnSync(
        process.execPath,
        [path.resolve(import.meta.dirname, "production-assets.mjs"), mode, "receipt.json"],
        { encoding: "utf8" },
      )
      expect(result.status).not.toBe(0)
      expect(result.stderr).toContain(
        "usage: production-assets.mjs {generate|verify|deploy} RECEIPT [UI_RUNTIME_PROFILE]",
      )
    },
  )

  it("keeps the production custom domain out of staging deployments", async () => {
    const productionConfig = await readFile(
      path.resolve(import.meta.dirname, "../wrangler.production.jsonc"),
      "utf8",
    )
    const stagingConfig = await readFile(
      path.resolve(import.meta.dirname, "../wrangler.jsonc"),
      "utf8",
    )
    const productionAssets = await readFile(
      path.resolve(import.meta.dirname, "production-assets.mjs"),
      "utf8",
    )
    const stagingAssets = await readFile(
      path.resolve(import.meta.dirname, "staging-assets.mjs"),
      "utf8",
    )
    expect(productionConfig).toContain("bridge.kinic.xyz")
    expect(stagingConfig).not.toContain("bridge.kinic.xyz")
    expect(productionAssets).toContain("wrangler.production.jsonc")
    expect(stagingAssets).toContain('resolve(uiRoot, "wrangler.jsonc")')
    expect(stagingAssets).not.toContain("wrangler.production.jsonc")
  })
})
