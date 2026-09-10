import { getAccount, getChainId } from "wagmi/actions"
import { deploymentProfile } from "@/config/profile"
import { bridgeAbi } from "@/generated/abi/bridge.generated"
import type { DepositView } from "@/generated/bridge.did"
import { createBasePublicClient, wagmiConfig } from "./evm/client"
import { validateMintAuthorization } from "./mint-authorization"
import {
  runtimeWriteBlocker,
  requireRuntimeWriteReady,
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
  context.stage("checking-ic")
  if (runtimeWriteBlocker(attestation))
    attestation = await validateRuntime(deploymentProfile, chainId, context.signal)
  context.check()
  requireRuntimeWriteReady(attestation)
  const observation = await validateRuntimeHeartbeat(deploymentProfile, chainId, context.signal)
  context.check()
  requireRuntimeWriteReady(observation)
  if (attestation.profileFingerprint !== observation.profileFingerprint)
    throw new Error("Runtime deployment profile changed.")
  context.stage("checking-base")
  const client = createBasePublicClient(deploymentProfile, context.signal)
  // Start the elapsed-time allowance before reading the block, conservatively covering RPC latency.
  const observedAt = performance.now()
  const validated = await validateMintAuthorization(record, observation, client)
  context.check()
  context.stage("simulating")
  await client.simulateContract({
    account,
    address: deploymentProfile.bridgeAddress as `0x${string}`,
    abi: bridgeAbi,
    functionName: "mintDepositWithAuthorization",
    args: [validated.authorization, validated.signature],
  })
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
