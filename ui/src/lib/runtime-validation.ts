import { createPublicClient, http, keccak256, type Address } from "viem"
import { defineChain } from "viem"
import { createBridgeActor } from "@/lib/ic/bridge"
import { createLedgerActor } from "@/lib/ic/ledger"
import { bridgeAbi } from "@/generated/abi/bridge.generated"
import { bsnsAbi } from "@/generated/abi/bsns.generated"
import { profileCompleteness, type DeploymentProfile } from "@/config/profile"

export interface RuntimeValidation { ready: boolean; blockers: string[]; checkedAt: number }

export async function validateRuntime(profile: DeploymentProfile, connectedChainId?: number): Promise<RuntimeValidation> {
  const blockers = profileCompleteness(profile)
  if (connectedChainId !== undefined && connectedChainId !== profile.chainId) blockers.push(`Wallet is on chain ${connectedChainId}; expected ${profile.chainId}`)
  if (blockers.length > 0) return { ready: false, blockers, checkedAt: Date.now() }

  const bridgeAddress = profile.bridgeAddress as Address
  const bsnsAddress = profile.bsnsAddress as Address
  const client = createPublicClient({
    chain: defineChain({ id: profile.chainId, name: profile.label, nativeCurrency: { name: "Ether", symbol: "ETH", decimals: 18 }, rpcUrls: { default: { http: [profile.baseRpcUrl] } } }),
    transport: http(profile.baseRpcUrl),
  })
  const [bridgeCode, bsnsCode, linkedBsns, bsnsSymbol, bsnsDecimals] = await Promise.all([
    client.getCode({ address: bridgeAddress }),
    client.getCode({ address: bsnsAddress }),
    client.readContract({ address: bridgeAddress, abi: bridgeAbi, functionName: "bsns" }),
    client.readContract({ address: bsnsAddress, abi: bsnsAbi, functionName: "symbol" }),
    client.readContract({ address: bsnsAddress, abi: bsnsAbi, functionName: "decimals" }),
  ])
  if (!bridgeCode || keccak256(bridgeCode) !== profile.bridgeRuntimeHash) blockers.push("Bridge runtime bytecode does not match the reviewed profile")
  if (!bsnsCode || keccak256(bsnsCode) !== profile.bsnsRuntimeHash) blockers.push("bSNS runtime bytecode does not match the reviewed profile")
  if (String(linkedBsns).toLowerCase() !== bsnsAddress.toLowerCase()) blockers.push("Bridge points to a different bSNS contract")
  if (bsnsSymbol !== "KINIC" || Number(bsnsDecimals) !== 8) blockers.push("Base token metadata is not KINIC/8")

  const [bridge, ledger] = await Promise.all([
    createBridgeActor(profile.icHost, profile.bridgeCanisterId as string),
    createLedgerActor(profile.icHost, profile.ledgerCanisterId as string),
  ])
  const [config, ledgerSymbol, ledgerDecimals] = await Promise.all([bridge.get_public_config(), ledger.icrc1_symbol(), ledger.icrc1_decimals()])
  if (config.base_chain_id !== BigInt(profile.chainId)) blockers.push("Canister Base chain ID differs from the profile")
  const configuredBridge = `0x${Array.from(config.bridge_contract, (byte) => Number(byte).toString(16).padStart(2, "0")).join("")}`
  if (configuredBridge.toLowerCase() !== bridgeAddress.toLowerCase()) blockers.push("Canister Bridge contract differs from the profile")
  if (config.ledger_canister_id.toText() !== profile.ledgerCanisterId) blockers.push("Canister ledger differs from the profile")
  if (config.schema_version !== 1) blockers.push(`Unsupported canister schema ${config.schema_version}`)
  if (ledgerSymbol !== "KINIC" || ledgerDecimals !== 8) blockers.push("IC token metadata is not KINIC/8")
  return { ready: blockers.length === 0, blockers, checkedAt: Date.now() }
}
