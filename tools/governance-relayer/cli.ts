#!/usr/bin/env node
import { createHash, createPrivateKey } from "node:crypto"
import { readFile, writeFile } from "node:fs/promises"
import { pathToFileURL } from "node:url"
import { Actor, HttpAgent } from "@icp-sdk/core/agent"
import { Ed25519KeyIdentity } from "@icp-sdk/core/identity"
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

type Options = Record<string, string | boolean>
interface RelayerRpc {
  sendRawTransaction(args: { serializedTransaction: TransactionSerialized }): Promise<Hex>
  getTransactionReceipt(args: { hash: Hex }): Promise<{
    blockNumber: bigint
    blockHash: Hex
    status: "success" | "reverted"
  }>
  getBlock(args: { blockTag: "finalized" } | { blockNumber: bigint }): Promise<{
    number: bigint | null
    hash: Hex
  }>
}

export interface FinalizedTransactionOutcome {
  blockNumber: bigint
  blockHash: Hex
  status: "success" | "reverted"
}

const POLL_INTERVAL_MS = 5_000

async function main(): Promise<void> {
  const [command = "help", ...rest] = process.argv.slice(2)
  const options = parseOptions(rest)
  validateCommandOptions(command, options)
  if (command === "help" || options.help) {
    printHelp()
    return
  }
  const actor = await bridgeActor(commandRequiresIdentity(command))
  switch (command) {
    case "prepare": {
      const result = await actor.prepare_base_governance_action(parseAction(options))
      printArtifact(unwrap(result))
      return
    }
    case "seal-operational-config": {
      const parametersPath = requiredOption(options, "parameters-file")
      const parametersBytes = await readFile(parametersPath)
      const parsed = JSON.parse(parametersBytes.toString("utf8")) as {
        derived?: Record<string, unknown>
        governance_operation_id?: unknown
      }
      const derived = parsed.derived
      if (!derived || typeof derived !== "object") {
        throw new Error("Initial operational parameters have no derived values")
      }
      const natural = (name: string): bigint => {
        const value = derived[name]
        if ((typeof value !== "string" && typeof value !== "number")
          || !/^(0|[1-9][0-9]*)$/.test(String(value))) {
          throw new Error(`Invalid derived operational parameter: ${name}`)
        }
        return BigInt(value)
      }
      parseExpectedGovernanceOperationId(parsed.governance_operation_id)
      const receipt = unwrap(await actor.seal_operational_config({
        governance_evm_fee: {
          gas_limit_ceiling: natural("gas_limit_ceiling"),
          max_fee_per_gas_ceiling: natural("max_fee_per_gas_ceiling"),
          max_priority_fee_per_gas_ceiling: natural("max_priority_fee_per_gas_ceiling"),
          l1_fee_per_transaction_ceiling_wei: natural("l1_fee_per_transaction_ceiling_wei"),
          quote_validity_seconds: natural("quote_validity_seconds"),
          gas_limit_multiplier_bps: Number(natural("gas_limit_multiplier_bps")),
          base_fee_multiplier_bps: Number(natural("base_fee_multiplier_bps")),
          l1_fee_multiplier_bps: Number(natural("l1_fee_multiplier_bps")),
        },
        cycles_floor: natural("cycles_floor"),
        settlement_cycle_ceiling: natural("settlement_cycle_ceiling"),
      }))
      const evidence = {
        schema_version: 1,
        parameters_sha256: createHash("sha256").update(parametersBytes).digest("hex"),
        sealed_at_unix: Math.floor(Date.now() / 1_000),
        response: jsonValue(receipt),
      }
      await writeJsonNew(evidence, requiredOption(options, "receipt-file"))
      process.stdout.write(`${JSON.stringify(evidence)}\n`)
      return
    }
    case "status": {
      const artifact = selectPendingArtifact(
        unwrap(await actor.get_pending_base_governance_transaction()),
        options["operation-id"],
      )
      if (!artifact) {
        process.stdout.write("No pending governance transaction.\n")
        return
      }
      printArtifact(artifact)
      return
    }
    case "recover-activation": {
      const phase = requiredOption(options, "phase")
      if (phase !== "schedule" && phase !== "execute") {
        throw new Error("--phase must be schedule or execute")
      }
      const pending = unwrap(await actor.get_pending_base_governance_transaction())
      const artifact = selectPendingActivationArtifact(pending, phase)
      const authorization = JSON.parse(
        (await readFile(requiredOption(options, "authorization-file"))).toString("utf8"),
      ) as Record<string, unknown>
      const expectedGate = requiredEnv("BRIDGE_GATE_B_MANIFEST_SHA256").toLowerCase()
      if (authorization.schema_version !== 1
        || authorization.phase !== phase
        || authorization.gate_b_manifest_sha256 !== expectedGate
        || typeof authorization.authorized_at_unix !== "number"
        || !Number.isSafeInteger(authorization.authorized_at_unix)
        || authorization.authorized_at_unix <= 0
        || artifact.signed_at_ns < BigInt(authorization.authorized_at_unix) * 1_000_000_000n) {
        throw new Error("Live activation pending transaction predates or differs from its authorization")
      }
      await writeOrMatchArtifact(artifact, requiredOption(options, "artifact-file"))
      printArtifact(artifact)
      return
    }
    case "relay": {
      const rpc = rpcClient()
      const artifact = await pendingArtifact(actor, options)
      const artifactBytes = await requireMatchingArtifactFile(artifact, options)
      await requireMatchingActivationBinding(artifact, artifactBytes, options)
      await validateArtifact(artifact)
      await relay(rpc, artifact)
      return
    }
    case "confirm": {
      const artifactPath = requiredOption(options, "artifact-file")
      const artifactBytes = await readFile(artifactPath)
      const stored = JSON.parse(artifactBytes.toString("utf8"))
      const storedIsActivation = isActivationArtifact(stored as { kind: unknown })
      const storedIdentity = storedIsActivation
        ? storedActivationConfirmationIdentity(stored)
        : undefined
      if (storedIdentity && options["operation-id"] !== undefined
        && options["operation-id"] !== storedIdentity.operationId.toString()) {
        throw new Error("--operation-id differs from the fixed activation artifact")
      }
      const pending = selectPendingArtifact(
        unwrap(await actor.get_pending_base_governance_transaction()),
        storedIdentity?.operationId.toString() ?? options["operation-id"],
      )
      let operationId: bigint
      let expectedHash: Hex
      let artifactToValidate: unknown
      if (pending && storedIdentity && activationAttemptMatchesPendingLineage(stored, pending)) {
        await requireActivationBindingBytes(artifactBytes, options)
        operationId = storedIdentity.operationId
        expectedHash = storedIdentity.transactionHash
        artifactToValidate = stored
      } else if (pending && storedArtifactMatches(stored, pending)) {
        await requireMatchingActivationBinding(pending, artifactBytes, options)
        operationId = pending.operation_id
        expectedHash = bytesHex(pending.transaction_hash)
        artifactToValidate = jsonValue(pending)
      } else if (!pending && storedIdentity) {
        await requireActivationBindingBytes(artifactBytes, options)
        operationId = storedIdentity.operationId
        expectedHash = storedIdentity.transactionHash
        artifactToValidate = stored
      } else {
        throw new Error("Fixed governance artifact differs from the live pending transaction")
      }
      const hash = storedIsActivation
        ? activationConfirmationHash(options, expectedHash)
        : confirmationHash(options) ?? expectedHash
      const receipt = unwrap(await afterValidatingStoredArtifacts(
        [artifactToValidate],
        () => actor.confirm_base_governance_transaction({
          operation_id: operationId,
          transaction_hash: hexToBytes(hash),
        }),
      ))
      const evidence = {
        schema_version: 1,
        confirmed_at_unix: Math.floor(Date.now() / 1_000),
        artifact_sha256: createHash("sha256").update(artifactBytes).digest("hex"),
        operation_id: operationId.toString(),
        transaction_hash: hash,
        response: jsonValue(receipt),
      }
      await writeOrMatchConfirmationEvidence(
        evidence,
        requiredOption(options, "receipt-file"),
      )
      process.stdout.write(`${JSON.stringify(evidence)}\n`)
      return
    }
    case "replace": {
      const artifact = await pendingArtifact(actor, options)
      if (isActivationArtifact(artifact)) {
        throw new Error("Activation replacement requires the production activation driver")
      }
      await requireMatchingArtifactFile(artifact, options)
      const result = await actor.prepare_base_governance_replacement({
        operation_id: artifact.operation_id,
        expected_transaction_hash: artifact.transaction_hash,
        max_fee_per_gas: BigInt(requiredOption(options, "max-fee")),
        max_priority_fee_per_gas: BigInt(requiredOption(options, "priority-fee")),
      })
      const replacement = unwrap(result)
      await writeArtifactNew(replacement, requiredOption(options, "output-artifact-file"))
      printArtifact(replacement)
      return
    }
    case "replace-activation": {
      const artifact = await pendingArtifact(actor, options)
      const oldPath = requiredOption(options, "artifact-file")
      const oldBytes = await readFile(oldPath)
      const stored = JSON.parse(oldBytes.toString("utf8"))
      if (!isActivationArtifact(stored as { kind: unknown })) {
        throw new Error("replace-activation requires an activation artifact")
      }
      await requireActivationBindingBytes(oldBytes, options)
      const maxFee = BigInt(requiredOption(options, "max-fee"))
      const priorityFee = BigInt(requiredOption(options, "priority-fee"))
      const replacement = await afterValidatingStoredArtifacts(
        [stored, jsonValue(artifact)],
        async (): Promise<SignedBaseGovernanceTransaction> => {
          if (storedArtifactMatches(stored, artifact)) {
            return unwrap(await actor.prepare_base_governance_replacement({
              operation_id: artifact.operation_id,
              expected_transaction_hash: artifact.transaction_hash,
              max_fee_per_gas: maxFee,
              max_priority_fee_per_gas: priorityFee,
            }))
          }
          if (activationReplacementMatches(stored, artifact, maxFee, priorityFee)) {
            return artifact
          }
          throw new Error("Live activation pending transaction is not the authorized replacement")
        },
      )
      const outputArtifact = requiredOption(options, "output-artifact-file")
      await writeOrMatchArtifact(replacement, outputArtifact)
      const replacementBytes = await readFile(outputArtifact)
      await writeOrMatchActivationBinding(
        replacement,
        replacementBytes,
        options,
        requiredOption(options, "output-binding-file"),
      )
      printArtifact(replacement)
      return
    }
    case "run": {
      if (options.action !== undefined) {
        throw new Error("run relays an existing signed transaction; use prepare separately")
      }
      const rpc = rpcClient()
      const artifact = await pendingArtifact(actor, options)
      if (isActivationArtifact(artifact)) {
        throw new Error("run cannot relay or confirm an activation transaction")
      }
      await validateArtifact(artifact)
      await relay(rpc, artifact)
      const outcome = await waitForFinalized(rpc, bytesHex(artifact.transaction_hash))
      if (outcome.status === "reverted") {
        throw new Error(`Transaction reverted; stopped before finality polling. Confirm it after finalization: ${bytesHex(artifact.transaction_hash)}`)
      }
      const confirmationResult = await actor.confirm_base_governance_transaction({
        operation_id: artifact.operation_id,
        transaction_hash: artifact.transaction_hash,
      })
      process.stdout.write(`${JSON.stringify(jsonValue(unwrap(confirmationResult)))}\n`)
      return
    }
    case "refresh-attestation": {
      const attestation = unwrap(await actor.refresh_activation_attestation())
      process.stdout.write(`${JSON.stringify(jsonValue(attestation))}\n`)
      return
    }
    case "prepare-schedule-activation": {
      const artifact = unwrap(await actor.schedule_activation())
      await writeOrMatchArtifact(artifact, requiredOption(options, "artifact-file"))
      printArtifact(artifact)
      return
    }
    case "prepare-execute-activation": {
      const artifact = unwrap(await actor.execute_activation())
      await writeOrMatchArtifact(artifact, requiredOption(options, "artifact-file"))
      printArtifact(artifact)
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
        const outcome = await waitForFinalized(rpc, bytesHex(artifact.transaction_hash))
        if (outcome.status === "reverted") {
          throw new Error(`Emergency Base action reverted; stopped before finality polling. Confirm it after finalization: ${bytesHex(artifact.transaction_hash)}`)
        }
        const confirmationResult = await actor.confirm_base_governance_transaction({
          operation_id: artifact.operation_id,
          transaction_hash: artifact.transaction_hash,
        })
        unwrap(confirmationResult)
      }
    }
    default:
      throw new Error(`Unknown command: ${command}`)
  }
}

