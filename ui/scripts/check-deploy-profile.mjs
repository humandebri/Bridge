import { createHash } from "node:crypto"
import { execFileSync, spawn } from "node:child_process"
import { mkdtempSync, readFileSync, rmSync } from "node:fs"
import { tmpdir } from "node:os"
import { dirname, join, resolve } from "node:path"

/** @param {string} sourceRoot */
function hashGitArchive(sourceRoot) {
  return new Promise((resolvePromise, reject) => {
    const child = spawn("git", ["-C", sourceRoot, "archive", "HEAD"], {
      stdio: ["ignore", "pipe", "pipe"],
    })
    const digest = createHash("sha256")
    let stderr = ""
    let settled = false
    /** @param {Error} error */
    const fail = (error) => {
      if (settled) return
      settled = true
      reject(error)
    }
    child.stdout.on("data", (chunk) => digest.update(chunk))
    child.stdout.on("error", fail)
    child.stderr.setEncoding("utf8")
    child.stderr.on("data", (chunk) => { stderr += chunk })
    child.stderr.on("error", fail)
    child.on("error", fail)
    child.on("close", (code, signal) => {
      if (settled) return
      if (code !== 0) {
        fail(new Error(`git archive failed (${code ?? signal}): ${stderr.trim()}`))
        return
      }
      settled = true
      resolvePromise(digest.digest("hex"))
    })
  })
}

try {
  const profileFile = process.env.BRIDGE_UI_RUNTIME_PROFILE_FILE
  const inputsManifestFile = process.env.BRIDGE_RELEASE_INPUTS_MANIFEST
  const bundle = process.env.BRIDGE_RELEASE_BUNDLE
  if (!profileFile || !inputsManifestFile || !bundle) throw new Error("Production UI deploy requires a signed Gate B bundle and reviewed release input files")
  if (!/^[0-9a-f]{32}$/i.test(process.env.VITE_WALLETCONNECT_PROJECT_ID?.trim() ?? "")) {
    throw new Error("Production UI deploy requires a 32-character hexadecimal VITE_WALLETCONNECT_PROJECT_ID")
  }
  const sourceRoot = resolve(import.meta.dirname, "../..")
  const releaseManifest = JSON.parse(readFileSync(join(bundle, "release-manifest.json"), "utf8"))
  if (!/^[0-9a-f]{40}$/i.test(releaseManifest.source_revision)
    || !/^[0-9a-f]{64}$/i.test(releaseManifest.source_tree_sha256)) {
    throw new Error("Gate B manifest does not bind a valid UI source revision and tree")
  }
  const dirty = execFileSync("git", ["-C", sourceRoot, "status", "--porcelain=v1", "--untracked-files=all", "--ignore-submodules=none"], { encoding: "utf8" })
  if (dirty !== "") throw new Error("Production UI deploy requires the exact clean Gate B source tree")
  const revision = execFileSync("git", ["-C", sourceRoot, "rev-parse", "HEAD"], { encoding: "utf8" }).trim()
  const tree = await hashGitArchive(sourceRoot)
  if (revision !== releaseManifest.source_revision || tree !== releaseManifest.source_tree_sha256.toLowerCase()) {
    throw new Error("Production UI checkout differs from the Gate B source revision or tree")
  }
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
  const releaseProfile = JSON.parse(rawProfile)
  const manifest = JSON.parse(readFileSync(inputsManifestFile, "utf8"))
  const actualHash = createHash("sha256").update(rawProfile).digest("hex")
  if (manifest.artifacts?.["ui-runtime-profile.json"] !== actualHash) {
    throw new Error("Production UI profile hash differs from the reviewed release inputs")
  }
  if (process.env.VITE_DEPLOYMENT_PROFILE_JSON?.trim() !== rawProfile.trim()) {
    throw new Error("VITE_DEPLOYMENT_PROFILE_JSON must be the reviewed UI runtime profile verbatim")
  }
  const { assertProductionUiProfile } = await import("../src/config/deploy-safety.ts")
  assertProductionUiProfile(releaseProfile, verifiedManifestSha256)
  process.stdout.write(`Production UI profile accepted: ${releaseProfile.environment}\n`)
} catch (error) {
  process.stderr.write(`${error instanceof Error ? error.message : String(error)}\n`)
  process.exitCode = 1
}
