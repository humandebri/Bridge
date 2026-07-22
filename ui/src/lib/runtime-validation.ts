import type { Address } from "viem"
import { createBridgeActor } from "@/lib/ic/bridge"
import { createIndexActor } from "@/lib/ic/index"
import { createLedgerActor } from "@/lib/ic/ledger"
import { bridgeAbi } from "@/generated/abi/bridge.generated"
import { bsnsAbi } from "@/generated/abi/bsns.generated"
import { profileCompleteness, type DeploymentProfile } from "@/config/profile"
import { runtimeBytecodeSha256 } from "@/lib/runtime-bytecode-hash"
import { createBasePublicClient } from "@/lib/evm/client"

export interface RuntimeValidation { ready: boolean; blockers: string[]; checkedAt: number }

export const RUNTIME_VALIDATION_TTL_MS = 60_000
export const CANISTER_FINALIZED_OBSERVATION_TTL_MS = 60_000
const CANISTER_CLOCK_SKEW_MS = 5_000

export function runtimeWriteBlocker(validation?: RuntimeValidation, now = Date.now()): string | undefined {
  if (!validation) return "Refresh to verify the reviewed deployment before continuing."
  if (!validation.ready) return validation.blockers[0] ?? "Runtime verification has not passed"
  if (!Number.isFinite(validation.checkedAt) || validation.checkedAt > now || now - validation.checkedAt > RUNTIME_VALIDATION_TTL_MS) {
    return "Runtime verification expired. Refresh before continuing."
  }
  return undefined
}

export function requireRuntimeWriteReady(validation?: RuntimeValidation, now = Date.now()): asserts validation is RuntimeValidation & { ready: true } {
  const blocker = runtimeWriteBlocker(validation, now)
  if (blocker) throw new Error(blocker)
}

export async function refetchRuntimeWriteReady(refetch: () => Promise<{ data?: RuntimeValidation }>): Promise<RuntimeValidation & { ready: true }> {
  const result = await refetch()
  requireRuntimeWriteReady(result.data)
  return result.data
}