export function parseExpectedGovernanceOperationId(value: unknown): bigint {
  if ((typeof value !== "string" && typeof value !== "number")
    || (typeof value === "number" && !Number.isSafeInteger(value))
    || !/^(0|[1-9][0-9]*)$/.test(String(value))) {
    throw new Error("Invalid expected governance operation ID")
  }
  const operationId = BigInt(value)
  if (operationId !== 0n) {
    throw new Error("Initial governance operation ID must be 0")
  }
  return operationId
}

export function storedActivationConfirmationIdentity(value: unknown): {
  operationId: bigint
  transactionHash: Hex
} {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    throw new Error("Fixed activation artifact is malformed")
  }
  const artifact = value as Record<string, unknown>
  if (typeof artifact.operation_id !== "string" || !/^(0|[1-9][0-9]*)$/.test(artifact.operation_id)
    || typeof artifact.transaction_hash !== "string"
    || !/^0x[0-9a-fA-F]{64}$/.test(artifact.transaction_hash)) {
    throw new Error("Fixed activation artifact has an invalid operation ID or transaction hash")
  }
  return {
    operationId: BigInt(artifact.operation_id),
    transactionHash: artifact.transaction_hash as Hex,
  }
}

export function commandRequiresIdentity(command: string): boolean {
  return !new Set(["status", "relay", "recover-activation"]).has(command)
}

