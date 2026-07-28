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
import { withBrowserLock } from "@/lib/browser-lock"
import { basePublicClient } from "@/lib/evm/client"

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

export function formatMintAuthorizationTtl(seconds: bigint): string {
  if (seconds > 0n && seconds % 3_600n === 0n) return `${seconds / 3_600n}時間`
  if (seconds > 0n && seconds % 60n === 0n) return `${seconds / 60n}分`
  return `${seconds}秒`
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

export async function validateMintAuthorization(record: DepositView): Promise<ValidatedMintAuthorization> {
  const view = record.mint_authorization[0]
  const signatureBytes = view?.signature[0]
  if (!view || !signatureBytes || !("AuthorizationAvailable" in record.state)) {
    throw new Error("Mint authorization is not available")
  }
  assertCanonicalDeposit(record, view)

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
  const [snapshot, processed, contractDomain, latestBlock] = await Promise.all([
    basePublicClient.readContract({
      address: configuredContract,
      abi: bridgeAbi,
      functionName: "bridgeSnapshot",
    }),
    basePublicClient.readContract({
      address: configuredContract,
      abi: bridgeAbi,
      functionName: "isDepositProcessed",
      args: [authorization.depositId],
    }),
    basePublicClient.readContract({
      address: configuredContract,
      abi: bridgeAbi,
      functionName: "eip712Domain",
    }),
    basePublicClient.getBlock({ blockTag: "latest" }),
  ])

  const [, contractName, contractVersion, contractChainId, verifyingContract] = contractDomain
  if (snapshot.depositMintsPaused
    || snapshot.mintAuthorizationEpoch !== authorization.authorizationEpoch
    || processed
    || contractName !== domain.name
    || contractVersion !== domain.version
    || contractChainId !== domain.chainId
    || verifyingContract.toLowerCase() !== domain.verifyingContract.toLowerCase()
    || recovered.toLowerCase() !== snapshot.bridgeSigner.toLowerCase()
    || latestBlock.timestamp > authorization.deadline) {
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

function pendingKey(depositId: Hex): string {
  return [
    "kinic.bridge.pending-mint.v1",
    deploymentProfile.chainId,
    String(deploymentProfile.bridgeAddress).toLowerCase(),
    deploymentProfile.bridgeCanisterId ?? "",
    depositId.toLowerCase(),
  ].join(":")
}

const sessionPendingMints = new Map<string, Hex>()
const removedSessionPendingMints = new Set<string>()

export async function savePendingMint(depositId: Hex, transactionHash: Hex): Promise<void> {
  const key = pendingKey(depositId)
  removedSessionPendingMints.delete(key)
  sessionPendingMints.set(key, transactionHash)
  await withBrowserLock(`kinic-storage:${key}`, () => {
    try {
      window.localStorage.setItem(key, transactionHash)
      sessionPendingMints.delete(key)
    } catch { /* The session copy still preserves recovery after a successful wallet broadcast. */ }
  })
}

export function readPendingMint(depositId: Hex): Hex | undefined {
  if (typeof window === "undefined") return undefined
  const key = pendingKey(depositId)
  if (removedSessionPendingMints.has(key)) return undefined
  const sessionValue = sessionPendingMints.get(key)
  if (sessionValue) return sessionValue
  try {
    const value = window.localStorage.getItem(key)
    return value && /^0x[0-9a-fA-F]{64}$/.test(value) ? value as Hex : undefined
  } catch {
    return undefined
  }
}

export async function removePendingMint(depositId: Hex): Promise<void> {
  const key = pendingKey(depositId)
  sessionPendingMints.delete(key)
  removedSessionPendingMints.add(key)
  await withBrowserLock(`kinic-storage:${key}`, () => {
    try {
      window.localStorage.removeItem(key)
      removedSessionPendingMints.delete(key)
    } catch { /* The session tombstone prevents a reverted transaction from reappearing. */ }
  })
}
