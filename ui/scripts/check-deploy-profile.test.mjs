// @vitest-environment node
import { chmodSync, mkdtempSync, mkdirSync, rmSync, writeFileSync } from "node:fs"
import { tmpdir } from "node:os"
import { join, resolve } from "node:path"
import { spawnSync } from "node:child_process"
import { afterEach, beforeEach, describe, expect, it } from "vitest"

/** @type {string} */
let root

beforeEach(() => {
  root = mkdtempSync(join(tmpdir(), "bridge-ui-deploy-check."))
})
afterEach(() => rmSync(root, { recursive: true, force: true }))

/** @param {Record<string, unknown>} [profileOverrides] */
function fixture(profileOverrides = {}) {
  const inputs = join(root, "inputs")
  const bundle = join(root, "bundle")
  const bin = join(root, "bin")
  mkdirSync(inputs)
  mkdirSync(bundle)
  mkdirSync(bin)
  const gate = "a".repeat(64)
  const profile =
    JSON.stringify({
      environment: "mainnet-candidate",
      label: "Base",
      testOnly: false,
      environmentMode: null,
      activationTimelockDelaySeconds: 86_400,
      gateBManifestSha256: gate,
      profileFileSha256: "1".repeat(64),
      profileCanonicalSha256: "2".repeat(64),
      canisterSchemaVersion: 36,
      canisterModuleSha256: "3".repeat(64),
      postActivationUpgradeSha256: "4".repeat(64),
      uiRpcConfigSha256: "5".repeat(64),
      icHost: "https://icp-api.io",
      baseRpcUrl: "https://rpc.example",
      chainId: 8453,
      bridgeCanisterId: "aaaaa-aa",
      ledgerCanisterId: "aaaaa-aa",
      indexCanisterId: "aaaaa-aa",
      deploymentInstanceId: `0x${"99".repeat(32)}`,
      minimumWithdrawalId: `0x${"00".repeat(31)}01`,
      icToken: { name: "KINIC", symbol: "KINIC", decimals: 8 },
      baseToken: { symbol: "KINIC", decimals: 8 },
      bridgeAddress: `0x${"11".repeat(20)}`,
      bsnsAddress: `0x${"22".repeat(20)}`,
      timelockAddress: `0x${"77".repeat(20)}`,
      expected_bridge_signer: `0x${"33".repeat(20)}`,
      evmRpcCanisterId: "7hfb6-caaaa-aaaar-qadga-cai",
      rpcProviderUrlsSha256: `0x${"44".repeat(32)}`,
      deploymentBlock: "1",
      bridgeRuntimeHash: `0x${"55".repeat(32)}`,
      bsnsRuntimeHash: `0x${"66".repeat(32)}`,
      ...profileOverrides,
    }) + "\n"
  const paths = {
    profile: join(inputs, "ui-runtime-profile.json"),
    asset: join(inputs, "ui-assets.json"),
    seal: join(inputs, "seal.json"),
    schedule: join(inputs, "schedule.json"),
    execute: join(inputs, "execute.json"),
    upgrade: join(inputs, "post-activation-upgrade.json"),
    rpc: join(inputs, "ui-rpc.json"),
  }
  writeFileSync(paths.profile, profile)
  for (const path of Object.values(paths).slice(1)) writeFileSync(path, "{}\n")
  const cargo = join(bin, "cargo")
  writeFileSync(
    cargo,
    `#!/usr/bin/env node
const a=process.argv.slice(2); const i=a.indexOf('verify-production-checkpoint-ui-live');
if(process.cwd()!==process.env.EXPECTED_CARGO_CWD || i<0 || a[i+1]!==process.env.BRIDGE_CHECKPOINT_EVIDENCE || a[i+2]!==process.env.BRIDGE_UI_RPC_CONFIG || a[i+3]!==process.env.BRIDGE_UI_RUNTIME_PROFILE_FILE || process.env.FAKE_VERIFY_FAIL) process.exit(1);
console.log('production_ui=live-pass schema='+ (process.env.FAKE_VERIFY_SCHEMA ?? '36') +' activation=execute manifest_sha256=${gate}');
`,
  )
  chmodSync(cargo, 0o755)
  return { bundle, bin, paths, profile }
}

/** @param {NodeJS.ProcessEnv} env */
function run(env) {
  return spawnSync(process.execPath, [resolve(import.meta.dirname, "check-deploy-profile.mjs")], {
    encoding: "utf8",
    env: { ...process.env, ...env },
  })
}

const walletConnectProjectId = "0123456789abcdef0123456789abcdef"