function rpcClient(): RelayerRpc {
  return createPublicClient({
    transport: http(requiredEnv("BASE_RPC_URL")),
  }) as unknown as RelayerRpc
}

async function bridgeActor(authenticated: boolean): Promise<_SERVICE> {
  const host = process.env.IC_HOST || "https://icp-api.io"
  const agent = authenticated
    ? HttpAgent.createSync({
      identity: identityFromPem(await readFile(requiredEnv("IC_IDENTITY_PEM"), "utf8")),
      host,
    })
    : HttpAgent.createSync({ host })
  if (agent.isLocal()) await agent.fetchRootKey()
  return Actor.createActor<_SERVICE>(idlFactory, {
    agent,
    canisterId: requiredEnv("BRIDGE_CANISTER_ID"),
  })
}

export function identityFromPem(pem: string): Ed25519KeyIdentity | Secp256k1KeyIdentity {
  const key = createPrivateKey(pem)
  const jwk = key.export({ format: "jwk" })
  if (!jwk.d) throw new Error("Identity PEM does not contain a private key")
  const secretKey = new Uint8Array(Buffer.from(jwk.d, "base64url"))
  if (key.asymmetricKeyType === "ed25519" && jwk.crv === "Ed25519") {
    return Ed25519KeyIdentity.fromSecretKey(secretKey)
  }
  if (key.asymmetricKeyType === "ec" && jwk.crv === "secp256k1") {
    return Secp256k1KeyIdentity.fromSecretKey(secretKey)
  }
  throw new Error(`Unsupported identity PEM key type: ${jwk.crv ?? key.asymmetricKeyType}`)
}

