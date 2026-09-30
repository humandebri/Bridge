import { bytesToHex, hashTypedData, recoverAddress, type Address, type Hex } from "viem"
import type { DepositView, MintAuthorizationView } from "@/generated/bridge.did"
import { bridgeAbi } from "@/generated/abi/bridge.generated"
import { multiTokenBridgeAbi } from "@/generated/abi/multitokenbridge.generated"
import { deploymentProfile } from "@/config/profile"
import { basePublicClient } from "@/lib/evm/client"
import { createBridgeActor } from "@/lib/ic/bridge"
import { runtimeBytecodeSha256 } from "@/lib/runtime-bytecode-hash"
import type { FinalizedRuntimeObservation } from "@/lib/runtime-validation"
import {
  hasCanonicalMintAuthorizationDeadline,
  mintAuthorizationWindow,
} from "@/lib/mint-authorization-window"

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

export const sharedMintAuthorizationTypes = {
  MintAuthorization: [
    { name: "assetId", type: "bytes32" },
    { name: "depositId", type: "bytes32" },
    { name: "recipient", type: "address" },
    { name: "grossAmount", type: "uint256" },
    { name: "maxServiceFee", type: "uint256" },
    { name: "chargedServiceFee", type: "uint256" },
    { name: "deadline", type: "uint256" },
    { name: "globalEpoch", type: "uint256" },
    { name: "assetEpoch", type: "uint256" },
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
  authorization: ContractMintAuthorization | SharedContractMintAuthorization
  shared: boolean
  bridgeAddress: Address
  signature: Hex
  digest: Hex
  recipient: Address
  signer: Address
  latestBlockTimestamp: bigint
}

export interface SharedContractMintAuthorization {
  assetId: Hex
  depositId: Hex
  recipient: Address
  grossAmount: bigint
  maxServiceFee: bigint
  chargedServiceFee: bigint
  deadline: bigint
  globalEpoch: bigint
  assetEpoch: bigint
}

function assertMintAuthorizationContractHorizon(
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
  if (
    !quote ||
    fixedHex(record.deposit_id, 32, "Canonical deposit ID") !==
      fixedHex(view.deposit_id, 32, "Authorization deposit ID") ||
    address(record.base_recipient, "Canonical recipient").toLowerCase() !==
      address(view.recipient, "Authorization recipient").toLowerCase() ||
    record.gross_amount !== view.gross_amount ||
    record.max_service_fee !== view.max_service_fee ||
    quote.service_fee !== view.charged_service_fee
  ) {
    throw new Error("Mint authorization does not match the canonical deposit")
  }
}

export async function validateMintAuthorization(
  record: DepositView,
  runtimeObservation: FinalizedRuntimeObservation,
  client = basePublicClient,
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

  const shared = "SharedMultiToken" in record.bridge_kind
  const registeredAsset = shared
    ? await (async () => {
        const actor = await createBridgeActor(
          deploymentProfile.icHost,
          deploymentProfile.bridgeCanisterId as string,
        )
        const result = await actor.get_asset(Uint8Array.from(record.asset_id))
        if ("Err" in result || !result.Ok[0]) throw new Error("Registered asset is unavailable")
        return result.Ok[0]
      })()
    : undefined
  const configuredContract = shared
    ? address(registeredAsset!.bridge_contract, "Registered shared Bridge")
    : (deploymentProfile.bridgeAddress as Address)
  const domainContract = address(view.verifying_contract, "Authorization contract")
  if (
    view.domain_name !== (shared ? "IC Base Multi-Token Bridge" : "KINIC Bridge") ||
    view.domain_version !== "1" ||
    view.chain_id !== BigInt(deploymentProfile.chainId) ||
    domainContract.toLowerCase() !== configuredContract.toLowerCase()
  ) {
    throw new Error("Mint authorization domain does not match this deployment")
  }

  const legacyAuthorization = contractAuthorization(view)
  const assetEpoch = record.asset_authorization_epoch[0]
  if (shared && assetEpoch === undefined) throw new Error("Asset authorization epoch is missing")
  const authorization: ContractMintAuthorization | SharedContractMintAuthorization = shared
    ? {
        assetId: fixedHex(record.asset_id, 32, "Asset ID"),
        depositId: legacyAuthorization.depositId,
        recipient: legacyAuthorization.recipient,
        grossAmount: legacyAuthorization.grossAmount,
        maxServiceFee: legacyAuthorization.maxServiceFee,
        chargedServiceFee: legacyAuthorization.chargedServiceFee,
        deadline: legacyAuthorization.deadline,
        globalEpoch: legacyAuthorization.authorizationEpoch,
        assetEpoch: BigInt(assetEpoch!),
      }
    : legacyAuthorization
  const domain = {
    name: view.domain_name,
    version: view.domain_version,
    chainId: view.chain_id,
    verifyingContract: domainContract,
  } as const
  const digest = shared
    ? hashTypedData({
        domain,
        types: sharedMintAuthorizationTypes,
        primaryType: "MintAuthorization",
        message: authorization as SharedContractMintAuthorization,
      })
    : hashTypedData({
        domain,
        types: mintAuthorizationTypes,
        primaryType: "MintAuthorization",
        message: authorization as ContractMintAuthorization,
      })
  if (digest.toLowerCase() !== fixedHex(view.digest, 32, "Authorization digest").toLowerCase()) {
    throw new Error("Mint authorization digest mismatch")
  }

  const signature = fixedHex(signatureBytes, 65, "Mint signature")
  const recovered = await recoverAddress({ hash: digest, signature })
  const snapshot = runtimeObservation.snapshot
  if (!shared && (!runtimeObservation.ready || !snapshot)) {
    throw new Error("Finalized Base runtime observation is unavailable")
  }
  let processed: boolean
  let latestBlock: Awaited<ReturnType<typeof basePublicClient.getBlock>>
  try {
    ;[processed, latestBlock] = await Promise.all([
      shared
        ? client.readContract({
            address: configuredContract,
            abi: multiTokenBridgeAbi,
            functionName: "isDepositProcessed",
            args: [
              (authorization as SharedContractMintAuthorization).assetId,
              authorization.depositId,
            ],
          })
        : client.readContract({
            address: configuredContract,
            abi: bridgeAbi,
            functionName: "isDepositProcessed",
            args: [authorization.depositId],
          }),
      client.getBlock({ blockTag: "latest" }),
    ])
  } catch {
    throw new Error(
      "Latest Base time or processed state could not be refreshed. No Base transaction was sent.",
    )
  }

  if (processed) {
    throw new Error(
      "This Deposit ID is already processed on Base. Do not submit another mint; refresh History for finalized status.",
    )
  }
  assertMintAuthorizationContractHorizon(authorization.deadline, latestBlock.timestamp)
  if (!mintAuthorizationWindow(authorization.deadline, latestBlock.timestamp).isUnexpired) {
    throw new Error("Mint authorization has expired. No Base transaction was sent.")
  }
  if (shared) {
    const sharedAuthorization = authorization as SharedContractMintAuthorization
    const tokenAddress = address(registeredAsset!.token_contract, "Registered token")
    const [assetSnapshot, globalPaused, globalEpoch, signer, token, bridgeCode, tokenCode] =
      await Promise.all([
        client.readContract({
          address: configuredContract,
          abi: multiTokenBridgeAbi,
          functionName: "assetSnapshot",
          args: [sharedAuthorization.assetId],
        }),
        client.readContract({
          address: configuredContract,
          abi: multiTokenBridgeAbi,
          functionName: "globalDepositMintsPaused",
        }),
        client.readContract({
          address: configuredContract,
          abi: multiTokenBridgeAbi,
          functionName: "globalEpoch",
        }),
        client.readContract({
          address: configuredContract,
          abi: multiTokenBridgeAbi,
          functionName: "bridgeSigner",
        }),
        client.readContract({
          address: configuredContract,
          abi: multiTokenBridgeAbi,
          functionName: "tokenForAsset",
          args: [sharedAuthorization.assetId],
        }),
        client.getBytecode({ address: configuredContract }),
        client.getBytecode({ address: tokenAddress }),
      ])
    if (
      !bridgeCode ||
      !tokenCode ||
      runtimeBytecodeSha256(bridgeCode).toLowerCase() !==
        fixedHex(
          registeredAsset!.expected_bridge_runtime_sha256,
          32,
          "Bridge runtime hash",
        ).toLowerCase() ||
      runtimeBytecodeSha256(tokenCode).toLowerCase() !==
        fixedHex(
          registeredAsset!.expected_token_runtime_sha256,
          32,
          "Token runtime hash",
        ).toLowerCase() ||
      token.toLowerCase() !== tokenAddress.toLowerCase() ||
      globalPaused ||
      assetSnapshot.depositMintsPaused ||
      assetSnapshot.minServiceFee === 0n ||
      assetSnapshot.serviceFee < assetSnapshot.minServiceFee ||
      assetSnapshot.maxServiceFee < assetSnapshot.minServiceFee ||
      sharedAuthorization.chargedServiceFee > sharedAuthorization.maxServiceFee ||
      sharedAuthorization.chargedServiceFee > assetSnapshot.maxServiceFee ||
      globalEpoch !== sharedAuthorization.globalEpoch ||
      assetSnapshot.assetEpoch !== sharedAuthorization.assetEpoch ||
      recovered.toLowerCase() !== signer.toLowerCase()
    ) {
      throw new Error("Mint authorization is no longer valid on the shared Bridge")
    }
  } else if (
    snapshot!.depositsPaused ||
    snapshot!.mintAuthorizationEpoch !== legacyAuthorization.authorizationEpoch ||
    recovered.toLowerCase() !== snapshot!.bridgeSigner.toLowerCase()
  ) {
    throw new Error("Mint authorization is no longer valid on Base")
  }

  return {
    authorization,
    shared,
    bridgeAddress: configuredContract,
    signature,
    digest,
    recipient: authorization.recipient,
    signer: recovered,
    latestBlockTimestamp: latestBlock.timestamp,
  }
}
