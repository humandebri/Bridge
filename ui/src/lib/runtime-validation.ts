import type { Address } from "viem"
import { createBridgeActor } from "@/lib/ic/bridge"
import { createIndexActor } from "@/lib/ic/index"
import { createLedgerActor } from "@/lib/ic/ledger"
import { bridgeAbi } from "@/generated/abi/bridge.generated"
import { bsnsAbi } from "@/generated/abi/bsns.generated"
import { profileCompleteness, type DeploymentProfile } from "@/config/profile"
import { runtimeBytecodeSha256 } from "@/lib/runtime-bytecode-hash"
import { createBasePublicClient } from "@/lib/evm/client"
import type { BridgeStatus } from "@/generated/bridge.did"

const timelockDelayAbi = [{
  type: "function",
  name: "getMinDelay",
  stateMutability: "view",
  inputs: [],
  outputs: [{ name: "", type: "uint256" }],
}] as const

export interface RuntimeValidation { ready: boolean; blockers: string[]; checkedAt: number; profileFingerprint?: string }

export interface DeploymentAttestation extends FinalizedRuntimeObservation {
  profileFingerprint: string
}

export interface BridgeSnapshotObservation {
  serviceFee: bigint
  maxServiceFee: bigint
  perDepositLimit: bigint
  minted: bigint
  limit: bigint
  startedAt: bigint
  duration: bigint
  depositsPaused: boolean
  withdrawalsPaused: boolean
  bridgeSigner: Address
  mintAuthorizationEpoch: bigint
  blockTimestamp: bigint
}

export interface FinalizedRuntimeObservation extends RuntimeValidation {
  profileFingerprint?: string
  chainId?: number
  finalizedBlock?: bigint
  finalizedBlockHash?: `0x${string}`
  finalizedBlockTimestamp?: bigint
  snapshot?: BridgeSnapshotObservation
  status?: BridgeStatus
}

export const RUNTIME_VALIDATION_TTL_MS = 60_000
export const FINALIZED_HEAD_MAX_AGE_MS = 45 * 60_000
export const FINALIZED_HEAD_FUTURE_SKEW_MS = 60_000

export function runtimeProfileFingerprint(profile: DeploymentProfile): string {
  return [
    profile.environment,
    profile.label,
    profile.testOnly,
    profile.environmentMode,
    profile.activationTimelockDelaySeconds,
    profile.icHost,
    profile.baseRpcUrl,
    ...(profile.baseHistoryRpcUrls ?? []),
    profile.chainId,
    profile.bridgeCanisterId,
    profile.ledgerCanisterId,
    profile.indexCanisterId,
    profile.evmRpcCanisterId,
    profile.bridgeAddress,
    profile.bsnsAddress,
    profile.timelockAddress,
    profile.expected_bridge_signer,
    profile.deploymentInstanceId,
    profile.deploymentBlock,
    profile.bridgeRuntimeHash,
    profile.bsnsRuntimeHash,
    profile.rpcProviderUrlsSha256,
    profile.icToken.name,
    profile.icToken.symbol,
    profile.icToken.decimals,
    profile.baseToken.symbol,
    profile.baseToken.decimals,
  ].join(":").toLowerCase()
}

export function finalizedHeadTimestampBlocker(timestamp?: bigint, now = Date.now()): string | undefined {
  if (timestamp === undefined || timestamp <= 0n || !Number.isSafeInteger(now) || now < 0) {
    return "Finalized Base block timestamp is unavailable"
  }
  const observedAtMs = timestamp * 1_000n
  const nowMs = BigInt(now)
  if (observedAtMs > nowMs + BigInt(FINALIZED_HEAD_FUTURE_SKEW_MS)) {
    return "Finalized Base block timestamp is ahead of the browser clock"
  }
  if (nowMs - observedAtMs > BigInt(FINALIZED_HEAD_MAX_AGE_MS)) {
    return "Finalized Base head is stale"
  }
  return undefined
}

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

