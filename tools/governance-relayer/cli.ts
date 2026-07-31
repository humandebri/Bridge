#!/usr/bin/env node
import { readFile } from "node:fs/promises"
import { pathToFileURL } from "node:url"
import { Actor, HttpAgent } from "@icp-sdk/core/agent"
import { Secp256k1KeyIdentity } from "@icp-sdk/core/identity/secp256k1"
import {
  createPublicClient,
  hexToBytes,
  http,
  keccak256,
  parseTransaction,
  recoverTransactionAddress,
  type Hex,
  type TransactionSerialized,
} from "viem"
import type {
  BaseGovernanceAction,
  SignedBaseGovernanceTransaction,
  _SERVICE,
} from "../../ui/src/generated/bridge.did.ts"
import { idlFactory } from "../../integration/generated/bridge.idl.ts"

const POLL_INTERVAL_MS = 5_000

type Options = Record<string, string | boolean>
interface RelayerRpc {
  sendRawTransaction(args: { serializedTransaction: TransactionSerialized }): Promise<Hex>
  getTransactionReceipt(args: { hash: Hex }): Promise<{
    blockNumber: bigint
    blockHash: Hex
  }>
  getBlock(args: { blockTag: "finalized" } | { blockNumber: bigint }): Promise<{
    number: bigint | null
    hash: Hex
  }>
}

async function main(): Promise<void> {
  const [command = "help", ...rest] = process.argv.slice(2)
  const options = parseOptions(rest)
  if (command === "help" || options.help) {
    printHelp()
    return
  }
  const actor = await bridgeActor()

  switch (command) {
    case "prepare": {
      const result = await actor.prepare_base_governance_action(parseAction(options))
      printArtifact(unwrap(result))
      return
    }
    case "status": {
      const artifact = unwrap(await actor.get_pending_base_governance_transaction())[0]
      if (!artifact) {
        process.stdout.write("No pending governance transaction.\n")
        return
      }
      printArtifact(artifact)
      return
    }
    case "relay": {
      const rpc = rpcClient()
      const artifact = await pendingArtifact(actor, options)
      await validateArtifact(artifact)
      await relay(rpc, artifact)
      return
    }
    case "confirm": {
      const artifact = await pendingArtifact(actor, options)
      const hash = optionHash(options.hash) ?? bytesHex(artifact.transaction_hash)
      const receipt = unwrap(await actor.confirm_base_governance_transaction({
        operation_id: artifact.operation_id,
        transaction_hash: hexToBytes(hash),
      }))
      process.stdout.write(`${JSON.stringify(jsonValue(receipt))}\n`)
      return
    }
    case "replace": {
      const artifact = await pendingArtifact(actor, options)
      const result = await actor.prepare_base_governance_replacement({
        operation_id: artifact.operation_id,
        expected_transaction_hash: artifact.transaction_hash,
        max_fee_per_gas: BigInt(requiredOption(options, "max-fee")),
        max_priority_fee_per_gas: BigInt(requiredOption(options, "priority-fee")),
      })
      printArtifact(unwrap(result))
      return
    }
    case "run": {
      const rpc = rpcClient()
      const artifact = options.action
        ? unwrap(await actor.prepare_base_governance_action(parseAction(options)))
        : await pendingArtifact(actor, options)
      await validateArtifact(artifact)
      await relay(rpc, artifact)
      await waitForFinalized(rpc, bytesHex(artifact.transaction_hash))
      const confirmation = unwrap(await actor.confirm_base_governance_transaction({
        operation_id: artifact.operation_id,
        transaction_hash: artifact.transaction_hash,
      }))
      process.stdout.write(`${JSON.stringify(jsonValue(confirmation))}\n`)
      return
    }
    case "drain-emergency": {
      const rpc = rpcClient()
      for (;;) {
        const prepared = await actor.prepare_next_emergency_base_action()
        if ("Err" in prepared && "InvalidArgument" in prepared.Err) {
          process.stdout.write("Emergency Base action queue is empty.\n")
          return
        }
        const artifact = unwrap(prepared)
        await validateArtifact(artifact)
        await relay(rpc, artifact)
        await waitForFinalized(rpc, bytesHex(artifact.transaction_hash))
        unwrap(await actor.confirm_base_governance_transaction({
          operation_id: artifact.operation_id,
          transaction_hash: artifact.transaction_hash,
        }))
      }
    }
    default:
      throw new Error(`Unknown command: ${command}`)
  }
}

