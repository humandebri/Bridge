import { createHash } from "node:crypto"
import { execFileSync, spawn, spawnSync } from "node:child_process"
import {
  chmodSync,
  copyFileSync,
  lstatSync,
  mkdirSync,
  mkdtempSync,
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

if (process.versions.node !== "24.14.0")
  throw new Error("Production UI artifacts require Node.js 24.14.0")
if (execFileSync("pnpm", ["--version"], { encoding: "utf8" }).trim() !== "11.0.8") {
  throw new Error("Production UI artifacts require pnpm 11.0.8")
}

/** @typedef {{ path: string, sha256: string }} ArtifactFile */
/** @typedef {{ source_revision: string, source_tree_sha256: string }} SourceIdentity */
/** @typedef {{ files: ArtifactFile[], artifact_set_sha256: string }} BuiltAssets */
/** @typedef {{ schema_version: number, source_revision: string, source_tree_sha256: string, artifact_set_sha256: string, files: ArtifactFile[] }} ArtifactReceipt */

/** @param {string | NodeJS.ArrayBufferView} value */
function sha256(value) {
  return createHash("sha256").update(value).digest("hex")
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

function buildGenericAssets() {
  const result = spawnSync("pnpm", ["run", "build"], {
    cwd: uiRoot,
    env: {
      ...process.env,
      KINIC_GENERIC_PRODUCTION_UI_BUILD: "1",
      VITE_DEPLOYMENT_PROFILE_JSON: "",
    },
    stdio: "inherit",
  })
  if (result.status !== 0) throw new Error("Generic production UI build failed")
  const files = walk(distRoot)
  if (files.length === 0) throw new Error("Generic production UI build produced no assets")
  return { files, artifact_set_sha256: sha256(JSON.stringify(files)) }
}

/** @param {ArtifactReceipt} receipt @param {SourceIdentity} identity @param {BuiltAssets} built */
function validateReceipt(receipt, identity, built) {
  const keys = Object.keys(receipt).sort().join(",")
  if (keys !== "artifact_set_sha256,files,schema_version,source_revision,source_tree_sha256") {
    throw new Error("UI artifact receipt has unexpected fields")
  }
  if (
    receipt.schema_version !== 1 ||
    receipt.source_revision !== identity.source_revision ||
    receipt.source_tree_sha256?.toLowerCase() !== identity.source_tree_sha256 ||
    receipt.artifact_set_sha256?.toLowerCase() !== built.artifact_set_sha256 ||
    JSON.stringify(receipt.files) !== JSON.stringify(built.files)
  ) {
    throw new Error("UI artifact receipt differs from the clean reproducible build")
  }
}

/** @param {string} targetRoot @param {string} profileFile */
function installRuntimeProfile(targetRoot, profileFile) {
  const raw = readFileSync(profileFile, "utf8")
  JSON.parse(raw)
  writeFileSync(
    resolve(targetRoot, profileBootstrap),
    `globalThis.__KINIC_DEPLOYMENT_PROFILE_JSON__ = ${JSON.stringify(raw.trim())};\n`,
    { flag: "wx", mode: 0o400 },
  )
}

/** @param {ArtifactReceipt} receipt @param {string} profileFile */
function deployFrozenAssets(receipt, profileFile) {
  const frozen = mkdtempSync(resolve(tmpdir(), "kinic-ui-deploy."))
  try {
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
    installRuntimeProfile(frozen, profileFile)
    for (const path of readdirSync(frozen, { recursive: true })
      .map((entry) => resolve(frozen, String(entry)))
      .sort()
      .reverse()) {
      if (lstatSync(path).isDirectory()) chmodSync(path, 0o500)
    }
    chmodSync(frozen, 0o500)
    const deployed = spawnSync(
      "pnpm",
      [
        "exec",
        "wrangler",
        "deploy",
        "--config",
        resolve(uiRoot, "wrangler.jsonc"),
        "--assets",
        frozen,
      ],
      {
        cwd: uiRoot,
        env: process.env,
        stdio: "inherit",
      },
    )
    if (deployed.status !== 0) throw new Error("Production UI deployment failed")
  } finally {
    chmodSync(frozen, 0o700)
    for (const path of readdirSync(frozen, { recursive: true }).map((entry) =>
      resolve(frozen, String(entry)),
    )) {
      if (lstatSync(path).isDirectory()) chmodSync(path, 0o700)
    }
    rmSync(frozen, { recursive: true, force: true })
  }
}

const [, , mode, receiptPath, profileFile] = process.argv
try {
  if (!receiptPath || !["generate", "verify", "deploy"].includes(mode)) {
    throw new Error(
      "usage: production-assets.mjs {generate|verify|deploy} RECEIPT [UI_RUNTIME_PROFILE]",
    )
  }
  const identity = await sourceIdentity()
  const built = buildGenericAssets()
  if (mode === "generate") {
    writeFileSync(
      receiptPath,
      `${JSON.stringify({ schema_version: 1, ...identity, ...built })}\n`,
      { flag: "wx" },
    )
    process.stdout.write(`ui_artifact_set_sha256=${built.artifact_set_sha256}\n`)
  } else {
    const receipt = JSON.parse(readFileSync(receiptPath, "utf8"))
    validateReceipt(receipt, identity, built)
    if (mode === "deploy") {
      if (!profileFile) throw new Error("deploy requires the verified UI runtime profile")
      deployFrozenAssets(receipt, profileFile)
    }
    process.stdout.write(`ui_artifact_set_sha256=${built.artifact_set_sha256}\n`)
  }
} catch (error) {
  process.stderr.write(`${error instanceof Error ? error.message : String(error)}\n`)
  process.exitCode = 1
}