async function pendingArtifact(
  actor: _SERVICE,
  options: Options,
): Promise<SignedBaseGovernanceTransaction> {
  const operationId = options["operation-id"]
  const artifact = selectPendingArtifact(
    unwrap(await actor.get_pending_base_governance_transaction()),
    operationId,
  )
  if (!artifact && operationId !== undefined) {
    throw new Error("No pending governance transaction matches --operation-id")
  }
  if (!artifact) throw new Error("No pending governance transaction")
  return artifact
}

async function writeArtifactNew(
  artifact: SignedBaseGovernanceTransaction,
  path: string,
): Promise<void> {
  await validateArtifact(artifact)
  const body = `${JSON.stringify(jsonValue(artifact), null, 2)}\n`
  await writeFile(path, body, { encoding: "utf8", flag: "wx", mode: 0o400 })
}

async function writeOrMatchArtifact(
  artifact: SignedBaseGovernanceTransaction,
  path: string,
): Promise<void> {
  await validateArtifact(artifact)
  try {
    await writeArtifactNew(artifact, path)
  } catch (error) {
    let stored: unknown
    try {
      stored = JSON.parse((await readFile(path)).toString("utf8"))
    } catch {
      throw error
    }
    if (!storedArtifactMatches(stored, artifact)) throw error
  }
}

async function writeJsonNew(value: unknown, path: string): Promise<void> {
  await writeFile(path, `${JSON.stringify(value, null, 2)}\n`, {
    encoding: "utf8",
    flag: "wx",
    mode: 0o400,
  })
}

export function confirmationEvidenceMatches(stored: unknown, candidate: unknown): boolean {
  if (!stored || typeof stored !== "object" || Array.isArray(stored)
    || !candidate || typeof candidate !== "object" || Array.isArray(candidate)) return false
  const previous = stored as Record<string, unknown>
  const current = candidate as Record<string, unknown>
  const previousKeys = Object.keys(previous).sort().join(",")
  if (previousKeys !== Object.keys(current).sort().join(",")) return false
  if (typeof previous.confirmed_at_unix !== "number"
    || !Number.isSafeInteger(previous.confirmed_at_unix)
    || previous.confirmed_at_unix <= 0
    || typeof current.confirmed_at_unix !== "number"
    || previous.confirmed_at_unix > current.confirmed_at_unix) return false
  const normalizedPrevious = { ...previous, confirmed_at_unix: current.confirmed_at_unix }
  return JSON.stringify(normalizedPrevious) === JSON.stringify(current)
}

export async function writeOrMatchConfirmationEvidence(
  value: unknown,
  path: string,
): Promise<void> {
  try {
    await writeJsonNew(value, path)
  } catch (error) {
    let stored: unknown
    try {
      stored = JSON.parse((await readFile(path)).toString("utf8"))
    } catch {
      throw error
    }
    if (!confirmationEvidenceMatches(stored, value)) throw error
  }
}

async function requireMatchingArtifactFile(
  artifact: SignedBaseGovernanceTransaction,
  options: Options,
): Promise<Buffer> {
  const path = requiredOption(options, "artifact-file")
  let bytes: Buffer
  let stored: unknown
  try {
    bytes = await readFile(path)
    stored = JSON.parse(bytes.toString("utf8"))
  } catch (error) {
    throw new Error(`Cannot read the fixed governance artifact: ${String(error)}`)
  }
  if (!storedArtifactMatches(stored, artifact)) {
    throw new Error("Fixed governance artifact differs from the live pending transaction")
  }
  return bytes
}

async function requireMatchingActivationBinding(
  artifact: SignedBaseGovernanceTransaction,
  artifactBytes: Buffer,
  options: Options,
): Promise<void> {
  const path = options["binding-file"]
  if (path === undefined) {
    if (isActivationArtifact(artifact)) {
      throw new Error("Activation relay and confirmation require --binding-file")
    }
    return
  }
  if (typeof path !== "string") throw new Error("--binding-file requires a path")
  const authorizationPath = requiredOption(options, "authorization-file")
  const authorizationBytes = await readFile(authorizationPath)
  const authorization = JSON.parse(authorizationBytes.toString("utf8")) as Record<string, unknown>
  const expectedGate = requiredEnv("BRIDGE_GATE_B_MANIFEST_SHA256").toLowerCase()
  const artifactSha256 = createHash("sha256").update(artifactBytes).digest("hex")
  let value: unknown
  try {
    value = JSON.parse((await readFile(path)).toString("utf8"))
  } catch (error) {
    throw new Error(`Cannot read the activation binding: ${String(error)}`)
  }
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    throw new Error("Activation binding is malformed")
  }
  const binding = value as Record<string, unknown>
  const authorizationSha256 = createHash("sha256").update(authorizationBytes).digest("hex")
  if (!activationBindingMatches(binding, artifactSha256, expectedGate)
    || binding.authorization_receipt_sha256 !== authorizationSha256
    || binding.phase !== activationPhase(artifact)
    || authorization.schema_version !== 1
    || authorization.phase !== binding.phase
    || authorization.gate_b_manifest_sha256 !== expectedGate) {
    throw new Error("Activation binding differs from the fixed Gate B artifact")
  }
}