function rpcClient(): RelayerRpc {
  return createPublicClient({
    transport: http(requiredEnv("BASE_RPC_URL")),
  }) as unknown as RelayerRpc
}

async function bridgeActor(): Promise<_SERVICE> {
  const pemPath = requiredEnv("IC_IDENTITY_PEM")
  const pem = await readFile(pemPath, "utf8")
  const identity = Secp256k1KeyIdentity.fromPem(pem)
  const host = process.env.IC_HOST || "https://icp-api.io"
  const agent = HttpAgent.createSync({ identity, host })
  if (agent.isLocal()) await agent.fetchRootKey()
  return Actor.createActor<_SERVICE>(idlFactory, {
    agent,
    canisterId: requiredEnv("BRIDGE_CANISTER_ID"),
  })
}

async function pendingArtifact(
  actor: _SERVICE,
  options: Options,
): Promise<SignedBaseGovernanceTransaction> {
  const artifact = unwrap(await actor.get_pending_base_governance_transaction())[0]
  if (!artifact) throw new Error("No pending governance transaction")
  if (options["operation-id"] !== undefined
    && artifact.operation_id !== BigInt(String(options["operation-id"]))) {
    throw new Error("Pending operation does not match --operation-id")
  }
  return artifact
}

export async function validateArtifact(
  artifact: SignedBaseGovernanceTransaction,
): Promise<void> {
  const raw = bytesHex(artifact.raw_transaction)
  const expectedHash = bytesHex(artifact.transaction_hash)
  if (keccak256(raw) !== expectedHash) throw new Error("Canister transaction hash does not match raw transaction")
  const transaction = parseTransaction(raw as TransactionSerialized)
  const sender = await recoverTransactionAddress({
    serializedTransaction: raw as TransactionSerialized,
  })
  if (sender.toLowerCase() !== bytesHex(artifact.sender).toLowerCase()) throw new Error("Signed transaction sender mismatch")
  if (transaction.chainId !== Number(artifact.chain_id)) throw new Error("Signed transaction chain mismatch")
  if (transaction.nonce !== Number(artifact.nonce)) throw new Error("Signed transaction nonce mismatch")
  if (transaction.to?.toLowerCase() !== bytesHex(artifact.target).toLowerCase()) throw new Error("Signed transaction target mismatch")
  if ((transaction.data ?? "0x").toLowerCase() !== bytesHex(artifact.calldata).toLowerCase()) throw new Error("Signed transaction calldata mismatch")
  if (transaction.gas !== artifact.gas_limit) throw new Error("Signed transaction gas limit mismatch")
  if (transaction.maxFeePerGas !== artifact.max_fee_per_gas) throw new Error("Signed transaction max fee mismatch")
  if (transaction.maxPriorityFeePerGas !== artifact.max_priority_fee_per_gas) throw new Error("Signed transaction priority fee mismatch")
}

async function relay(
  rpc: RelayerRpc,
  artifact: SignedBaseGovernanceTransaction,
): Promise<void> {
  const raw = bytesHex(artifact.raw_transaction)
  const expectedHash = bytesHex(artifact.transaction_hash)
  try {
    const hash = await rpc.sendRawTransaction({ serializedTransaction: raw as TransactionSerialized })
    if (hash.toLowerCase() !== expectedHash.toLowerCase()) {
      throw new Error(`RPC returned unexpected transaction hash ${hash}`)
    }
    process.stdout.write(`Relayed ${hash}\n`)
  } catch (error) {
    if (isAlreadyKnown(error)) {
      process.stdout.write(`Transaction already known: ${expectedHash}\n`)
      return
    }
    throw error
  }
}

async function waitForFinalized(
  rpc: RelayerRpc,
  hash: Hex,
): Promise<void> {
  for (;;) {
    const receipt = await rpc.getTransactionReceipt({ hash }).catch(() => undefined)
    if (receipt) {
      const finalized = await rpc.getBlock({ blockTag: "finalized" })
      if (finalized.number !== null && receipt.blockNumber <= finalized.number) {
        const canonical = await rpc.getBlock({ blockNumber: receipt.blockNumber })
        if (canonical.hash !== receipt.blockHash) throw new Error("Receipt block is not canonical")
        return
      }
    }
    await new Promise((resolve) => setTimeout(resolve, POLL_INTERVAL_MS))
  }
}

