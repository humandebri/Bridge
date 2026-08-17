// @vitest-environment node
import { createHash } from "node:crypto"
import { chmodSync, mkdtempSync, mkdirSync, rmSync, writeFileSync } from "node:fs"
import { tmpdir } from "node:os"
import { join, resolve } from "node:path"
import { spawnSync } from "node:child_process"
import { afterEach, beforeEach, describe, expect, it } from "vitest"

let root

beforeEach(() => {
  root = mkdtempSync(join(tmpdir(), "bridge-ui-deploy-check."))
})
afterEach(() => rmSync(root, { recursive: true, force: true }))

function fixture(profileOverrides = {}) {
  const inputs = join(root, "inputs")
  const bundle = join(root, "bundle")
  const bin = join(root, "bin")
  mkdirSync(inputs); mkdirSync(bundle); mkdirSync(bin)
  const gate = "a".repeat(64)
  const revision = "b".repeat(40)
  const archive = "x".repeat(2 * 1024 * 1024)
  const tree = createHash("sha256").update(archive).digest("hex")
  const profile = JSON.stringify({
    environment: "mainnet-candidate", label: "Base", testOnly: false,
    environmentMode: null, activationTimelockDelaySeconds: 86_400, gateBManifestSha256: gate,
    profileFileSha256: "1".repeat(64), profileCanonicalSha256: "2".repeat(64),
    icHost: "https://icp-api.io", baseRpcUrl: "https://rpc.example", chainId: 8453,
    bridgeCanisterId: "aaaaa-aa", ledgerCanisterId: "aaaaa-aa", indexCanisterId: "aaaaa-aa",
    deploymentInstanceId: `0x${"99".repeat(32)}`,
    minimumWithdrawalId: `0x${"00".repeat(31)}01`,
    icToken: { name: "KINIC", symbol: "KINIC", decimals: 8 }, baseToken: { symbol: "KINIC", decimals: 8 },
    bridgeAddress: `0x${"11".repeat(20)}`, bsnsAddress: `0x${"22".repeat(20)}`,
    timelockAddress: `0x${"77".repeat(20)}`,
    expected_bridge_signer: `0x${"33".repeat(20)}`, evmRpcCanisterId: "7hfb6-caaaa-aaaar-qadga-cai",
    rpcProviderUrlsSha256: `0x${"44".repeat(32)}`, deploymentBlock: "1",
    bridgeRuntimeHash: `0x${"55".repeat(32)}`, bsnsRuntimeHash: `0x${"66".repeat(32)}`,
    ...profileOverrides,
  }) + "\n"
  for (const name of ["canister-init.json", "contract-constructor-args.json"]) writeFileSync(join(inputs, name), "{}\n")
  writeFileSync(join(inputs, "ui-runtime-profile.json"), profile)
  const uiHash = createHash("sha256").update(profile).digest("hex")
  writeFileSync(join(inputs, "release-inputs-manifest.json"), JSON.stringify({ artifacts: { "ui-runtime-profile.json": uiHash } }) + "\n")
  writeFileSync(join(bundle, "release-manifest.json"), JSON.stringify({ source_revision: revision, source_tree_sha256: tree }) + "\n")
  const cargo = join(bin, "cargo")
  writeFileSync(cargo, `#!/usr/bin/env node
const fs=require('node:fs'); const a=process.argv.slice(2);
if(a.includes('verify-live')) { if(process.env.FAKE_VERIFY_FAIL) process.exit(1); console.log('gate_b=pass manifest_sha256=${gate}'); }
else if(a.includes('render-bundle-inputs')) fs.cpSync(process.env.FAKE_INPUTS,a.at(-1),{recursive:true});
else process.exit(2);
`)
  chmodSync(cargo, 0o755)
  const git = join(bin, "git")
  writeFileSync(git, `#!/usr/bin/env node
const a=process.argv.slice(2);
if(a.includes('status')) process.stdout.write(process.env.FAKE_GIT_DIRTY ? ' M ui/src/config.ts\\n' : '');
else if(a.includes('rev-parse')) console.log('${revision}');
else if(a.includes('archive')) process.stdout.write('${archive}');
else process.exit(2);
`)
  chmodSync(git, 0o755)
  return { bundle, inputs, bin, gate, profile }
}

function run(env) {
  return spawnSync(process.execPath, [resolve(import.meta.dirname, "check-deploy-profile.mjs")], {
    encoding: "utf8", env: { ...process.env, ...env },
  })
}

const walletConnectProjectId = "0123456789abcdef0123456789abcdef"

