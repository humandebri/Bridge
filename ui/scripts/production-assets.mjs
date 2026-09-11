import { createHash } from "node:crypto"
import { execFileSync, spawn, spawnSync } from "node:child_process"
import {
  chmodSync,
  closeSync,
  constants,
  copyFileSync,
  fstatSync,
  lstatSync,
  mkdirSync,
  mkdtempSync,
  openSync,
  readdirSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs"
import { dirname, relative, resolve, sep } from "node:path"
import { tmpdir } from "node:os"

const uiRoot = resolve(import.meta.dirname, "..")
const sourceRoot = resolve(uiRoot, "..")
const distRoot = resolve(uiRoot, "dist")
const profileBootstrap = "deployment-profile.js"
const productionWranglerConfig = resolve(uiRoot, "wrangler.production.jsonc")

if (process.versions.node !== "24.14.0")
  throw new Error("Production UI artifacts require Node.js 24.14.0")
if (execFileSync("pnpm", ["--version"], { encoding: "utf8" }).trim() !== "11.0.8") {
  throw new Error("Production UI artifacts require pnpm 11.0.8")
}

/** @typedef {{ path: string, sha256: string }} ArtifactFile */
/** @typedef {{ source_revision: string, source_tree_sha256: string }} SourceIdentity */
/** @typedef {{ files: ArtifactFile[], artifact_set_sha256: string }} BuiltAssets */
/** @typedef {{ schema_version: number, source_revision: string, source_tree_sha256: string, walletconnect_project_id: string, artifact_set_sha256: string, files: ArtifactFile[] }} ArtifactReceipt */

/** @param {string | NodeJS.ArrayBufferView} value */
function sha256(value) {
  return createHash("sha256").update(value).digest("hex")
}

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

function hashGitArchive() {
  return new Promise((resolve, reject) => {
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
    child.stderr.on("data", (chunk) => {
      stderr += chunk
    })
    child.stderr.on("error", fail)
    child.on("error", fail)
    child.on("close", (code, signal) => {
      if (settled) return
      if (code !== 0) {
        fail(new Error(`git archive failed (${code ?? signal}): ${stderr.trim()}`))
        return
      }
      settled = true
      resolve(digest.digest("hex"))
    })
  })
}

async function sourceIdentity() {
  const dirty = execFileSync(
    "git",
    [
      "-C",
      sourceRoot,
      "status",
      "--porcelain=v1",
      "--untracked-files=all",
      "--ignore-submodules=none",
    ],
    { encoding: "utf8" },
  )
  if (dirty !== "")
    throw new Error("Production UI artifact generation requires a clean source tree")
  return {
    source_revision: execFileSync("git", ["-C", sourceRoot, "rev-parse", "HEAD"], {
      encoding: "utf8",
    }).trim(),
    source_tree_sha256: await hashGitArchive(),
  }
}

/** @param {string} root @param {string} [current] @returns {ArtifactFile[]} */
function walk(root, current = root) {
  /** @type {ArtifactFile[]} */
  const files = []
  for (const name of readdirSync(current).sort()) {
    const path = resolve(current, name)
    const stat = lstatSync(path)
    if (stat.isSymbolicLink()) throw new Error(`Production UI artifact rejects symlink: ${path}`)
    if (stat.isDirectory()) files.push(...walk(root, path))
    else if (stat.isFile()) {
      const relativePath = relative(root, path).split(sep).join("/")
      if (relativePath !== profileBootstrap)
        files.push({ path: relativePath, sha256: sha256(readFileSync(path)) })
    } else throw new Error(`Production UI artifact rejects non-file: ${path}`)
  }
  return files
}

/** @returns {string} */
function walletConnectProjectId() {
  const value = process.env.VITE_WALLETCONNECT_PROJECT_ID ?? ""
  if (!/^[0-9a-fA-F]{32}$/.test(value)) {
    throw new Error(
      "Production UI artifacts require a 32-character hexadecimal VITE_WALLETCONNECT_PROJECT_ID",
    )
  }
  return value.toLowerCase()
}

/** @param {string} projectId */
function buildGenericAssets(projectId) {
  const result = spawnSync("pnpm", ["run", "build"], {
    cwd: uiRoot,
    env: {
      ...process.env,
      KINIC_GENERIC_PRODUCTION_UI_BUILD: "1",
      VITE_DEPLOYMENT_PROFILE_JSON: "",
      VITE_WALLETCONNECT_PROJECT_ID: projectId,
    },
    stdio: "inherit",
  })
  if (result.status !== 0) throw new Error("Generic production UI build failed")
  const files = walk(distRoot)
  if (files.length === 0) throw new Error("Generic production UI build produced no assets")
  return { files, artifact_set_sha256: sha256(JSON.stringify(files)) }
}

/** @param {ArtifactReceipt} receipt @param {SourceIdentity} identity @param {BuiltAssets} built @param {string} projectId */
function validateReceipt(receipt, identity, built, projectId) {
  const keys = Object.keys(receipt).sort().join(",")
  if (
    keys !==
    "artifact_set_sha256,files,schema_version,source_revision,source_tree_sha256,walletconnect_project_id"
  ) {
    throw new Error("UI artifact receipt has unexpected fields")
  }
  if (
    receipt.schema_version !== 2 ||
    receipt.source_revision !== identity.source_revision ||
    receipt.source_tree_sha256?.toLowerCase() !== identity.source_tree_sha256 ||
    receipt.walletconnect_project_id?.toLowerCase() !== projectId ||
    receipt.artifact_set_sha256?.toLowerCase() !== built.artifact_set_sha256 ||
    JSON.stringify(receipt.files) !== JSON.stringify(built.files)
  ) {
    throw new Error("UI artifact receipt differs from the clean reproducible build")
  }
}

/** @param {string} targetRoot @param {string} raw */
async function installRuntimeProfile(targetRoot, raw) {
  const { deploymentProfileSchema } = await import("../src/config/profile.ts")
  const parsedProfile = deploymentProfileSchema.parse(JSON.parse(raw))
  const publicProfile = {
    ...parsedProfile,
    deploymentBlock: parsedProfile.deploymentBlock?.toString() ?? null,
  }
  const publicRaw = JSON.stringify(publicProfile)
  writeFileSync(
    resolve(targetRoot, profileBootstrap),
    `globalThis.__KINIC_DEPLOYMENT_PROFILE_JSON__ = ${JSON.stringify(publicRaw)};\n`,
    { flag: "wx", mode: 0o400 },
  )
}

/** @param {string} profileFile */
function verifyProductionUiLive(profileFile) {
  const checkpointEvidence = process.env.BRIDGE_CHECKPOINT_EVIDENCE
  const uiRpcConfig = process.env.BRIDGE_UI_RPC_CONFIG
  const productionInstallerIdentity = process.env.BRIDGE_PRODUCTION_INSTALLER_IDENTITY
  if (!checkpointEvidence || !uiRpcConfig || !productionInstallerIdentity) {
    throw new Error(
      "Production UI deploy requires approved checkpoint evidence, reviewed UI RPC configuration, and production installer identity",
    )
  }
  const cargoArgs = [
    "run",
    "--locked",
    "--quiet",
    "--manifest-path",
    resolve(sourceRoot, "Cargo.toml"),
    "-p",
    "bridge-profile",
    "--",
  ]
  const gateOutput = execFileSync(
    "cargo",
    [
      ...cargoArgs,
      "verify-production-checkpoint-ui-live",
      checkpointEvidence,
      uiRpcConfig,
      profileFile,
    ],
    { cwd: sourceRoot, encoding: "utf8" },
  )
  const manifestSha256 =
    /^production_ui=live-pass schema=36 activation=execute manifest_sha256=([0-9a-fA-F]{64})$/m.exec(
      gateOutput,
    )?.[1]
  if (!manifestSha256) {
    throw new Error("Fixed bridge-profile did not authorize the live production UI")
  }
  return manifestSha256
}

/** @param {string} profileFile */
async function validateProductionProfile(profileFile) {
  const rawBuffer = readOrdinaryFile(profileFile)
  const raw = rawBuffer.toString("utf8")
  if (process.env.VITE_DEPLOYMENT_PROFILE_JSON?.trim() !== raw.trim()) {
    throw new Error("VITE_DEPLOYMENT_PROFILE_JSON must be the reviewed UI runtime profile verbatim")
  }
  /** @type {typeof globalThis & { __KINIC_DEPLOYMENT_PROFILE_JSON__?: string }} */
  const deploymentGlobal = globalThis
  deploymentGlobal.__KINIC_DEPLOYMENT_PROFILE_JSON__ = raw.trim()
  const { releaseProfileSchema } = await import("../src/config/profile.ts")
  const releaseProfile = releaseProfileSchema.parse(JSON.parse(raw))
  return { raw, releaseProfile }
}

/** @param {SourceIdentity} expected */
async function requireUnchangedSourceIdentity(expected) {
  const current = await sourceIdentity()
  if (
    current.source_revision !== expected.source_revision ||
    current.source_tree_sha256 !== expected.source_tree_sha256
  ) {
    throw new Error("Production UI checkout changed before publication")
  }
}

/** @param {ArtifactReceipt} receipt @param {string} rawProfile @param {import("zod").output<typeof import("../src/config/profile.ts").releaseProfileSchema>} releaseProfile @param {string} profileFile @param {SourceIdentity} identity */
async function deployFrozenAssets(receipt, rawProfile, releaseProfile, profileFile, identity) {
  const frozenRoot = mkdtempSync(resolve(tmpdir(), "kinic-ui-deploy."))
  const frozen = resolve(frozenRoot, "assets")
  const frozenConfig = resolve(frozenRoot, "wrangler.production.jsonc")
  try {
    mkdirSync(frozen, { mode: 0o700 })
    for (const file of receipt.files) {
      const source = resolve(distRoot, file.path)
      const target = resolve(frozen, file.path)
      mkdirSync(dirname(target), { recursive: true, mode: 0o700 })
      copyFileSync(source, target)
      if (sha256(readFileSync(target)) !== file.sha256.toLowerCase()) {
        throw new Error(`UI artifact changed while freezing: ${file.path}`)
      }
      chmodSync(target, 0o400)
    }
    await installRuntimeProfile(frozen, rawProfile)
    const configBytes = readOrdinaryFile(productionWranglerConfig)
    const reviewedConfig = execFileSync("git", [
      "-C",
      sourceRoot,
      "show",
      "HEAD:ui/wrangler.production.jsonc",
    ])
    if (!configBytes.equals(reviewedConfig)) {
      throw new Error("Production Wrangler config differs from the reviewed HEAD")
    }
    writeFileSync(frozenConfig, configBytes, { flag: "wx", mode: 0o400 })
    for (const path of readdirSync(frozen, { recursive: true })
      .map((entry) => resolve(frozen, String(entry)))
      .sort()
      .reverse()) {
      if (lstatSync(path).isDirectory()) chmodSync(path, 0o500)
    }
    chmodSync(frozen, 0o500)
    await requireUnchangedSourceIdentity(identity)
    const manifestSha256 = verifyProductionUiLive(profileFile)
    if (readOrdinaryFile(profileFile).toString("utf8") !== rawProfile) {
      throw new Error("Production UI runtime profile changed after assets were frozen")
    }
    const { assertProductionUiProfile } = await import("../src/config/deploy-safety.ts")
    assertProductionUiProfile(releaseProfile, manifestSha256)
    const deployArgs = ["exec", "wrangler", "deploy", "--config", frozenConfig, "--assets", frozen]
    const deployed = spawnSync("pnpm", deployArgs, {
      cwd: uiRoot,
      env: process.env,
      stdio: "inherit",
    })
    if (deployed.status !== 0) throw new Error("Production UI deployment failed")
  } finally {
    chmodSync(frozen, 0o700)
    for (const path of readdirSync(frozen, { recursive: true }).map((entry) =>
      resolve(frozen, String(entry)),
    )) {
      if (lstatSync(path).isDirectory()) chmodSync(path, 0o700)
    }
    rmSync(frozenRoot, { recursive: true, force: true })
  }
}

const [, , mode, receiptPath, profileFile] = process.argv
try {
  const modes = ["generate", "verify", "deploy"]
  if (!receiptPath || !modes.includes(mode)) {
    throw new Error(
      "usage: production-assets.mjs {generate|verify|deploy} RECEIPT [UI_RUNTIME_PROFILE]",
    )
  }
  const identity = await sourceIdentity()
  const projectId = walletConnectProjectId()
  const built = buildGenericAssets(projectId)
  if (mode === "generate") {
    writeFileSync(
      receiptPath,
      `${JSON.stringify({
        schema_version: 2,
        ...identity,
        walletconnect_project_id: projectId,
        ...built,
      })}\n`,
      { flag: "wx" },
    )
    process.stdout.write(`ui_artifact_set_sha256=${built.artifact_set_sha256}\n`)
  } else {
    const receipt = JSON.parse(readFileSync(receiptPath, "utf8"))
    validateReceipt(receipt, identity, built, projectId)
    if (mode === "deploy") {
      if (!profileFile) throw new Error("deploy requires the UI runtime profile")
      const { raw, releaseProfile } = await validateProductionProfile(profileFile)
      await deployFrozenAssets(receipt, raw, releaseProfile, profileFile, identity)
    }
    process.stdout.write(`ui_artifact_set_sha256=${built.artifact_set_sha256}\n`)
  }
} catch (error) {
  process.stderr.write(`${error instanceof Error ? error.message : String(error)}\n`)
  process.exitCode = 1
}
