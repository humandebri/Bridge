import { describe, expect, it } from "vitest"
import { hashTypedData, recoverAddress } from "viem"
import vector from "../../../verification/generated/mint-authorization-vector.json"
import { mintAuthorizationTypes } from "./mint-authorization"

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
