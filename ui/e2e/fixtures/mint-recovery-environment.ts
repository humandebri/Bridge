import { encodeAbiParameters, encodeEventTopics, type Hex } from "viem"
import { bridgeAbi } from "@/generated/abi/bridge.generated"
import type { DepositView } from "@/generated/bridge.did"
export const deploymentProfile = {
  chainId: 8453,
  bridgeAddress: `0x${"44".repeat(20)}` as Hex,
  deploymentInstanceId: `0x${"88".repeat(32)}`,
  mintRecoveryUrl: "https://recovery.bridge.kinic.xyz/v1/mint-recovery",
  bridgeCanisterId: "aaaaa-aa",
  icHost: "http://localhost",
  deploymentBlock: 1n,
}
const id = `0x${"11".repeat(32)}` as Hex,
  digest = `0x${"33".repeat(32)}` as Hex
const recipient = `0x${"55".repeat(20)}` as Hex,
  hash = `0x${"22".repeat(32)}` as Hex
const blockHash = `0x${"66".repeat(32)}` as Hex
const args = {
  depositId: id,
  authorizationDigest: digest,
  recipient,
  grossAmount: 100n,
  serviceFee: 10n,
  mintedAmount: 90n,
}
export const record = {
  deposit_id: new Uint8Array(32).fill(0x11),
  state: { AuthorizationAvailable: null },
  mint_receipt: [],
  mint_authorization: [
    {
      signature: [new Uint8Array(65)],
      digest: new Uint8Array(32).fill(0x33),
      recipient: new Uint8Array(20).fill(0x55),
      gross_amount: 100n,
      charged_service_fee: 10n,
      finalized_block_number: 1n,
      deadline: 2000n,
    },
  ],
} as unknown as DepositView
const completed = () => localStorage.getItem("fixture-base-success") === "yes"
export const basePublicClient = {
  async getBlock(input: { blockNumber?: bigint }) {
    return {
      number: input.blockNumber ?? (completed() ? 100n : 40n),
      hash: blockHash,
      timestamp: 3000n,
    }
  },
  async getContractEvents() {
    return completed()
      ? [
          {
            address: deploymentProfile.bridgeAddress,
            blockNumber: 42n,
            blockHash,
            transactionHash: hash,
            logIndex: 0,
            args,
          },
        ]
      : []
  },
  async getTransactionReceipt() {
    return {
      status: "success" as const,
      blockNumber: 42n,
      blockHash,
      logs: [
        {
          address: deploymentProfile.bridgeAddress,
          topics: encodeEventTopics({ abi: bridgeAbi, eventName: "DepositMinted", args }),
          data: encodeAbiParameters(
            [{ type: "uint256" }, { type: "uint256" }, { type: "uint256" }],
            [100n, 10n, 90n],
          ),
        },
      ],
    }
  },
}
export const baseHistoryClients = [basePublicClient]
export const createBridgeActor = async () => ({
  get_deposit: async () => [
    { ...record, mint_receipt: localStorage.getItem("fixture-recorded") === "yes" ? [{}] : [] },
  ],
  list_deposit_ids: async () => ({ Ok: { deposit_ids: [record.deposit_id], next_cursor: [] } }),
  notify_deposit_mint: async () => {
    localStorage.setItem("fixture-recorded", "yes")
    return { Ok: { Recorded: { deposit_id: record.deposit_id } } }
  },
})
export const getWithdrawalNotificationIdentity = async () => ({})

export async function firstSuccessfulHistoryClient<C, T>(clients: readonly C[], operation: (client: C) => Promise<T>): Promise<T> {
  return operation(clients[0]!)
}

export function installRecoveryServiceFixture() {
  window.fetch = async () =>
    localStorage.getItem("fixture-service-failure") === "yes"
      ? new Response(null, { status: 503 })
      : Response.json({
          deploymentInstanceId: deploymentProfile.deploymentInstanceId,
          depositId: id,
          authorizationDigest: digest,
          hashes: completed() ? [hash] : [],
          cursor: null,
        })
}