async function requireActivationBindingBytes(
  artifactBytes: Buffer,
  options: Options,
): Promise<Record<string, unknown>> {
  const path = requiredOption(options, "binding-file")
  const authorizationPath = requiredOption(options, "authorization-file")
  const expectedGate = requiredEnv("BRIDGE_GATE_B_MANIFEST_SHA256").toLowerCase()
  const artifactSha256 = createHash("sha256").update(artifactBytes).digest("hex")
  const authorizationBytes = await readFile(authorizationPath)
  const authorization = JSON.parse(authorizationBytes.toString("utf8")) as Record<string, unknown>
  const authorizationSha256 = createHash("sha256").update(authorizationBytes).digest("hex")
  const binding = JSON.parse((await readFile(path)).toString("utf8")) as Record<string, unknown>
  const storedArtifact = JSON.parse(artifactBytes.toString("utf8")) as { kind: unknown }
  if (!activationBindingMatches(binding, artifactSha256, expectedGate)
    || binding.authorization_receipt_sha256 !== authorizationSha256
    || binding.phase !== activationPhase(storedArtifact)
    || authorization.schema_version !== 1
    || authorization.phase !== binding.phase
    || authorization.gate_b_manifest_sha256 !== expectedGate) {
    throw new Error("Activation binding differs from the fixed Gate B artifact")
  }
  return binding
}

async function writeOrMatchActivationBinding(
  artifact: SignedBaseGovernanceTransaction,
  artifactBytes: Buffer,
  options: Options,
  path: string,
): Promise<void> {
  const source = await requireActivationBindingBytes(
    await readFile(requiredOption(options, "artifact-file")),
    options,
  )
  const value = {
    schema_version: 1,
    phase: activationPhase(artifact),
    gate_b_manifest_sha256: source.gate_b_manifest_sha256,
    artifact_sha256: createHash("sha256").update(artifactBytes).digest("hex"),
    authorization_receipt_sha256: source.authorization_receipt_sha256,
    bound_at_unix: Math.floor(Date.now() / 1_000),
  }
  try {
    await writeJsonNew(value, path)
  } catch (error) {
    const existing = JSON.parse((await readFile(path)).toString("utf8")) as Record<string, unknown>
    if (!activationBindingMatches(
      existing,
      value.artifact_sha256,
      String(value.gate_b_manifest_sha256),
    ) || existing.authorization_receipt_sha256 !== value.authorization_receipt_sha256
      || existing.phase !== value.phase) throw error
  }
}

function activationPhase(artifact: { kind: unknown }): "schedule" | "execute" {
  if (artifact.kind && typeof artifact.kind === "object" && "ScheduleActivation" in artifact.kind) return "schedule"
  if (artifact.kind && typeof artifact.kind === "object" && "ExecuteActivation" in artifact.kind) return "execute"
  throw new Error("Artifact is not an activation transaction")
}

export function activationReplacementMatches(
  stored: unknown,
  live: SignedBaseGovernanceTransaction,
  maxFee: bigint,
  priorityFee: bigint,
): boolean {
  if (!stored || typeof stored !== "object" || Array.isArray(stored)) return false
  const old = stored as Record<string, unknown>
  const current = jsonValue(live) as Record<string, unknown>
  const same = ["operation_id", "kind", "chain_id", "nonce", "sender", "target", "calldata", "gas_limit"]
    .every((field) => JSON.stringify(old[field]) === JSON.stringify(current[field]))
  const oldGeneration = Number(old.generation)
  return same
    && Number.isSafeInteger(oldGeneration)
    && live.generation === oldGeneration + 1
    && live.max_fee_per_gas === maxFee
    && live.max_priority_fee_per_gas === priorityFee
}