export async function validateRuntime(profile: DeploymentProfile, connectedChainId?: number): Promise<RuntimeValidation> {
  const blockers = profileCompleteness(profile)
  if (connectedChainId !== undefined && connectedChainId !== profile.chainId) blockers.push(`Wallet is on chain ${connectedChainId}; expected ${profile.chainId}`)
  if (blockers.length > 0) return { ready: false, blockers, checkedAt: Date.now() }

  const bridgeAddress = profile.bridgeAddress as Address
  const bsnsAddress = profile.bsnsAddress as Address
  const client = createBasePublicClient(profile)
  const [bridge, ledger] = await Promise.all([
    createBridgeActor(profile.icHost, profile.bridgeCanisterId as string),
    createLedgerActor(profile.icHost, profile.ledgerCanisterId as string),
  ])
  const refreshedObservation = await bridge.refresh_base_observation()
  if ("Err" in refreshedObservation) {
    blockers.push(`Canister Base observation refresh failed: ${Object.keys(refreshedObservation.Err)[0] ?? "unknown error"}`)
  }
  const [config, status, ledgerName, ledgerSymbol, ledgerDecimals, localChainId, localFinalized] = await Promise.all([
    bridge.get_public_config(),
    bridge.get_bridge_status(),
    ledger.icrc1_name(),
    ledger.icrc1_symbol(),
    ledger.icrc1_decimals(),
    client.getChainId(),
    client.getBlock({ blockTag: "finalized" }),
  ])
  if (status.withdrawal_fee_guard_active) blockers.push("Withdrawal fee guard is active; pause Base withdrawals and reconcile fees")
  if (localChainId !== profile.chainId) blockers.push(`Base RPC is on chain ${localChainId}; expected ${profile.chainId}`)
  if (localFinalized.number === null || localFinalized.hash === null) blockers.push("Finalized Base block number or hash is unavailable")
  blockers.push(...canisterFinalizedObservationBlockers(profile, localFinalized.number, status, Date.now()))
  if (blockers.length > 0) return { ready: false, blockers, checkedAt: Date.now() }

  const observedHash = bytesHex(status.last_finalized_base_block_hash, 32)!
  const observedBlock = await client.getBlock({ blockHash: observedHash })
  if (observedBlock.number !== status.last_finalized_base_block || observedBlock.hash?.toLowerCase() !== observedHash.toLowerCase()) {
    blockers.push("Canister finalized block hash is not canonical on the configured Base RPC")
    return { ready: false, blockers, checkedAt: Date.now() }
  }
  const [bridgeCode, bsnsCode, bridgeSnapshot, linkedBsns, bsnsSymbol, bsnsDecimals] = await Promise.all([
    client.getCode({ address: bridgeAddress, blockHash: observedHash, requireCanonical: true }),
    client.getCode({ address: bsnsAddress, blockHash: observedHash, requireCanonical: true }),
    client.readContract({ address: bridgeAddress, abi: bridgeAbi, functionName: "bridgeSnapshot", blockHash: observedHash, requireCanonical: true }),
    client.readContract({ address: bridgeAddress, abi: bridgeAbi, functionName: "bsns", blockHash: observedHash, requireCanonical: true }),
    client.readContract({ address: bsnsAddress, abi: bsnsAbi, functionName: "symbol", blockHash: observedHash, requireCanonical: true }),
    client.readContract({ address: bsnsAddress, abi: bsnsAbi, functionName: "decimals", blockHash: observedHash, requireCanonical: true }),
  ])
  if (!bridgeCode || runtimeBytecodeSha256(bridgeCode) !== profile.bridgeRuntimeHash) blockers.push("Bridge runtime bytecode does not match the reviewed profile")
  if (!bsnsCode || runtimeBytecodeSha256(bsnsCode) !== profile.bsnsRuntimeHash) blockers.push("bSNS runtime bytecode does not match the reviewed profile")
  if (String(linkedBsns).toLowerCase() !== bsnsAddress.toLowerCase()) blockers.push("Bridge points to a different bSNS contract")
  if (bsnsSymbol !== profile.baseToken.symbol || Number(bsnsDecimals) !== profile.baseToken.decimals) {
    blockers.push(`Base token metadata is not ${profile.baseToken.symbol}/${profile.baseToken.decimals}`)
  }

  blockers.push(...bridgeSignerBlockers(profile.expected_bridge_signer as Address, bridgeSnapshot.bridgeSigner, config.expected_bridge_signer))
  if (config.base_chain_id !== BigInt(profile.chainId)) blockers.push("Canister Base chain ID differs from the profile")
  const configuredBridge = `0x${Array.from(config.bridge_contract, (byte) => Number(byte).toString(16).padStart(2, "0")).join("")}`
  if (configuredBridge.toLowerCase() !== bridgeAddress.toLowerCase()) blockers.push("Canister Bridge contract differs from the profile")
  if (config.ledger_canister_id.toText() !== profile.ledgerCanisterId) blockers.push("Canister ledger differs from the profile")
  if (config.index_canister_id.toText() !== profile.indexCanisterId) blockers.push("Canister index differs from the profile")
  if (config.evm_rpc_canister_id.toText() !== profile.evmRpcCanisterId) blockers.push("Canister EVM RPC ID differs from the profile")
  const rpcUrlsDigest = `0x${Array.from(config.rpc_provider_urls_sha256, (byte) => Number(byte).toString(16).padStart(2, "0")).join("")}`
  if (rpcUrlsDigest.toLowerCase() !== profile.rpcProviderUrlsSha256?.toLowerCase()) blockers.push("Canister RPC provider URLs differ from the profile")
  try {
    const index = await createIndexActor(profile.icHost, profile.indexCanisterId as string)
    const indexLedgerId = await index.ledger_id()
    if (indexLedgerId.toText() !== profile.ledgerCanisterId) blockers.push("Index ledger differs from the profile")
  } catch {
    blockers.push("Index ledger binding is unavailable")
  }
  if (config.schema_version !== 17) blockers.push(`Unsupported canister schema ${config.schema_version}`)
  if (ledgerName !== profile.icToken.name || ledgerSymbol !== profile.icToken.symbol || ledgerDecimals !== profile.icToken.decimals) {
    blockers.push(`IC token metadata is not ${profile.icToken.name}/${profile.icToken.symbol}/${profile.icToken.decimals}`)
  }
  return { ready: blockers.length === 0, blockers, checkedAt: Date.now() }
}