function parseAction(options: Options): BaseGovernanceAction {
  switch (requiredOption(options, "action")) {
    case "pause-deposits":
      return { PauseDepositMints: null }
    case "pause-withdrawals":
      return { PauseWithdrawals: null }
    case "cancel-timelock":
      return { CancelPendingTimelock: null }
    case "set-service-fee":
      return { SetServiceFee: { value: BigInt(requiredOption(options, "value")) } }
    default:
      throw new Error("Unsupported --action")
  }
}

export function parseOptions(args: string[]): Options {
  const options: Options = {}
  for (let index = 0; index < args.length; index += 1) {
    const argument = args[index]
    if (!argument?.startsWith("--")) throw new Error(`Unexpected argument: ${argument}`)
    const [rawName, inlineValue] = argument.slice(2).split("=", 2)
    if (!rawName) throw new Error("Empty option name")
    if (inlineValue !== undefined) {
      options[rawName] = inlineValue
      continue
    }
    const next = args[index + 1]
    if (!next || next.startsWith("--")) {
      options[rawName] = true
    } else {
      options[rawName] = next
      index += 1
    }
  }
  return options
}

function unwrap<T, E>(result: { Ok: T } | { Err: E }): T {
  if ("Err" in result) throw new Error(`Canister rejected request: ${JSON.stringify(jsonValue(result.Err))}`)
  return result.Ok
}

function requiredEnv(name: string): string {
  const value = process.env[name]
  if (!value) throw new Error(`${name} is required`)
  return value
}

function requiredOption(options: Options, name: string): string {
  const value = options[name]
  if (typeof value !== "string" || value.length === 0) throw new Error(`--${name} is required`)
  return value
}

function optionHash(value: string | boolean | undefined): Hex | undefined {
  if (value === undefined) return undefined
  if (typeof value !== "string" || !/^0x[0-9a-fA-F]{64}$/.test(value)) throw new Error("--hash must be 32-byte hex")
  return value as Hex
}

function bytesHex(value: Uint8Array | number[]): Hex {
  return `0x${Array.from(value, (byte) => Number(byte).toString(16).padStart(2, "0")).join("")}`
}

export function isAlreadyKnown(error: unknown): boolean {
  const message = error instanceof Error ? error.message : String(error)
  return /already known|known transaction/i.test(message)
}

export function redactedErrorMessage(
  error: unknown,
  environment: NodeJS.ProcessEnv = process.env,
): string {
  let message = error instanceof Error ? error.message : String(error)
  for (const name of ["BASE_RPC_URL", "IC_IDENTITY_PEM"]) {
    const secret = environment[name]
    if (secret) message = message.split(secret).join("[REDACTED]")
  }
  return message
}

function jsonValue(value: unknown): unknown {
  if (typeof value === "bigint") return value.toString()
  if (value instanceof Uint8Array) return bytesHex(value)
  if (Array.isArray(value)) return value.map(jsonValue)
  if (value && typeof value === "object") {
    return Object.fromEntries(Object.entries(value).map(([key, child]) => [key, jsonValue(child)]))
  }
  return value
}

function printArtifact(artifact: SignedBaseGovernanceTransaction): void {
  process.stdout.write(`${JSON.stringify(jsonValue(artifact), null, 2)}\n`)
}

function printHelp(): void {
  process.stdout.write(`Usage: npm run governance-relayer -- <command> [options]

Commands:
  prepare --action pause-deposits|pause-withdrawals|cancel-timelock|set-service-fee [--value N]
  status [--operation-id N]
  relay [--operation-id N]
  confirm [--operation-id N] [--hash 0x...]
  run [--operation-id N | --action ...]
  replace --operation-id N --max-fee N --priority-fee N
  drain-emergency

Environment:
  BRIDGE_CANISTER_ID  Bridge Canister principal
  IC_IDENTITY_PEM    Governance secp256k1 identity PEM path
  BASE_RPC_URL       Base JSON-RPC URL
  IC_HOST            Optional IC API host (defaults to https://icp-api.io)
`)
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main().catch((error: unknown) => {
    process.stderr.write(`${redactedErrorMessage(error)}\n`)
    process.exitCode = 1
  })
}
