import { execFileSync } from "node:child_process"
import { closeSync, constants, fstatSync, openSync, readFileSync } from "node:fs"
import { join, resolve } from "node:path"

/** @param {string} path */
function readOrdinaryFile(path) {
  const fd = openSync(path, constants.O_RDONLY | constants.O_NOFOLLOW)
  try {
    if (!fstatSync(fd).isFile()) throw new Error(`Expected an ordinary file: ${path}`)
    return readFileSync(fd)
  } finally {
    closeSync(fd)
  }
}

try {
  const profileFile = process.env.BRIDGE_UI_RUNTIME_PROFILE_FILE
  const bundle = process.env.BRIDGE_RELEASE_BUNDLE
  const sealReceipt = process.env.BRIDGE_OPERATIONAL_CONFIG_SEAL_RECEIPT
  const scheduleReceipt = process.env.BRIDGE_CONTROLLER_SCHEDULE_RECEIPT
  const executeReceipt = process.env.BRIDGE_CONTROLLER_EXECUTE_RECEIPT
  const postActivationUpgradeEvidence = process.env.BRIDGE_POST_ACTIVATION_UPGRADE_EVIDENCE
  const assetReceipt = process.env.BRIDGE_UI_ASSET_RECEIPT
  const productionInstallerIdentity = process.env.BRIDGE_PRODUCTION_INSTALLER_IDENTITY
  if (
    !profileFile ||
    !bundle ||
    !sealReceipt ||
    !scheduleReceipt ||
    !executeReceipt ||
    !postActivationUpgradeEvidence ||
    !assetReceipt ||
    !productionInstallerIdentity
  )
    throw new Error(
      "Production UI deploy requires the UI asset receipt, historical Gate B, activation receipts, post-activation upgrade evidence, runtime profile, and production installer identity",
    )
  if (!/^[0-9a-f]{32}$/i.test(process.env.VITE_WALLETCONNECT_PROJECT_ID?.trim() ?? "")) {
    throw new Error(
      "Production UI deploy requires a 32-character hexadecimal VITE_WALLETCONNECT_PROJECT_ID",
    )
  }
  const sourceRoot = resolve(import.meta.dirname, "../..")
  readOrdinaryFile(assetReceipt)
  const cargoArgs = [
    "run",
    "--locked",
    "--quiet",
    "--manifest-path",
    join(sourceRoot, "Cargo.toml"),
    "-p",
    "bridge-profile",
    "--",
  ]
  const gateOutput = execFileSync(
    "cargo",
    [
      ...cargoArgs,
      "verify-production-ui-live",
      bundle,
      sealReceipt,
      scheduleReceipt,
      executeReceipt,
      postActivationUpgradeEvidence,
      profileFile,
    ],
    { cwd: sourceRoot, encoding: "utf8" },
  )
  const verifiedManifestSha256 =
    /^production_ui=live-pass schema=35 activation=execute manifest_sha256=([0-9a-fA-F]{64})$/m.exec(
      gateOutput,
    )?.[1]
  if (!verifiedManifestSha256)
    throw new Error("Fixed bridge-profile did not authorize the live production UI")
  const rawProfileBuffer = readOrdinaryFile(profileFile)
  const rawProfile = rawProfileBuffer.toString("utf8")
  const { releaseProfileSchema } = await import("../src/config/profile.ts")
  const releaseProfile = releaseProfileSchema.parse(JSON.parse(rawProfile))
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
