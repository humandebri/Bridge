import { beforeEach, describe, expect, it, vi } from "vitest"
import { hashTypedData, recoverAddress } from "viem"
import type { DepositView, MintAuthorizationView } from "@/generated/bridge.did"
import type { FinalizedRuntimeObservation } from "@/lib/runtime-validation"
import vector from "../../../verification/generated/mint-authorization-vector.json"
import {
  assertMintAuthorizationContractHorizon,
  mintAuthorizationTypes,
  validateMintAuthorization,
} from "./mint-authorization"

describe("mint authorization deadline horizon", () => {
  it("rejects_an_authorization_beyond_the_Base_contract_deadline_horizon", () => {
    expect(() => assertMintAuthorizationContractHorizon(1_901n, 1_000n)).toThrow(
      "Mint authorization exceeds the Base contract deadline horizon",
    )
  })
})

const mocks = vi.hoisted(() => ({
  getBlock: vi.fn(),
  readContract: vi.fn(),
}))

vi.mock("@/config/profile", () => ({
  deploymentProfile: {
    bridgeAddress: "0x1111111111111111111111111111111111111111",
    chainId: 8453,
  },
}))

vi.mock("@/lib/evm/client", () => ({
  basePublicClient: {
    getBlock: mocks.getBlock,
    readContract: mocks.readContract,
  },
}))

function hexBytes(value: string): Uint8Array {
  const pairs = value.slice(2).match(/.{2}/g)
  if (!pairs) throw new Error("invalid hex fixture")
  return Uint8Array.from(pairs.map((pair) => Number.parseInt(pair, 16)))
}

function authorizationRecord(): DepositView {
  const authorization = {
    finalized_block_number: 1n,
    signature: [hexBytes(vector.signature)],
    deposit_id: hexBytes(vector.authorization.deposit_id),
    issued_at_timestamp: BigInt(vector.authorization.deadline) - 600n,
    domain_name: vector.domain.name,
    charged_service_fee: BigInt(vector.authorization.charged_service_fee),
    recipient: hexBytes(vector.authorization.recipient),
    domain_version: vector.domain.version,
    authorization_epoch: BigInt(vector.authorization.authorization_epoch),
    max_service_fee: BigInt(vector.authorization.max_service_fee),
    deadline: BigInt(vector.authorization.deadline),
    signature_dispatch_attempt: 1,
    chain_id: BigInt(vector.domain.chain_id),
    finalized_block_hash: new Uint8Array(32).fill(1),
    finalized_block_timestamp: BigInt(vector.authorization.deadline) - 600n,
    verifying_contract: hexBytes(vector.domain.verifying_contract),
    digest: hexBytes(vector.digest),
    gross_amount: BigInt(vector.authorization.gross_amount),
  } satisfies MintAuthorizationView
  return {
    base_recipient: authorization.recipient,
    deposit_id: authorization.deposit_id,
    quote: [{
      net_amount: authorization.gross_amount - authorization.charged_service_fee,
      service_fee: authorization.charged_service_fee,
    }],
    max_service_fee: authorization.max_service_fee,
    state: { AuthorizationAvailable: null },
    mint_authorization: [authorization],
    gross_amount: authorization.gross_amount,
  } as DepositView
}

function runtimeObservation(): FinalizedRuntimeObservation {
  return {
    ready: true,
    snapshot: {
      depositsPaused: false,
      mintAuthorizationEpoch: BigInt(vector.authorization.authorization_epoch),
      bridgeSigner: vector.signer,
    },
  } as FinalizedRuntimeObservation
}

describe("mint authorization protocol vector", () => {
  it("matches the shared digest and recovered signer", async () => {
    const digest = hashTypedData({
      domain: {
        name: vector.domain.name,
        version: vector.domain.version,
        chainId: BigInt(vector.domain.chain_id),
        verifyingContract: vector.domain.verifying_contract as `0x${string}`,
      },
      types: mintAuthorizationTypes,
      primaryType: "MintAuthorization",
      message: {
        depositId: vector.authorization.deposit_id as `0x${string}`,
        recipient: vector.authorization.recipient as `0x${string}`,
        grossAmount: BigInt(vector.authorization.gross_amount),
        maxServiceFee: BigInt(vector.authorization.max_service_fee),
        chargedServiceFee: BigInt(vector.authorization.charged_service_fee),
        deadline: BigInt(vector.authorization.deadline),
        authorizationEpoch: BigInt(vector.authorization.authorization_epoch),
      },
    })

    expect(digest).toBe(vector.digest)
    expect((await recoverAddress({
      hash: digest,
      signature: vector.signature as `0x${string}`,
    })).toLowerCase()).toBe(vector.signer)
  })

})

describe("mint authorization latest Base admission", () => {
  beforeEach(() => {
    mocks.getBlock.mockReset()
    mocks.readContract.mockReset().mockResolvedValue(false)
  })

  it("accepts_exactly_300_seconds_of_remaining_Base_time", async () => {
    const deadline = BigInt(vector.authorization.deadline)
    mocks.getBlock.mockResolvedValue({ timestamp: deadline - 300n })

    await expect(validateMintAuthorization(authorizationRecord(), runtimeObservation()))
      .resolves.toMatchObject({ latestBlockTimestamp: deadline - 300n })
  })

  it("rejects_299_seconds_of_remaining_Base_time", async () => {
    const deadline = BigInt(vector.authorization.deadline)
    mocks.getBlock.mockResolvedValue({ timestamp: deadline - 299n })

    await expect(validateMintAuthorization(authorizationRecord(), runtimeObservation()))
      .rejects.toThrow("less than five minutes remaining")
  })

  it("fails_closed_when_latest_Base_state_cannot_be_refreshed", async () => {
    mocks.getBlock.mockRejectedValue(new Error("RPC unavailable"))

    await expect(validateMintAuthorization(authorizationRecord(), runtimeObservation()))
      .rejects.toThrow("No Base transaction was sent")
  })

  it("rejects_an_authorization_already_processed_on_Base", async () => {
    const deadline = BigInt(vector.authorization.deadline)
    mocks.readContract.mockResolvedValue(true)
    mocks.getBlock.mockResolvedValue({ timestamp: deadline - 300n })

    await expect(validateMintAuthorization(authorizationRecord(), runtimeObservation()))
      .rejects.toThrow("already processed on Base")
  })
})