export function activationAttemptMatchesPendingLineage(
  stored: unknown,
  live: SignedBaseGovernanceTransaction,
): boolean {
  if (!stored || typeof stored !== "object" || Array.isArray(stored)
    || !isActivationArtifact(live)) return false
  const old = stored as Record<string, unknown>
  if (!isActivationArtifact(old as { kind: unknown })) return false
  const current = jsonValue(live) as Record<string, unknown>
  const invariantFields = [
    "operation_id", "kind", "chain_id", "nonce", "sender", "target", "calldata", "gas_limit",
  ]
  if (!invariantFields.every(
    (field) => JSON.stringify(old[field]) === JSON.stringify(current[field]),
  )) return false
  const oldGeneration = old.generation
  const oldSignedAt = old.signed_at_ns
  const oldMaxFee = old.max_fee_per_gas
  const oldPriorityFee = old.max_priority_fee_per_gas
  if (typeof oldGeneration !== "number" || !Number.isInteger(oldGeneration)
    || oldGeneration < 0 || oldGeneration > live.generation
    || typeof oldSignedAt !== "string" || !/^(0|[1-9][0-9]*)$/.test(oldSignedAt)
    || typeof oldMaxFee !== "string" || !/^(0|[1-9][0-9]*)$/.test(oldMaxFee)
    || typeof oldPriorityFee !== "string" || !/^(0|[1-9][0-9]*)$/.test(oldPriorityFee)) return false
  const signedAt = BigInt(oldSignedAt)
  const maxFee = BigInt(oldMaxFee)
  const priorityFee = BigInt(oldPriorityFee)
  if (signedAt === 0n || signedAt > live.signed_at_ns
    || maxFee > live.max_fee_per_gas
    || priorityFee > live.max_priority_fee_per_gas) return false
  return oldGeneration < live.generation || storedArtifactMatches(stored, live)
}

export function isActivationArtifact(artifact: { kind: unknown }): boolean {
  if (!artifact.kind || typeof artifact.kind !== "object" || Array.isArray(artifact.kind)) return false
  return "ScheduleActivation" in artifact.kind || "ExecuteActivation" in artifact.kind
}

export function activationBindingMatches(
  binding: Record<string, unknown>,
  artifactSha256: string,
  expectedGate: string,
): boolean {
  const fields = Object.keys(binding).sort().join(",")
  const expectedFields = [
    "artifact_sha256", "authorization_receipt_sha256", "bound_at_unix",
    "gate_b_manifest_sha256", "phase", "schema_version",
  ].sort().join(",")
  return !(fields !== expectedFields
    || binding.schema_version !== 1
    || binding.gate_b_manifest_sha256 !== expectedGate
    || binding.artifact_sha256 !== artifactSha256
    || (binding.phase !== "schedule" && binding.phase !== "execute")
    || typeof binding.authorization_receipt_sha256 !== "string"
    || !/^[0-9a-f]{64}$/.test(binding.authorization_receipt_sha256)
    || typeof binding.bound_at_unix !== "number"
    || !Number.isSafeInteger(binding.bound_at_unix)
    || binding.bound_at_unix <= 0)
}

export function storedArtifactMatches(stored: unknown, live: unknown): boolean {
  return JSON.stringify(stored) === JSON.stringify(jsonValue(live))
}

export function selectPendingArtifact<T extends { operation_id: bigint }>(
  artifacts: readonly T[],
  operationId: string | boolean | undefined,
): T | undefined {
  if (operationId !== undefined) {
    if (typeof operationId !== "string" || !/^(0|[1-9][0-9]*)$/.test(operationId)) {
      throw new Error("--operation-id must be a non-negative integer")
    }
    const expected = BigInt(operationId)
    return artifacts.find((artifact) => artifact.operation_id === expected)
  }
  if (artifacts.length > 1) {
    throw new Error("--operation-id is required when multiple governance transactions are pending")
  }
  return artifacts[0]
}

export function selectPendingActivationArtifact(
  artifacts: readonly SignedBaseGovernanceTransaction[],
  phase: "schedule" | "execute",
): SignedBaseGovernanceTransaction {
  const matching = artifacts.filter(
    (artifact) => isActivationArtifact(artifact) && activationPhase(artifact) === phase,
  )
  if (matching.length !== 1) {
    throw new Error(`Expected exactly one pending ${phase} activation transaction`)
  }
  return matching[0]!
}

export async function validateArtifact(
  artifact: SignedBaseGovernanceTransaction,
): Promise<void> {
  await validateStoredArtifact(jsonValue(artifact))
}