export async function refetchRuntimeWriteReady<T extends RuntimeValidation>(
  refetch: () => Promise<{ data?: T }>,
): Promise<T & { ready: true }> {
  const result = await refetch()
  requireRuntimeWriteReady(result.data)
  return result.data
}

export async function refetchRuntimeAttestedWriteReady<
  TAttestation extends RuntimeValidation,
  THeartbeat extends RuntimeValidation,
>(
  cachedAttestation: TAttestation | undefined,
  refetchAttestation: () => Promise<{ data?: TAttestation }>,
  refetchHeartbeat: () => Promise<{ data?: THeartbeat }>,
): Promise<THeartbeat & { ready: true }> {
  let attestation = cachedAttestation
  if (runtimeWriteBlocker(attestation)) {
    attestation = (await refetchAttestation()).data
  }
  requireRuntimeWriteReady(attestation)
  const heartbeat = await refetchRuntimeWriteReady(refetchHeartbeat)
  if (attestation.profileFingerprint
    && heartbeat.profileFingerprint
    && attestation.profileFingerprint !== heartbeat.profileFingerprint) {
    throw new Error("Runtime attestation and heartbeat refer to different deployment profiles")
  }
  return heartbeat
}

export async function validateRuntimeHeartbeat(profile: DeploymentProfile, connectedChainId?: number): Promise<FinalizedRuntimeObservation> {
  const profileFingerprint = runtimeProfileFingerprint(profile)
  const blockers = profileCompleteness(profile)
  if (connectedChainId !== undefined && connectedChainId !== profile.chainId) blockers.push(`Wallet is on chain ${connectedChainId}; expected ${profile.chainId}`)
  if (blockers.length > 0) return { ready: false, blockers, checkedAt: Date.now(), profileFingerprint }

  const bridgeAddress = profile.bridgeAddress as Address
  const client = createBasePublicClient(profile)
  const bridge = await createBridgeActor(profile.icHost, profile.bridgeCanisterId as string)
  const [status, localChainId, localFinalized] = await Promise.all([
    bridge.get_bridge_status(),
    client.getChainId(),
    client.getBlock({ blockTag: "finalized" }),
  ])
  if (status.withdrawal_fee_guard_active) blockers.push("Withdrawal fee guard is active; pause Base withdrawals and reconcile fees")
  if (localChainId !== profile.chainId) blockers.push(`Base RPC is on chain ${localChainId}; expected ${profile.chainId}`)
  if (localFinalized.number === null || localFinalized.hash === null) blockers.push("Finalized Base block number or hash is unavailable")
  const timestampBlocker = finalizedHeadTimestampBlocker(localFinalized.timestamp)
  if (timestampBlocker) blockers.push(timestampBlocker)
  if (blockers.length > 0) {
    return {
      ready: false,
      blockers,
      checkedAt: Date.now(),
      profileFingerprint,
      chainId: localChainId,
      finalizedBlock: localFinalized.number ?? undefined,
      finalizedBlockHash: localFinalized.hash ?? undefined,
      finalizedBlockTimestamp: localFinalized.timestamp,
      status,
    }
  }

  const bridgeSnapshot = await client.readContract({
    address: bridgeAddress,
    abi: bridgeAbi,
    functionName: "bridgeSnapshot",
    blockHash: localFinalized.hash,
    requireCanonical: true,
  })
  if (bridgeSnapshot.bridgeSigner.toLowerCase() !== profile.expected_bridge_signer?.toLowerCase()) {
    blockers.push("Bridge signer differs from the reviewed profile")
  }
  return {
    ready: blockers.length === 0,
    blockers,
    checkedAt: Date.now(),
    profileFingerprint,
    chainId: localChainId,
    finalizedBlock: localFinalized.number,
    finalizedBlockHash: localFinalized.hash,
    finalizedBlockTimestamp: localFinalized.timestamp,
    snapshot: bridgeSnapshotView(bridgeSnapshot),
    status,
  }
}

