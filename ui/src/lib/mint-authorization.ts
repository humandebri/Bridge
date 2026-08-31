import {
  bytesToHex,
  hashTypedData,
  recoverAddress,
  type Address,
  type Hex,
} from "viem"
import type { DepositView, MintAuthorizationView } from "@/generated/bridge.did"
import { bridgeAbi } from "@/generated/abi/bridge.generated"
import { deploymentProfile } from "@/config/profile"
import { basePublicClient } from "@/lib/evm/client"
import type { FinalizedRuntimeObservation } from "@/lib/runtime-validation"
import { hasCanonicalMintAuthorizationDeadline, mintAuthorizationWindow } from "@/lib/mint-authorization-window"

export const mintAuthorizationTypes = {
  MintAuthorization: [
    { name: "depositId", type: "bytes32" },
    { name: "recipient", type: "address" },
    { name: "grossAmount", type: "uint256" },
    { name: "maxServiceFee", type: "uint256" },
    { name: "chargedServiceFee", type: "uint256" },
    { name: "deadline", type: "uint256" },
    { name: "authorizationEpoch", type: "uint256" },
  ],
} as const

export interface ContractMintAuthorization {
  depositId: Hex
  recipient: Address
  grossAmount: bigint
  maxServiceFee: bigint
  chargedServiceFee: bigint
  deadline: bigint
  authorizationEpoch: bigint
}

export interface ValidatedMintAuthorization {
  authorization: ContractMintAuthorization
  signature: Hex
  digest: Hex
  recipient: Address
  signer: Address
  latestBlockTimestamp: bigint
}

export function assertMintAuthorizationContractHorizon(
  deadline: bigint,
  latestBaseTimestamp: bigint,
): void {
  if (deadline > latestBaseTimestamp + 900n) {
    throw new Error("Mint authorization exceeds the Base contract deadline horizon")
  }
}

function fixedHex(bytes: Uint8Array | number[], length: number, label: string): Hex {
  if (bytes.length !== length) throw new Error(`${label} has an invalid length`)
  return bytesToHex(Uint8Array.from(bytes))
}

function address(bytes: Uint8Array | number[], label: string): Address {
  return fixedHex(bytes, 20, label)
}

export function contractAuthorization(view: MintAuthorizationView): ContractMintAuthorization {
  return {
    depositId: fixedHex(view.deposit_id, 32, "Deposit ID"),
    recipient: address(view.recipient, "Mint recipient"),
    grossAmount: view.gross_amount,
    maxServiceFee: view.max_service_fee,
    chargedServiceFee: view.charged_service_fee,
    deadline: view.deadline,
    authorizationEpoch: view.authorization_epoch,
  }
}

function assertCanonicalDeposit(record: DepositView, view: MintAuthorizationView): void {
  const quote = record.quote[0]
  if (!quote
    || fixedHex(record.deposit_id, 32, "Canonical deposit ID") !== fixedHex(view.deposit_id, 32, "Authorization deposit ID")
    || address(record.base_recipient, "Canonical recipient").toLowerCase() !== address(view.recipient, "Authorization recipient").toLowerCase()
    || record.gross_amount !== view.gross_amount
    || record.max_service_fee !== view.max_service_fee
    || quote.service_fee !== view.charged_service_fee) {
    throw new Error("Mint authorization does not match the canonical deposit")
  }
}

export async function validateMintAuthorization(
  record: DepositView,
  runtimeObservation: FinalizedRuntimeObservation,
): Promise<ValidatedMintAuthorization> {
  const view = record.mint_authorization[0]
  const signatureBytes = view?.signature[0]
  if (!view || !signatureBytes || !("AuthorizationAvailable" in record.state)) {
    throw new Error("Mint authorization is not available")
  }
  assertCanonicalDeposit(record, view)
  if (!hasCanonicalMintAuthorizationDeadline(view.issued_at_timestamp, view.deadline)) {
    throw new Error("Mint authorization issue time and deadline are inconsistent")
  }

  const configuredContract = deploymentProfile.bridgeAddress as Address
  const domainContract = address(view.verifying_contract, "Authorization contract")
  if (view.domain_name !== "KINIC Bridge"
    || view.domain_version !== "1"
    || view.chain_id !== BigInt(deploymentProfile.chainId)
    || domainContract.toLowerCase() !== configuredContract.toLowerCase()) {
    throw new Error("Mint authorization domain does not match this deployment")
  }

  const authorization = contractAuthorization(view)
  const domain = {
    name: view.domain_name,
    version: view.domain_version,
    chainId: view.chain_id,
    verifyingContract: domainContract,
  } as const
  const digest = hashTypedData({
    domain,
    types: mintAuthorizationTypes,
    primaryType: "MintAuthorization",
    message: authorization,
  })
  if (digest.toLowerCase() !== fixedHex(view.digest, 32, "Authorization digest").toLowerCase()) {
    throw new Error("Mint authorization digest mismatch")
  }

  const signature = fixedHex(signatureBytes, 65, "Mint signature")
  const recovered = await recoverAddress({ hash: digest, signature })
  const snapshot = runtimeObservation.snapshot
  if (!runtimeObservation.ready || !snapshot) {
    throw new Error("Finalized Base runtime observation is unavailable")
  }
  let processed: boolean
  let latestBlock: Awaited<ReturnType<typeof basePublicClient.getBlock>>
  try {
    [processed, latestBlock] = await Promise.all([
      basePublicClient.readContract({
        address: configuredContract,
        abi: bridgeAbi,
        functionName: "isDepositProcessed",
        args: [authorization.depositId],
      }),
      basePublicClient.getBlock({ blockTag: "latest" }),
    ])
  } catch {
    throw new Error("Latest Base time or processed state could not be refreshed. No Base transaction was sent.")
  }

  if (processed) {
    throw new Error("This Deposit ID is already processed on Base. Do not submit another mint; refresh History for finalized status.")
  }
  if (!mintAuthorizationWindow(authorization.deadline, latestBlock.timestamp).hasMinimumRemainingTime) {
    throw new Error("Mint authorization has less than five minutes remaining. No Base transaction was sent.")
  }
  if (snapshot.depositsPaused
    || snapshot.mintAuthorizationEpoch !== authorization.authorizationEpoch
    || recovered.toLowerCase() !== snapshot.bridgeSigner.toLowerCase()) {
    throw new Error("Mint authorization is no longer valid on Base")
  }

  return {
    authorization,
    signature,
    digest,
    recipient: authorization.recipient,
    signer: recovered,
    latestBlockTimestamp: latestBlock.timestamp,
  }
}