describe("production UI Gate B binding", () => {
  it("rejects an arbitrary manifest environment value without a signed bundle", () => {
    const result = run({ BRIDGE_GATE_B_MANIFEST_SHA256: "f".repeat(64) })
    expect(result.status).not.toBe(0)
    expect(result.stderr).toContain("requires a signed Gate B bundle")
  })

  it("rejects when the fixed bridge-profile verifier fails", () => {
    const f = fixture()
    const result = run({
      PATH: `${f.bin}:${process.env.PATH}`, FAKE_VERIFY_FAIL: "1", FAKE_INPUTS: f.inputs,
      BRIDGE_RELEASE_BUNDLE: f.bundle, BRIDGE_UI_RUNTIME_PROFILE_FILE: join(f.inputs, "ui-runtime-profile.json"),
      BRIDGE_RELEASE_INPUTS_MANIFEST: join(f.inputs, "release-inputs-manifest.json"),
      VITE_DEPLOYMENT_PROFILE_JSON: f.profile, BRIDGE_GATE_B_MANIFEST_SHA256: "f".repeat(64),
      VITE_WALLETCONNECT_PROJECT_ID: walletConnectProjectId,
    })
    expect(result.status).not.toBe(0)
  })

  it("rejects a production deploy without a WalletConnect project ID", () => {
    const f = fixture()
    const result = run({
      PATH: `${f.bin}:${process.env.PATH}`, FAKE_INPUTS: f.inputs,
      BRIDGE_RELEASE_BUNDLE: f.bundle, BRIDGE_UI_RUNTIME_PROFILE_FILE: join(f.inputs, "ui-runtime-profile.json"),
      BRIDGE_RELEASE_INPUTS_MANIFEST: join(f.inputs, "release-inputs-manifest.json"),
      VITE_DEPLOYMENT_PROFILE_JSON: f.profile, VITE_WALLETCONNECT_PROJECT_ID: "",
    })
    expect(result.status).not.toBe(0)
    expect(result.stderr).toContain("requires a 32-character hexadecimal VITE_WALLETCONNECT_PROJECT_ID")
  })

  it("derives approval from the verified bundle rather than the environment value", () => {
    const f = fixture()
    const result = run({
      PATH: `${f.bin}:${process.env.PATH}`, FAKE_INPUTS: f.inputs,
      BRIDGE_RELEASE_BUNDLE: f.bundle, BRIDGE_UI_RUNTIME_PROFILE_FILE: join(f.inputs, "ui-runtime-profile.json"),
      BRIDGE_RELEASE_INPUTS_MANIFEST: join(f.inputs, "release-inputs-manifest.json"),
      VITE_DEPLOYMENT_PROFILE_JSON: f.profile, BRIDGE_GATE_B_MANIFEST_SHA256: "f".repeat(64),
      VITE_WALLETCONNECT_PROJECT_ID: walletConnectProjectId,
    })
    expect(result.status, result.stderr).toBe(0)
  })

  it("rejects a dirty UI checkout before build or deploy", () => {
    const f = fixture()
    const result = run({
      PATH: `${f.bin}:${process.env.PATH}`, FAKE_INPUTS: f.inputs, FAKE_GIT_DIRTY: "1",
      BRIDGE_RELEASE_BUNDLE: f.bundle, BRIDGE_UI_RUNTIME_PROFILE_FILE: join(f.inputs, "ui-runtime-profile.json"),
      BRIDGE_RELEASE_INPUTS_MANIFEST: join(f.inputs, "release-inputs-manifest.json"),
      VITE_DEPLOYMENT_PROFILE_JSON: f.profile, VITE_WALLETCONNECT_PROJECT_ID: walletConnectProjectId,
    })
    expect(result.status).not.toBe(0)
    expect(result.stderr).toContain("exact clean Gate B source tree")
  })

  it.each([
    [{ activationTimelockDelaySeconds: null }, "at least 24 hours"],
    [{ activationTimelockDelaySeconds: 300 }, "at least 24 hours"],
    [{ environmentMode: "short-delay-test-only" }, "environment modes"],
  ])("rejects an unsafe production Timelock profile", (overrides, message) => {
    const f = fixture(overrides)
    const result = run({
      PATH: `${f.bin}:${process.env.PATH}`, FAKE_INPUTS: f.inputs,
      BRIDGE_RELEASE_BUNDLE: f.bundle, BRIDGE_UI_RUNTIME_PROFILE_FILE: join(f.inputs, "ui-runtime-profile.json"),
      BRIDGE_RELEASE_INPUTS_MANIFEST: join(f.inputs, "release-inputs-manifest.json"),
      VITE_DEPLOYMENT_PROFILE_JSON: f.profile,
      VITE_WALLETCONNECT_PROJECT_ID: walletConnectProjectId,
    })
    expect(result.status).not.toBe(0)
    expect(result.stderr).toContain(message)
  })
})
