import { beforeEach, describe, expect, it, vi } from "vitest"
import type { DepositView, MintAuthorizationView } from "@/generated/bridge.did"
import type { FinalizedRuntimeObservation } from "@/lib/runtime-validation"
import vector from "../../../verification/generated/mint-authorization-vector.json"
import { validateMintAuthorization, sharedMintAuthorizationTypes } from "./mint-authorization"
import { privateKeyToAccount } from "viem/accounts"
import { hashTypedData, hexToBytes, bytesToHex } from "viem"
import { runtimeBytecodeSha256 } from "./runtime-bytecode-hash"
vi.mock("@/lib/ic/bridge", () => ({
  createBridgeActor: async () => ({ get_asset: mocks.getAsset }),
}))

const mocks = vi.hoisted(() => ({
  getBlock: vi.fn(),
  readContract: vi.fn(),
  getAsset: vi.fn(),
  getBytecode: vi.fn(),
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
    issued_at_timestamp: BigInt(vector.authorization.deadline) - 900n,
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
    finalized_block_timestamp: BigInt(vector.authorization.deadline) - 900n,
    verifying_contract: hexBytes(vector.domain.verifying_contract),
    digest: hexBytes(vector.digest),
    gross_amount: BigInt(vector.authorization.gross_amount),
  } satisfies MintAuthorizationView
  return {
    asset_id: new Uint8Array(32),
    bridge_kind: { LegacySingleToken: null },
    asset_authorization_epoch: [],
    base_recipient: authorization.recipient,
    deposit_id: authorization.deposit_id,
    quote: [
      {
        net_amount: authorization.gross_amount - authorization.charged_service_fee,
        service_fee: authorization.charged_service_fee,
      },
    ],
    max_service_fee: authorization.max_service_fee,
    state: { AuthorizationAvailable: null },
    mint_receipt: [],
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
    mocks.getBlock.mockResolvedValue({ timestamp: BigInt(vector.authorization.deadline) - 300n })
    mocks.readContract.mockResolvedValue(false)
    const validated = await validateMintAuthorization(authorizationRecord(), runtimeObservation())
    expect(validated.digest).toBe(vector.digest)
    expect(validated.signer.toLowerCase()).toBe(vector.signer)
    expect(validated.signature).toBe(vector.signature)
  })
})

describe("mint authorization latest Base admission", () => {
  beforeEach(() => {
    mocks.getBlock.mockReset()
    mocks.readContract.mockReset().mockResolvedValue(false)
  })

  it("rejects_an_authorization_beyond_the_Base_contract_deadline_horizon", async () => {
    const deadline = BigInt(vector.authorization.deadline)
    mocks.getBlock
      .mockResolvedValueOnce({ timestamp: deadline - 901n })
      .mockResolvedValueOnce({ timestamp: deadline - 900n })

    await expect(
      validateMintAuthorization(authorizationRecord(), runtimeObservation()),
    ).rejects.toThrow("Mint authorization exceeds the Base contract deadline horizon")
    await expect(
      validateMintAuthorization(authorizationRecord(), runtimeObservation()),
    ).resolves.toMatchObject({ latestBlockTimestamp: deadline - 900n })
  })

  it("accepts_exactly_300_seconds_of_remaining_Base_time", async () => {
    const deadline = BigInt(vector.authorization.deadline)
    mocks.getBlock.mockResolvedValue({ timestamp: deadline - 300n })

    await expect(
      validateMintAuthorization(authorizationRecord(), runtimeObservation()),
    ).resolves.toMatchObject({ latestBlockTimestamp: deadline - 300n })
  })

  it("accepts_unexpired_Base_time_and_rejects_expired_authorization", async () => {
    const deadline = BigInt(vector.authorization.deadline)
    for (const remaining of [299n, 1n, 0n]) {
      mocks.getBlock.mockResolvedValue({ timestamp: deadline - remaining })
      await expect(
        validateMintAuthorization(authorizationRecord(), runtimeObservation()),
      ).resolves.toMatchObject({ latestBlockTimestamp: deadline - remaining })
    }
    mocks.getBlock.mockResolvedValue({ timestamp: deadline + 1n })
    await expect(
      validateMintAuthorization(authorizationRecord(), runtimeObservation()),
    ).rejects.toThrow("Mint authorization has expired. No Base transaction was sent.")
  })

  it("fails_closed_when_latest_Base_state_cannot_be_refreshed", async () => {
    mocks.getBlock.mockRejectedValue(new Error("RPC unavailable"))

    await expect(
      validateMintAuthorization(authorizationRecord(), runtimeObservation()),
    ).rejects.toThrow("No Base transaction was sent")
  })

  it("rejects_an_authorization_already_processed_on_Base", async () => {
    const deadline = BigInt(vector.authorization.deadline)
    mocks.readContract.mockResolvedValue(true)
    mocks.getBlock.mockResolvedValue({ timestamp: deadline - 300n })

    await expect(
      validateMintAuthorization(authorizationRecord(), runtimeObservation()),
    ).rejects.toThrow("already processed on Base")
  })
})

