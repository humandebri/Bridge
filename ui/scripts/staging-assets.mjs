import { createHash } from "node:crypto"
import { execFileSync, spawn, spawnSync } from "node:child_process"
import { chmodSync, copyFileSync, lstatSync, mkdirSync, mkdtempSync, readdirSync, readFileSync, rmSync, writeFileSync } from "node:fs"
import { dirname, relative, resolve, sep } from "node:path"
import { tmpdir } from "node:os"

const uiRoot = resolve(import.meta.dirname, "..")
const sourceRoot = resolve(uiRoot, "..")
const distRoot = resolve(uiRoot, "dist")
const profileFile = resolve(sourceRoot, "deployments/sepolia-staging/frontend-profile.json")
const workerName = "kinic-bridge-ui-test"

function sha256(value) {
  return createHash("sha256").update(value).digest("hex")
}

function hashGitArchive() {
  return new Promise((resolveHash, reject) => {
    const child = spawn("git", ["-C", sourceRoot, "archive", "HEAD"], { stdio: ["ignore", "pipe", "pipe"] })
    const digest = createHash("sha256")
    let stderr = ""
    child.stdout.on("data", (chunk) => digest.update(chunk))
    child.stderr.setEncoding("utf8")
    child.stderr.on("data", (chunk) => { stderr += chunk })
    child.on("error", reject)
    child.on("close", (code) => {
      if (code !== 0) reject(new Error(`git archive failed (${code}): ${stderr.trim()}`))
      else resolveHash(digest.digest("hex"))
    })
  })
}

async function sourceIdentity() {
  const dirty = execFileSync("git", ["-C", sourceRoot, "status", "--porcelain=v1", "--untracked-files=all", "--ignore-submodules=none"], { encoding: "utf8" })
  if (dirty !== "") throw new Error("Staging UI artifacts require a clean source tree")
  return {
    source_revision: execFileSync("git", ["-C", sourceRoot, "rev-parse", "HEAD"], { encoding: "utf8" }).trim(),
    source_tree_sha256: await hashGitArchive(),
  }
}

function walk(root, current = root) {
  const files = []
  for (const name of readdirSync(current).sort()) {
    const path = resolve(current, name)
    const stat = lstatSync(path)
    if (stat.isSymbolicLink()) throw new Error(`Staging UI artifact rejects symlink: ${path}`)
    if (stat.isDirectory()) files.push(...walk(root, path))
    else if (stat.isFile()) files.push({ path: relative(root, path).split(sep).join("/"), sha256: sha256(readFileSync(path)) })
    else throw new Error(`Staging UI artifact rejects non-file: ${path}`)
  }
  return files
}

function artifactSet() {
  const files = walk(distRoot)
  if (files.length === 0) throw new Error("Staging UI build produced no assets")
  return { files, artifact_set_sha256: sha256(JSON.stringify(files)) }
}

function validateReceipt(receipt, identity, built) {
  const keys = Object.keys(receipt).sort().join(",")
  if (keys !== "artifact_set_sha256,files,profile_sha256,schema_version,source_revision,source_tree_sha256,worker_name") {
    throw new Error("Staging UI artifact receipt has unexpected fields")
  }
  if (receipt.schema_version !== 1
    || receipt.worker_name !== workerName
    || receipt.source_revision !== identity.source_revision
    || receipt.source_tree_sha256 !== identity.source_tree_sha256
    || receipt.profile_sha256 !== sha256(readFileSync(profileFile))
    || receipt.artifact_set_sha256 !== built.artifact_set_sha256
    || JSON.stringify(receipt.files) !== JSON.stringify(built.files)) {
    throw new Error("Staging UI artifact receipt differs from the clean build")
  }
}

function deployFrozen(receipt) {
  const frozen = mkdtempSync(resolve(tmpdir(), "kinic-staging-ui."))
  try {
    for (const file of receipt.files) {
      const source = resolve(distRoot, file.path)
      const target = resolve(frozen, file.path)
      mkdirSync(dirname(target), { recursive: true, mode: 0o700 })
      copyFileSync(source, target)
      if (sha256(readFileSync(target)) !== file.sha256) throw new Error(`Staging UI artifact changed: ${file.path}`)
      chmodSync(target, 0o400)
    }
    const deployed = spawnSync("pnpm", ["exec", "wrangler", "deploy", "--config", resolve(uiRoot, "wrangler.jsonc"), "--name", workerName, "--assets", frozen], {
      cwd: uiRoot,
      env: process.env,
      stdio: "inherit",
    })
    if (deployed.status !== 0) throw new Error("Staging UI deployment failed")
  } finally {
    chmodSync(frozen, 0o700)
    for (const path of readdirSync(frozen, { recursive: true }).map((entry) => resolve(frozen, String(entry)))) {
      if (lstatSync(path).isDirectory()) chmodSync(path, 0o700)
    }
    rmSync(frozen, { recursive: true, force: true })
  }
}

const [, , mode, receiptPath] = process.argv
try {
  if (!receiptPath || !["generate", "verify", "deploy"].includes(mode)) {
    throw new Error("usage: staging-assets.mjs {generate|verify|deploy} RECEIPT")
  }
  const identity = await sourceIdentity()
  if (mode === "generate") {
    const built = spawnSync("pnpm", ["run", "build:sepolia"], { cwd: uiRoot, env: process.env, stdio: "inherit" })
    if (built.status !== 0) throw new Error("Staging UI build failed")
    const checked = spawnSync("node", [resolve(uiRoot, "scripts/check-sepolia-assets.mjs")], { cwd: uiRoot, env: process.env, stdio: "inherit" })
    if (checked.status !== 0) throw new Error("Staging UI profile check failed")
    const artifact = artifactSet()
    const receipt = {
      schema_version: 1,
      worker_name: workerName,
      ...identity,
      profile_sha256: sha256(readFileSync(profileFile)),
      ...artifact,
    }
    writeFileSync(resolve(receiptPath), `${JSON.stringify(receipt, null, 2)}\n`, { flag: "wx", mode: 0o600 })
    process.stdout.write(`staging_ui_artifact_set_sha256=${artifact.artifact_set_sha256}\n`)
  } else {
    const receipt = JSON.parse(readFileSync(resolve(receiptPath), "utf8"))
    const artifact = artifactSet()
    validateReceipt(receipt, identity, artifact)
    if (mode === "deploy") deployFrozen(receipt)
    process.stdout.write(`staging_ui_artifact_set_sha256=${artifact.artifact_set_sha256}\n`)
  }
} catch (error) {
  process.stderr.write(`${error instanceof Error ? error.message : String(error)}\n`)
  process.exit(1)
}