function bridgeSnapshotView(snapshot: {
  serviceFee: bigint
  maxServiceFee: bigint
  perDepositLimit: bigint
  mintedInWindow: bigint
  mintWindowLimit: bigint
  mintWindowStartedAt: bigint
  mintWindowDuration: bigint
  depositMintsPaused: boolean
  withdrawalsPaused: boolean
  bridgeSigner: Address
  mintAuthorizationEpoch: bigint
  blockTimestamp: bigint
}): BridgeSnapshotObservation {
  return {
    serviceFee: snapshot.serviceFee,
    maxServiceFee: snapshot.maxServiceFee,
    perDepositLimit: snapshot.perDepositLimit,
    minted: snapshot.mintedInWindow,
    limit: snapshot.mintWindowLimit,
    startedAt: snapshot.mintWindowStartedAt,
    duration: snapshot.mintWindowDuration,
    depositsPaused: snapshot.depositMintsPaused,
    withdrawalsPaused: snapshot.withdrawalsPaused,
    bridgeSigner: snapshot.bridgeSigner,
    mintAuthorizationEpoch: snapshot.mintAuthorizationEpoch,
    blockTimestamp: snapshot.blockTimestamp,
  }
}

export async function validateRuntime(profile: DeploymentProfile, connectedChainId?: number): Promise<DeploymentAttestation> {
  const profileFingerprint = runtimeProfileFingerprint(profile)
  const blockers = profileCompleteness(profile)
  if (connectedChainId !== undefined && connectedChainId !== profile.chainId) blockers.push(`Wallet is on chain ${connectedChainId}; expected ${profile.chainId}`)
  if (blockers.length > 0) return { ready: false, blockers, checkedAt: Date.now(), profileFingerprint }
  const expectedTimelockDelaySeconds = profile.activationTimelockDelaySeconds
  if (expectedTimelockDelaySeconds === null) {
    return { ready: false, blockers: ["Timelock delay is missing"], checkedAt: Date.now(), profileFingerprint }
  }

  const bridgeAddress = profile.bridgeAddress as Address
  const bsnsAddress = profile.bsnsAddress as Address
  const timelockAddress = profile.timelockAddress as Address
  const client = createBasePublicClient(profile)
  const [bridge, ledger] = await Promise.all([
    createBridgeActor(profile.icHost, profile.bridgeCanisterId as string),
    createLedgerActor(profile.icHost, profile.ledgerCanisterId as string),
  ])
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
  const timestampBlocker = finalizedHeadTimestampBlocker(localFinalized.timestamp)
  if (timestampBlocker) blockers.push(timestampBlocker)
  if (blockers.length > 0) return { ready: false, blockers, checkedAt: Date.now(), profileFingerprint }

  const finalizedHash = localFinalized.hash
  const [bridgeCode, bsnsCode, bridgeSnapshot, contractDomain, linkedBsns, bsnsSymbol, bsnsDecimals, timelockDelay] = await Promise.all([
    client.getCode({ address: bridgeAddress, blockHash: finalizedHash, requireCanonical: true }),
    client.getCode({ address: bsnsAddress, blockHash: finalizedHash, requireCanonical: true }),
    client.readContract({ address: bridgeAddress, abi: bridgeAbi, functionName: "bridgeSnapshot", blockHash: finalizedHash, requireCanonical: true }),
    client.readContract({ address: bridgeAddress, abi: bridgeAbi, functionName: "eip712Domain", blockHash: finalizedHash, requireCanonical: true }),
    client.readContract({ address: bridgeAddress, abi: bridgeAbi, functionName: "bsns", blockHash: finalizedHash, requireCanonical: true }),
    client.readContract({ address: bsnsAddress, abi: bsnsAbi, functionName: "symbol", blockHash: finalizedHash, requireCanonical: true }),
    client.readContract({ address: bsnsAddress, abi: bsnsAbi, functionName: "decimals", blockHash: finalizedHash, requireCanonical: true }),
    client.readContract({ address: timelockAddress, abi: timelockDelayAbi, functionName: "getMinDelay", blockHash: finalizedHash, requireCanonical: true }),
  ])
  if (!bridgeCode || runtimeBytecodeSha256(bridgeCode) !== profile.bridgeRuntimeHash) blockers.push("Bridge runtime bytecode does not match the reviewed profile")
  if (!bsnsCode || runtimeBytecodeSha256(bsnsCode) !== profile.bsnsRuntimeHash) blockers.push("bSNS runtime bytecode does not match the reviewed profile")
  if (String(linkedBsns).toLowerCase() !== bsnsAddress.toLowerCase()) blockers.push("Bridge points to a different bSNS contract")
  if (bsnsSymbol !== profile.baseToken.symbol || Number(bsnsDecimals) !== profile.baseToken.decimals) {
    blockers.push(`Base token metadata is not ${profile.baseToken.symbol}/${profile.baseToken.decimals}`)
  }
  if (timelockDelay !== BigInt(expectedTimelockDelaySeconds)) {
    blockers.push("Timelock delay differs from the reviewed profile")
  }
  const [, domainName, domainVersion, domainChainId, domainContract] = contractDomain
  if (domainName !== "KINIC Bridge"
    || domainVersion !== "1"
    || domainChainId !== BigInt(profile.chainId)
    || domainContract.toLowerCase() !== bridgeAddress.toLowerCase()) {
    blockers.push("Bridge EIP-712 domain differs from the reviewed profile")
  }

  blockers.push(...bridgeSignerBlockers(profile.expected_bridge_signer as Address, bridgeSnapshot.bridgeSigner, config.expected_bridge_signer))
  if (config.base_chain_id !== BigInt(profile.chainId)) blockers.push("Canister Base chain ID differs from the profile")
  const configuredBridge = `0x${Array.from(config.bridge_contract, (byte) => Number(byte).toString(16).padStart(2, "0")).join("")}`
  if (configuredBridge.toLowerCase() !== bridgeAddress.toLowerCase()) blockers.push("Canister Bridge contract differs from the profile")
  const configuredBridgeRuntime = bytesHex(config.expected_bridge_runtime_sha256, 32)
  if (configuredBridgeRuntime?.toLowerCase() !== profile.bridgeRuntimeHash?.toLowerCase()) {
    blockers.push("Canister expected Bridge runtime differs from the profile")
  }
  const configuredTimelock = `0x${Array.from(config.timelock_contract, (byte) => Number(byte).toString(16).padStart(2, "0")).join("")}`
  if (configuredTimelock.toLowerCase() !== timelockAddress.toLowerCase()) blockers.push("Canister Timelock contract differs from the profile")
  const configuredDeploymentInstance = `0x${Array.from(config.deployment_instance_id, (byte) => Number(byte).toString(16).padStart(2, "0")).join("")}`
  if (configuredDeploymentInstance.toLowerCase() !== profile.deploymentInstanceId?.toLowerCase()) blockers.push("Canister deployment instance differs from the profile")
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
  if (config.schema_version !== 31) blockers.push(`Unsupported canister schema ${config.schema_version}`)
  if (ledgerName !== profile.icToken.name || ledgerSymbol !== profile.icToken.symbol || ledgerDecimals !== profile.icToken.decimals) {
    blockers.push(`IC token metadata is not ${profile.icToken.name}/${profile.icToken.symbol}/${profile.icToken.decimals}`)
  }
  return {
    ready: blockers.length === 0,
    blockers,
    checkedAt: Date.now(),
    profileFingerprint,
    chainId: localChainId,
    finalizedBlock: localFinalized.number,
    finalizedBlockHash: localFinalized.hash,
    finalizedBlockTimestamp: localFinalized.timestamp,
    snapshot: bridgeSnapshotView(bridgeSnapshot),
    status,
  }
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
