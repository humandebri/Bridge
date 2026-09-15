export const deploymentProfile = {
  chainId: 8453,
  bridgeAddress: `0x${"11".repeat(20)}`,
  bridgeCanisterId: "aaaaa-aa",
  deploymentInstanceId: `0x${"99".repeat(32)}`,
  icHost: "http://localhost",
}
export const useChainId = () => 8453
const refetch = async () => ({ data: undefined })
export const useRuntimeValidation = () => ({ data: undefined, refetch })
export const useRuntimeHeartbeat = () => ({ data: undefined, refetch })
export const refetchRuntimeAttestedWriteReady = refetch
export const mintExecutionDiagnostics = () => undefined
const forbidden = async () => {
  await fetch("/withdrawal-test/forbidden-write", { method: "POST" })
  throw new Error("This fixture must only observe already recorded withdrawals")
}
export const notifyWithdrawalWithBrowserIdentity = forbidden
export const continueWithdrawalWithBrowserIdentity = forbidden
export const continueTransferPayout = forbidden
export class NotifyWithdrawalCallError extends Error {}
export class TransactionEvidenceMismatch extends Error {}
export const withdrawalReceiptDetails = async () => ({ id: BigInt(`0x${"07".repeat(32)}`) })
export const readBaseReceipt = async () => ({
  status: "success",
  blockNumber: 10n,
  blockHash: `0x${"44".repeat(32)}`,
})
export const readBaseBlock = async () => ({ number: 10n, hash: `0x${"44".repeat(32)}` })
export const hasIndependentFinalizedRevertQuorum = async () => false
export const createBridgeActor = async () => ({
  get_withdrawal: async () => {
    const response = await fetch(
      `/withdrawal-test/read?tab=${new URL(location.href).searchParams.get("tab")}`,
    )
    if (!response.ok) throw new Error("test read failed")
    const { paid } = await response.json()
    return [
      {
        withdrawal_id: new Uint8Array(32).fill(7),
        state: paid ? { Paid: null } : { ReleasePending: null },
        last_settlement_stop_reason: [],
      },
    ]
  },
})