export async function validateStoredArtifact(value: unknown): Promise<void> {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    throw new Error("Signed governance artifact is malformed")
  }
  const artifact = value as Record<string, unknown>
  const hexField = (name: string): Hex => {
    const field = artifact[name]
    if (typeof field !== "string" || !/^0x[0-9a-fA-F]*$/.test(field)) {
      throw new Error(`Signed governance artifact has invalid ${name}`)
    }
    return field as Hex
  }
  const natural = (name: string): bigint => {
    const field = artifact[name]
    if (typeof field !== "string" || !/^(0|[1-9][0-9]*)$/.test(field)) {
      throw new Error(`Signed governance artifact has invalid ${name}`)
    }
    return BigInt(field)
  }
  const expectedFields = [
    "operation_id", "kind", "chain_id", "sender", "nonce", "target", "calldata",
    "gas_limit", "max_fee_per_gas", "max_priority_fee_per_gas", "raw_transaction",
    "transaction_hash", "generation", "signed_at_ns",
  ].sort().join(",")
  if (Object.keys(artifact).sort().join(",") !== expectedFields) {
    throw new Error("Signed governance artifact has unexpected fields")
  }
  const maxU64 = 18_446_744_073_709_551_615n
  if (natural("operation_id") > maxU64 || natural("chain_id") > maxU64
    || natural("nonce") > maxU64 || natural("signed_at_ns") === 0n
    || natural("signed_at_ns") > maxU64) {
    throw new Error("Signed governance artifact has an out-of-range nat64 field")
  }
  if (typeof artifact.generation !== "number" || !Number.isInteger(artifact.generation)
    || artifact.generation < 0 || artifact.generation > 255) {
    throw new Error("Signed governance artifact has an invalid generation")
  }
  if (!artifact.kind || typeof artifact.kind !== "object" || Array.isArray(artifact.kind)) {
    throw new Error("Signed governance artifact has an invalid operation kind")
  }
  const kind = artifact.kind as Record<string, unknown>
  const kindKeys = Object.keys(kind)
  if (kindKeys.length !== 1) {
    throw new Error("Signed governance artifact must have exactly one operation kind")
  }
  if (kindKeys[0] === "ScheduleActivation" || kindKeys[0] === "ExecuteActivation") {
    const operation = kind[kindKeys[0]]
    if (!operation || typeof operation !== "object" || Array.isArray(operation)
      || Object.keys(operation).sort().join(",") !== "operation_id,salt") {
      throw new Error("Signed governance artifact has invalid activation kind fields")
    }
    const activation = operation as Record<string, unknown>
    for (const field of ["operation_id", "salt"]) {
      if (typeof activation[field] !== "string" || !/^0x[0-9a-fA-F]{64}$/.test(activation[field])) {
        throw new Error(`Signed governance artifact has invalid activation ${field}`)
      }
    }
  }
  const raw = hexField("raw_transaction")
  const expectedHash = hexField("transaction_hash")
  if (keccak256(raw) !== expectedHash) throw new Error("Canister transaction hash does not match raw transaction")
  const transaction = parseTransaction(raw as TransactionSerialized)
  const sender = await recoverTransactionAddress({
    serializedTransaction: raw as TransactionSerialized,
  })
  if (sender.toLowerCase() !== hexField("sender").toLowerCase()) throw new Error("Signed transaction sender mismatch")
  if (transaction.chainId === undefined || transaction.nonce === undefined) {
    throw new Error("Signed governance transaction must bind chain ID and nonce")
  }
  if (BigInt(transaction.chainId) !== natural("chain_id")) throw new Error("Signed transaction chain mismatch")
  if (BigInt(transaction.nonce) !== natural("nonce")) throw new Error("Signed transaction nonce mismatch")
  if (transaction.to?.toLowerCase() !== hexField("target").toLowerCase()) throw new Error("Signed transaction target mismatch")
  if ((transaction.data ?? "0x").toLowerCase() !== hexField("calldata").toLowerCase()) throw new Error("Signed transaction calldata mismatch")
  if (transaction.gas !== natural("gas_limit")) throw new Error("Signed transaction gas limit mismatch")
  if (transaction.maxFeePerGas !== natural("max_fee_per_gas")) throw new Error("Signed transaction max fee mismatch")
  if (transaction.maxPriorityFeePerGas !== natural("max_priority_fee_per_gas")) throw new Error("Signed transaction priority fee mismatch")
  if ((transaction.value ?? 0n) !== 0n
    || ("accessList" in transaction && (transaction.accessList?.length ?? 0) !== 0)) {
    throw new Error("Signed governance transaction must have zero value and an empty access list")
  }
}

export async function afterValidatingStoredArtifacts<T>(
  values: readonly unknown[],
  effect: () => Promise<T>,
): Promise<T> {
  for (const value of values) await validateStoredArtifact(value)
  return effect()
}

export async function relay(
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
    process.stdout.write(`Unverified submission accepted by configured RPC: ${hash}\n`)
  } catch (error) {
    if (isAlreadyKnown(error)) {
      process.stdout.write(`Unverified configured-RPC response (already known): ${expectedHash}\n`)
      return
    }
    if (isNonceTooLow(error)) {
      const receipt = await rpc.getTransactionReceipt({ hash: expectedHash }).catch(() => undefined)
      if (receipt) {
        process.stdout.write(`Transaction nonce already consumed by expected hash: ${expectedHash}\n`)
        return
      }
    }
    throw error
  }
}