it("honors_signed_shared_fee_after_current_fee_change", async () => {
  const account = privateKeyToAccount(`0x${"11".repeat(32)}`)
  const record = authorizationRecord()
  record.bridge_kind = { SharedMultiToken: null }
  record.asset_id = new Uint8Array(32).fill(9)
  record.asset_authorization_epoch = [2n]
  const view = record.mint_authorization[0]!
  view.domain_name = "IC Base Multi-Token Bridge"
  view.verifying_contract = new Uint8Array(20).fill(0x11)
  const code = "0x6000" as const
  mocks.getAsset.mockResolvedValue({
    Ok: [
      {
        asset_id: record.asset_id,
        bridge_kind: record.bridge_kind,
        bridge_contract: view.verifying_contract,
        token_contract: new Uint8Array(20).fill(0x22),
        expected_bridge_runtime_sha256: hexToBytes(runtimeBytecodeSha256(code)),
        expected_token_runtime_sha256: hexToBytes(runtimeBytecodeSha256(code)),
      },
    ],
  })
  mocks.getBytecode.mockResolvedValue(code)
  const currentMaximum = 30n
  mocks.readContract.mockImplementation(async ({ functionName }) => {
    if (functionName === "assetSnapshot")
      return {
        serviceFee: 20n,
        minServiceFee: 1n,
        maxServiceFee: currentMaximum,
        assetEpoch: 2n,
        depositMintsPaused: false,
      }
    if (functionName === "globalEpoch") return view.authorization_epoch
    if (functionName === "bridgeSigner") return account.address
    if (functionName === "tokenForAsset") return `0x${"22".repeat(20)}`
    return false
  })
  mocks.getBlock.mockResolvedValue({ timestamp: view.deadline - 300n })
  const client = {
    getBlock: mocks.getBlock,
    readContract: mocks.readContract,
    getBytecode: mocks.getBytecode,
  } as unknown as Parameters<typeof validateMintAuthorization>[2]
  for (const chargedFee of [10n, 31n]) {
    record.max_service_fee = 40n
    view.max_service_fee = 40n
    view.charged_service_fee = chargedFee
    record.quote[0]!.service_fee = chargedFee
    const typedData = {
      domain: {
        name: view.domain_name,
        version: "1",
        chainId: Number(view.chain_id),
        verifyingContract: bytesToHex(view.verifying_contract),
      },
      types: sharedMintAuthorizationTypes,
      primaryType: "MintAuthorization" as const,
      message: {
        assetId: bytesToHex(record.asset_id),
        depositId: bytesToHex(Uint8Array.from(view.deposit_id)),
        recipient: bytesToHex(Uint8Array.from(view.recipient)),
        grossAmount: view.gross_amount,
        maxServiceFee: view.max_service_fee,
        chargedServiceFee: chargedFee,
        deadline: view.deadline,
        globalEpoch: view.authorization_epoch,
        assetEpoch: 2n,
      },
    }
    view.digest = hexToBytes(hashTypedData(typedData))
    view.signature = [hexToBytes(await account.signTypedData(typedData))]
    if (chargedFee <= currentMaximum)
      await expect(
        validateMintAuthorization(record, runtimeObservation(), client),
      ).resolves.toMatchObject({ shared: true, authorization: { chargedServiceFee: 10n } })
    else
      await expect(validateMintAuthorization(record, runtimeObservation(), client)).rejects.toThrow(
        "no longer valid",
      )
  }
})