interface CanisterFinalizedObservation {
  base_chain_id_matches_config: boolean
  last_finalized_base_block: bigint
  last_finalized_base_block_hash: Uint8Array | number[]
  last_finalized_observation_ns: bigint
  observed_base_chain_id: [] | [bigint]
  observed_bridge_signer: Uint8Array | number[]
  observed_bridge_runtime_sha256: Uint8Array | number[]
}

export function canisterFinalizedObservationBlockers(
  profile: DeploymentProfile,
  localFinalizedBlock: bigint,
  observation: CanisterFinalizedObservation,
  now = Date.now(),
): string[] {
  const blockers: string[] = []
  const observedChainId = observation.observed_base_chain_id[0]
  if (observedChainId === undefined) blockers.push("Canister Base chain observation is unavailable")
  else if (observedChainId !== BigInt(profile.chainId) || !observation.base_chain_id_matches_config) blockers.push("Canister observed a different Base chain")

  const observedHash = bytesHex(observation.last_finalized_base_block_hash, 32)
  if (observation.last_finalized_base_block === 0n || observedHash === undefined) blockers.push("Canister finalized block observation is unavailable")
  else if (observation.last_finalized_base_block > localFinalizedBlock) blockers.push("Canister finalized block is ahead of the configured Base RPC finalized head")

  const observedAtMs = Number(observation.last_finalized_observation_ns / 1_000_000n)
  if (
    observation.last_finalized_observation_ns === 0n
    || !Number.isSafeInteger(observedAtMs)
    || observedAtMs > now + CANISTER_CLOCK_SKEW_MS
    || now - observedAtMs > CANISTER_FINALIZED_OBSERVATION_TTL_MS
  ) blockers.push("Canister finalized block observation is unavailable or stale")

  const signer = bytesHex(observation.observed_bridge_signer, 20)
  if (signer === undefined) blockers.push("Canister observed Bridge signer is unavailable")
  else if (signer.toLowerCase() !== profile.expected_bridge_signer?.toLowerCase()) blockers.push("Canister observed Bridge signer differs from the reviewed profile")

  const runtimeHash = bytesHex(observation.observed_bridge_runtime_sha256, 32)
  if (runtimeHash === undefined) blockers.push("Canister observed Bridge runtime hash is unavailable")
  else if (runtimeHash.toLowerCase() !== profile.bridgeRuntimeHash?.toLowerCase()) blockers.push("Canister observed Bridge runtime bytecode differs from the reviewed profile")
  return blockers
}

export function bytesHex(bytes: Uint8Array | number[], expectedLength: number): `0x${string}` | undefined {
  if (bytes.length !== expectedLength || Array.from(bytes).some((byte) => !Number.isInteger(byte) || byte < 0 || byte > 255)) return undefined
  return `0x${Array.from(bytes, (byte) => Number(byte).toString(16).padStart(2, "0")).join("")}`
}

export function bridgeSignerBlockers(profileSigner: Address, contractSigner: Address, canisterSigner?: Uint8Array | number[]): string[] {
  if (!canisterSigner || canisterSigner.length !== 20) return ["Canister expected Bridge signer is unavailable"]
  const canisterAddress = `0x${Array.from(canisterSigner, (byte) => Number(byte).toString(16).padStart(2, "0")).join("")}`
  const expected = profileSigner.toLowerCase()
  const blockers: string[] = []
  if (contractSigner.toLowerCase() !== expected) blockers.push("Bridge signer differs from the reviewed profile")
  if (canisterAddress.toLowerCase() !== expected) blockers.push("Canister expected Bridge signer differs from the reviewed profile")
  return blockers
}
