import { createHash } from "node:crypto"
import { execFileSync } from "node:child_process"
import { mkdtempSync, readFileSync, rmSync } from "node:fs"
import { tmpdir } from "node:os"
import { dirname, join, resolve } from "node:path"

try {
  const profileFile = process.env.BRIDGE_UI_RUNTIME_PROFILE_FILE
  const inputsManifestFile = process.env.BRIDGE_RELEASE_INPUTS_MANIFEST
  const bundle = process.env.BRIDGE_RELEASE_BUNDLE
  if (!profileFile || !inputsManifestFile || !bundle) throw new Error("Production UI deploy requires a signed Gate B bundle and reviewed release input files")
  if (!/^[0-9a-f]{32}$/i.test(process.env.VITE_WALLETCONNECT_PROJECT_ID?.trim() ?? "")) {
    throw new Error("Production UI deploy requires a 32-character hexadecimal VITE_WALLETCONNECT_PROJECT_ID")
  }
  const sourceRoot = resolve(import.meta.dirname, "../..")
  const cargoArgs = ["run", "--locked", "--quiet", "--manifest-path", join(sourceRoot, "Cargo.toml"), "-p", "bridge-profile", "--"]
  const gateOutput = execFileSync("cargo", [...cargoArgs, "verify-live", bundle], { encoding: "utf8" })
  const verifiedManifestSha256 = /manifest_sha256=([0-9a-fA-F]{64})/.exec(gateOutput)?.[1]
  if (!verifiedManifestSha256) throw new Error("Fixed bridge-profile did not verify the Gate B manifest")
  const rendered = mkdtempSync(join(tmpdir(), "bridge-ui-release-inputs."))
  try {
    execFileSync("cargo", [...cargoArgs, "render-bundle-inputs", bundle, rendered], { stdio: "pipe" })
    const reviewedRoot = dirname(inputsManifestFile)
    for (const name of ["canister-init.json", "contract-constructor-args.json", "ui-runtime-profile.json", "release-inputs-manifest.json"]) {
      if (!readFileSync(join(rendered, name)).equals(readFileSync(join(reviewedRoot, name)))) {
        throw new Error(`Production release input drift: ${name}`)
      }
    }
  } finally {
    rmSync(rendered, { recursive: true, force: true })
  }
  const rawProfile = readFileSync(profileFile, "utf8")
  const manifest = JSON.parse(readFileSync(inputsManifestFile, "utf8"))
  const actualHash = createHash("sha256").update(rawProfile).digest("hex")
  if (manifest.artifacts?.["ui-runtime-profile.json"] !== actualHash) {
    throw new Error("Production UI profile hash differs from the reviewed release inputs")
  }
  if (process.env.VITE_DEPLOYMENT_PROFILE_JSON?.trim() !== rawProfile.trim()) {
    throw new Error("VITE_DEPLOYMENT_PROFILE_JSON must be the reviewed UI runtime profile verbatim")
  }
  globalThis.__KINIC_DEPLOYMENT_PROFILE_JSON__ = process.env.VITE_DEPLOYMENT_PROFILE_JSON
  const [{ deploymentProfile }, { assertProductionUiProfile }] = await Promise.all([
    import("../src/config/profile.ts"),
    import("../src/config/deploy-safety.ts"),
  ])
  assertProductionUiProfile(deploymentProfile, verifiedManifestSha256)
  process.stdout.write(`Production UI profile accepted: ${deploymentProfile.environment}\n`)
} catch (error) {
  process.stderr.write(`${error instanceof Error ? error.message : String(error)}\n`)
  process.exitCode = 1
}
