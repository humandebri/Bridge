import { execFileSync } from "node:child_process"
import { mkdtempSync, readFileSync, writeFileSync, rmSync } from "node:fs"
import { tmpdir } from "node:os"
import { resolve } from "node:path"
import { releaseProfileSchema } from "../src/config/profile.ts"
import { Actor, HttpAgent } from "@icp-sdk/core/agent"
import { hexToBytes, toHex } from "viem"
import { idlFactory } from "../src/generated/bridge.idl.ts"
import { assertProductionUiProfile } from "../src/config/deploy-safety.ts"

const root = resolve(import.meta.dirname, "../..")
const command = process.argv[2]
if (!["dry-run", "deploy", "smoke"].includes(command))
  throw new Error("Use release.mjs dry-run|deploy|smoke")
if (process.versions.node !== "24.14.0") throw new Error("Use pinned Node.js 24.14.0")
const required = (name) => {
  if (!process.env[name]) throw new Error(`Missing ${name}`)
  return process.env[name]
}
const profileFile = required("BRIDGE_UI_RUNTIME_PROFILE_FILE")
const rawProfile = readFileSync(profileFile, "utf8")
const profile = releaseProfileSchema.parse(JSON.parse(rawProfile))
const depositId = required("BRIDGE_RECOVERY_SMOKE_DEPOSIT_ID")
const expectedHash = required("BRIDGE_RECOVERY_SMOKE_TRANSACTION_HASH").toLowerCase()
if (![depositId, expectedHash].every((value) => /^0x[0-9a-f]{64}$/i.test(value)))
  throw new Error("Use a known successful mint for smoke verification")

if (command !== "smoke") {
  if (
    execFileSync("git", ["status", "--porcelain", "--untracked-files=all"], {
      cwd: root,
      encoding: "utf8",
    }).trim()
  )
    throw new Error("Recovery release requires a clean checkout")
  execFileSync("bash", ["scripts/ci-local.sh", "proofs"], {
    cwd: root,
    stdio: "inherit",
    env: { ...process.env, PROOF_RECEIPT: required("BRIDGE_PROOF_RECEIPT") },
  })
  execFileSync(
    "python3",
    ["scripts/check_proof_impact.py", "--receipt", required("BRIDGE_PROOF_RECEIPT")],
    { cwd: root, stdio: "inherit" },
  )
  const output = execFileSync(
    "cargo",
    [
      "run",
      "--locked",
      "--quiet",
      "-p",
      "bridge-profile",
      "--",
      "verify-production-checkpoint-ui-live",
      required("BRIDGE_CHECKPOINT_EVIDENCE"),
      required("BRIDGE_UI_RPC_CONFIG"),
      profileFile,
    ],
    { cwd: root, encoding: "utf8" },
  )
  const manifest =
    /^production_ui=live-pass schema=36 activation=execute manifest_sha256=([0-9a-f]{64})$/m.exec(
      output,
    )?.[1]
  assertProductionUiProfile(profile, manifest)
  // Prove the server-side key can discover an existing success before publishing anything.
  const actor = Actor.createActor(idlFactory, {
    agent: HttpAgent.createSync({ host: profile.icHost }),
    canisterId: profile.bridgeCanisterId,
  })
  const record = (await actor.get_deposit(hexToBytes(depositId)))[0]
  const authorization = record?.mint_authorization[0]
  if (!authorization) throw new Error("Smoke deposit has no authorization")
  const from =
    authorization.finalized_block_number > profile.deploymentBlock
      ? authorization.finalized_block_number
      : profile.deploymentBlock
  const key = required("RECOVERY_ALCHEMY_API_KEY")
  const rpc = async (method, params) => {
    const response = await fetch(`https://base-mainnet.g.alchemy.com/v2/${key}`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ jsonrpc: "2.0", id: 1, method, params }),
      signal: AbortSignal.timeout(15_000),
    })
    if (!response.ok) throw new Error(`Alchemy provider smoke HTTP ${response.status}`)
    const value = await response.json()
    if (value.error || value.result === undefined) throw new Error("Alchemy provider smoke failed")
    return value.result
  }
  const toBlock = await rpc("eth_blockNumber", [])
  let pageKey
  let discovered = false
  for (let page = 0; page < 20; page++) {
    const result = await rpc("alchemy_getAssetTransfers", [
      {
        fromBlock: `0x${from.toString(16)}`,
        toBlock,
        toAddress: toHex(Uint8Array.from(authorization.recipient)),
        contractAddresses: [profile.bsnsAddress],
        category: ["erc20"],
        order: "asc",
        maxCount: "0x64",
        excludeZeroValue: true,
        ...(pageKey ? { pageKey } : {}),
      },
    ])
    if (result.transfers?.some((transfer) => transfer.hash?.toLowerCase() === expectedHash)) {
      discovered = true
      break
    }
    pageKey = result.pageKey
    if (!pageKey) break
  }
  if (!discovered) throw new Error("Alchemy could not discover the known mint; deployment stopped")
  const directory = mkdtempSync(resolve(tmpdir(), "bridge-mint-recovery-"))
  try {
    const config = JSON.parse(readFileSync(resolve(import.meta.dirname, "wrangler.jsonc"), "utf8"))
    config.main = resolve(import.meta.dirname, "index.ts")
    config.vars.RECOVERY_ENABLED = "true"
    const configFile = resolve(directory, "wrangler.json")
    const secretsFile = resolve(directory, "secrets.json")
    writeFileSync(configFile, JSON.stringify(config), { mode: 0o600 })
    writeFileSync(
      secretsFile,
      JSON.stringify({
        BRIDGE_PROFILE_JSON: rawProfile,
        ALCHEMY_API_KEY: required("RECOVERY_ALCHEMY_API_KEY"),
        CURSOR_KEY: required("RECOVERY_CURSOR_KEY"),
      }),
      { mode: 0o600 },
    )
    execFileSync(
      "pnpm",
      [
        "--dir",
        "ui",
        "exec",
        "wrangler",
        "deploy",
        "--config",
        configFile,
        "--secrets-file",
        secretsFile,
        ...(command === "dry-run" ? ["--dry-run", "--outdir", resolve(directory, "bundle")] : []),
      ],
      { cwd: root, stdio: "inherit" },
    )
  } finally {
    rmSync(directory, { recursive: true, force: true })
  }
}
if (command !== "dry-run") {
  let cursor
  let found = false
  for (let page = 0; page < 20; page++) {
    const response = await fetch("https://recovery.bridge.kinic.xyz/v1/mint-recovery", {
      method: "POST",
      headers: { Origin: "https://bridge.kinic.xyz", "Content-Type": "application/json" },
      body: JSON.stringify({ depositId, ...(cursor ? { cursor } : {}) }),
      signal: AbortSignal.timeout(20_000),
    })
    if (!response.ok) throw new Error(`Recovery smoke failed: HTTP ${response.status}`)
    const value = await response.json()
    if (
      value.deploymentInstanceId?.toLowerCase() !== profile.deploymentInstanceId?.toLowerCase() ||
      value.depositId?.toLowerCase() !== depositId.toLowerCase()
    )
      throw new Error("Recovery smoke binding mismatch")
    if (value.hashes?.includes(expectedHash)) {
      found = true
      break
    }
    cursor = value.cursor
    if (!cursor) break
    await new Promise((resolve) => setTimeout(resolve, 11_000))
  }
  if (!found) throw new Error("Known successful mint was not discovered; do not publish the UI")
  console.log("Recovery smoke passed; known successful mint discovered")
}
