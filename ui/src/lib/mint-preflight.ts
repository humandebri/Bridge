import { getAccount, getChainId } from "wagmi/actions"
import { deploymentProfile } from "@/config/profile"
import { bridgeAbi } from "@/generated/abi/bridge.generated"
import { multiTokenBridgeAbi } from "@/generated/abi/multitokenbridge.generated"
import type { DepositView } from "@/generated/bridge.did"
import { createBasePublicClient, wagmiConfig } from "./evm/client"
import { validateMintAuthorization } from "./mint-authorization"
import type {
  ContractMintAuthorization,
  SharedContractMintAuthorization,
} from "./mint-authorization"
import {
  runtimeWriteBlocker,
  requireRuntimeWriteReady,
  validateAssetRuntime,
  validateRuntime,
  validateRuntimeHeartbeat,
  type RuntimeValidation,
} from "./runtime-validation"
import type { MintExecutionContext } from "./mint-execution"

export function mintWalletConnected(address: string, chainId: number): boolean {
  return (
    getAccount(wagmiConfig).address?.toLowerCase() === address.toLowerCase() &&
    getChainId(wagmiConfig) === chainId
  )
}

export async function prepareMint(
  record: DepositView,
  account: `0x${string}`,
  chainId: number,
  attestation: RuntimeValidation | undefined,
  context: MintExecutionContext,
) {
  const shared = "SharedMultiToken" in record.bridge_kind
  context.stage("checking-ic")
  let observation
  if (shared) {
    observation = await validateAssetRuntime(
      deploymentProfile,
      record.asset_id,
      "deposit",
      chainId,
      context.signal,
    )
    context.check()
    requireRuntimeWriteReady(observation)
  } else {
    if (runtimeWriteBlocker(attestation))
      attestation = await validateRuntime(deploymentProfile, chainId, context.signal)
    context.check()
    requireRuntimeWriteReady(attestation)
    observation = await validateRuntimeHeartbeat(deploymentProfile, chainId, context.signal)
    context.check()
    requireRuntimeWriteReady(observation)
    if (attestation.profileFingerprint !== observation.profileFingerprint)
      throw new Error("Runtime deployment profile changed.")
  }
  context.stage("checking-base")
  const client = createBasePublicClient(deploymentProfile, context.signal)
  // Start the elapsed-time allowance before reading the block, conservatively covering RPC latency.
  const observedAt = performance.now()
  const validated = await validateMintAuthorization(record, observation, client)
  context.check()
  context.stage("simulating")
  if (validated.shared) {
    await client.simulateContract({
      account,
      address: validated.bridgeAddress,
      abi: multiTokenBridgeAbi,
      functionName: "mintDepositWithAuthorization",
      args: [validated.authorization as SharedContractMintAuthorization, validated.signature],
    })
  } else {
    await client.simulateContract({
      account,
      address: validated.bridgeAddress,
      abi: bridgeAbi,
      functionName: "mintDepositWithAuthorization",
      args: [validated.authorization as ContractMintAuthorization, validated.signature],
    })
  }
  context.check()
  return { validated, observedAt }
}

export function checkMintDeadline(prepared: Awaited<ReturnType<typeof prepareMint>>): void {
  const estimated =
    prepared.validated.latestBlockTimestamp +
    BigInt(Math.ceil((performance.now() - prepared.observedAt) / 1_000))
  if (estimated >= prepared.validated.authorization.deadline)
    throw new Error("Mint authorization expired during preflight. No wallet request was sent.")
}