export async function waitForFinalized(
  rpc: Pick<RelayerRpc, "getTransactionReceipt" | "getBlock">,
  hash: Hex,
): Promise<FinalizedTransactionOutcome> {
  for (;;) {
    const receipt = await rpc.getTransactionReceipt({ hash }).catch(() => undefined)
    if (receipt) {
      if (receipt.status === "reverted") return receipt
      const finalized = await rpc.getBlock({ blockTag: "finalized" })
      if (finalized.number !== null && receipt.blockNumber <= finalized.number) {
        const canonical = await rpc.getBlock({ blockNumber: receipt.blockNumber })
        if (canonical.hash !== receipt.blockHash) throw new Error("Receipt block is not canonical")
        return receipt
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
    if (rawName in options) throw new Error(`Duplicate option: --${rawName}`)
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

const COMMAND_OPTIONS: Readonly<Record<string, readonly string[]>> = {
  help: ["help"],
  prepare: ["help", "action", "value"],
  "seal-operational-config": ["help", "parameters-file", "receipt-file"],
  status: ["help", "operation-id"],
  "recover-activation": ["help", "phase", "artifact-file", "authorization-file"],
  relay: ["help", "operation-id", "artifact-file", "authorization-file", "binding-file"],
  confirm: ["help", "operation-id", "transaction-hash", "hash", "artifact-file", "authorization-file", "binding-file", "receipt-file"],
  replace: ["help", "operation-id", "max-fee", "priority-fee", "artifact-file", "output-artifact-file"],
  "replace-activation": ["help", "operation-id", "max-fee", "priority-fee", "artifact-file", "authorization-file", "binding-file", "output-artifact-file", "output-binding-file"],
  run: ["help", "operation-id"],
  "refresh-attestation": ["help"],
  "prepare-schedule-activation": ["help", "artifact-file"],
  "prepare-execute-activation": ["help", "artifact-file"],
  "drain-emergency": ["help"],
}

export function validateCommandOptions(command: string, options: Options): void {
  const allowed = COMMAND_OPTIONS[command]
  if (!allowed) throw new Error(`Unknown command: ${command}`)
  for (const name of Object.keys(options)) {
    if (!allowed.includes(name)) throw new Error(`Unknown option for ${command}: --${name}`)
  }
  if (options["transaction-hash"] !== undefined && options.hash !== undefined) {
    throw new Error("--transaction-hash and --hash cannot be used together")
  }
}

export function unwrap<T, E>(result: { Ok: T } | { Err: E }): T {
  if ("Err" in result) throw new Error(canisterErrorMessage(result.Err))
  return result.Ok
}

export function canisterErrorMessage(error: unknown): string {
  const failureClass = signingFailureClass(error)
  if (!failureClass) {
    return `Canister rejected request: ${JSON.stringify(jsonValue(error))}`
  }
  const advice = failureClass === "InsufficientCycles"
    ? "Top up the canister and verify its reserve before an explicit retry."
    : ["InvalidPublicKey", "InvalidSignature", "RecoveryMismatch", "Storage", "ResponseDecode"]
        .includes(failureClass)
      ? "Do not retry; inspect the canister state and controller-only logs."
      : "Verify chain-key service health before an explicit retry."
  return `Threshold signing unavailable (${failureClass}). No automatic retry was performed. ${advice}`
}

function signingFailureClass(error: unknown): string | undefined {
  if (!error || typeof error !== "object" || !("SigningUnavailable" in error)) return undefined
  const unavailable = (error as { SigningUnavailable?: unknown }).SigningUnavailable
  if (!unavailable || typeof unavailable !== "object" || !("class" in unavailable)) return undefined
  const failureClass = (unavailable as { class?: unknown }).class
  if (!failureClass || typeof failureClass !== "object") return undefined
  return Object.keys(failureClass)[0]
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
  if (typeof value !== "string" || !/^0x[0-9a-fA-F]{64}$/.test(value)) throw new Error("transaction hash must be 32-byte hex")
  return value as Hex
}

export function confirmationHash(options: Options): Hex | undefined {
  return optionHash(options["transaction-hash"] ?? options.hash)
}

export function activationConfirmationHash(options: Options, expectedHash: Hex): Hex {
  const explicit = confirmationHash(options)
  if (explicit !== undefined && explicit.toLowerCase() !== expectedHash.toLowerCase()) {
    throw new Error("Explicit transaction hash differs from the fixed activation artifact")
  }
  return expectedHash
}

function bytesHex(value: Uint8Array | number[]): Hex {
  return `0x${Array.from(value, (byte) => Number(byte).toString(16).padStart(2, "0")).join("")}`
}

export function isAlreadyKnown(error: unknown): boolean {
  const message = error instanceof Error ? error.message : String(error)
  return /already known|known transaction/i.test(message)
}

export function isNonceTooLow(error: unknown): boolean {
  const message = error instanceof Error ? error.message : String(error)
  return /nonce too low|nonce has already been used/i.test(message)
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
  seal-operational-config --parameters-file FILE --receipt-file NEW_FILE
  prepare --action pause-deposits|pause-withdrawals|cancel-timelock|set-service-fee [--value N]
  status [--operation-id N]
  recover-activation --phase schedule|execute --authorization-file FILE --artifact-file FILE
  relay --artifact-file FILE [--authorization-file FILE --binding-file FILE] [--operation-id N]
  confirm --artifact-file FILE [--authorization-file FILE --binding-file FILE] --receipt-file NEW_FILE [--operation-id N] [--hash 0x...]
  run [--operation-id N]
  prepare-schedule-activation --artifact-file NEW_FILE
  prepare-execute-activation --artifact-file NEW_FILE
  refresh-attestation
  replace --artifact-file FILE --output-artifact-file NEW_FILE --operation-id N --max-fee N --priority-fee N
  replace-activation --artifact-file FILE --authorization-file FILE --binding-file FILE --output-artifact-file NEW_FILE --output-binding-file NEW_FILE --operation-id N --max-fee N --priority-fee N
  drain-emergency

Environment:
  BRIDGE_CANISTER_ID  Bridge Canister principal
  IC_IDENTITY_PEM    Required for confirm, run, prepare, replace, activation prepare, attestation, and emergency commands
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