/** @param {ReturnType<typeof fixture>} f @param {NodeJS.ProcessEnv} [overrides] */
function validEnv(f, overrides = {}) {
  return {
    PATH: `${f.bin}:${process.env.PATH}`,
    EXPECTED_CARGO_CWD: resolve(import.meta.dirname, "../.."),
    BRIDGE_RELEASE_BUNDLE: f.bundle,
    BRIDGE_UI_RUNTIME_PROFILE_FILE: f.paths.profile,
    BRIDGE_UI_ASSET_RECEIPT: f.paths.asset,
    BRIDGE_OPERATIONAL_CONFIG_SEAL_RECEIPT: f.paths.seal,
    BRIDGE_CONTROLLER_SCHEDULE_RECEIPT: f.paths.schedule,
    BRIDGE_CONTROLLER_EXECUTE_RECEIPT: f.paths.execute,
    BRIDGE_CHECKPOINT_EVIDENCE: f.paths.upgrade,
    BRIDGE_UI_RPC_CONFIG: f.paths.rpc,
    BRIDGE_PRODUCTION_INSTALLER_IDENTITY: "production-installer",
    VITE_DEPLOYMENT_PROFILE_JSON: f.profile,
    VITE_WALLETCONNECT_PROJECT_ID: walletConnectProjectId,
    ...overrides,
  }
}

describe("production UI live binding", () => {
  it("rejects the old live v35 gate and missing reviewed RPC configuration", () => {
    const f = fixture()
    expect(run(validEnv(f, { FAKE_VERIFY_SCHEMA: "35" })).status).not.toBe(0)
    expect(run(validEnv(f, { BRIDGE_UI_RPC_CONFIG: "" })).status).not.toBe(0)
  })

  it("rejects an arbitrary environment value without the required evidence", () => {
    const result = run({ BRIDGE_GATE_B_MANIFEST_SHA256: "f".repeat(64) })
    expect(result.status).not.toBe(0)
    expect(result.stderr).toContain("requires the UI asset receipt")
  })

  it("rejects when the fixed bridge-profile verifier fails", () => {
    const result = run(validEnv(fixture(), { FAKE_VERIFY_FAIL: "1" }))
    expect(result.status).not.toBe(0)
  })

  it("rejects a production deploy without a WalletConnect project ID", () => {
    const result = run(validEnv(fixture(), { VITE_WALLETCONNECT_PROJECT_ID: "" }))
    expect(result.status).not.toBe(0)
    expect(result.stderr).toContain(
      "requires a 32-character hexadecimal VITE_WALLETCONNECT_PROJECT_ID",
    )
  })

  it("derives approval from the live verifier rather than an environment hash", () => {
    const result = run(validEnv(fixture(), { BRIDGE_GATE_B_MANIFEST_SHA256: "f".repeat(64) }))
    expect(result.status, result.stderr).toBe(0)
  })

  it("requires the standalone UI asset receipt", () => {
    const result = run(validEnv(fixture(), { BRIDGE_UI_ASSET_RECEIPT: "" }))
    expect(result.status).not.toBe(0)
    expect(result.stderr).toContain("requires the UI asset receipt")
  })

  it("requires the production installer identity for the controller-only storage check", () => {
    const result = run(validEnv(fixture(), { BRIDGE_PRODUCTION_INSTALLER_IDENTITY: "" }))
    expect(result.status).not.toBe(0)
    expect(result.stderr).toContain("production installer identity")
  })

  it("requires approved checkpoint evidence", () => {
    const result = run(validEnv(fixture(), { BRIDGE_CHECKPOINT_EVIDENCE: "" }))
    expect(result.status).not.toBe(0)
    expect(result.stderr).toContain("approved checkpoint evidence")
  })

  it("does not request archived Gate B and activation files", () => {
    const f = fixture()
    rmSync(f.bundle, { recursive: true, force: true })
    for (const path of [f.paths.seal, f.paths.schedule, f.paths.execute]) rmSync(path)
    const result = run(
      validEnv(f, {
        BRIDGE_RELEASE_BUNDLE: "",
        BRIDGE_OPERATIONAL_CONFIG_SEAL_RECEIPT: "",
        BRIDGE_CONTROLLER_SCHEDULE_RECEIPT: "",
        BRIDGE_CONTROLLER_EXECUTE_RECEIPT: "",
      }),
    )
    expect(result.status, result.stderr).toBe(0)
  })

  it("rejects a runtime profile that the live verifier does not authorize", () => {
    const f = fixture()
    const alternate = join(root, "alternate-profile.json")
    const drifted = JSON.stringify({
      ...JSON.parse(f.profile),
      bridgeAddress: `0x${"88".repeat(20)}`,
    })
    writeFileSync(alternate, `${drifted}\n`)
    const result = run(
      validEnv(f, {
        BRIDGE_UI_RUNTIME_PROFILE_FILE: alternate,
        VITE_DEPLOYMENT_PROFILE_JSON: drifted,
        FAKE_VERIFY_FAIL: "1",
      }),
    )
    expect(result.status).not.toBe(0)
  })

  it.each([
    [{ activationTimelockDelaySeconds: null }, "at least 24 hours"],
    [{ activationTimelockDelaySeconds: 300 }, "at least 24 hours"],
    [{ environmentMode: "short-delay-test-only" }, "environment modes"],
    [{ deploymentBlock: "0" }, "positive deployment block"],
    [{ deploymentBlock: null }, "positive deployment block"],
  ])("rejects an unsafe production profile", (overrides, message) => {
    const result = run(validEnv(fixture(overrides)))
    expect(result.status).not.toBe(0)
    expect(result.stderr).toContain(message)
  })
})
