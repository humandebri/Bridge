import { BaseError, BlockNotFoundError, RpcRequestError } from "viem"
import { bridgeAbi } from "@/generated/abi/bridge.generated"
import { bsnsAbi } from "@/generated/abi/bsns.generated"
import type { IcAccount } from "@/lib/ic/wallet"
import { refetchRuntimeWriteReady, type RuntimeValidation } from "@/lib/runtime-validation"
import { requireWalletSnapshot } from "@/lib/wallet-snapshot"

interface ExpectedWallets {
  address: `0x${string}`
  chainId: number
  icAccount: IcAccount
}

export interface WithdrawalBroadcastResult {
  transactionHash: `0x${string}`
  pendingSaved: boolean
}

export async function createWithdrawalAfterRevalidation<R extends RuntimeValidation, Q>({
  expectedWallets,
  approval,
  simulateWithdrawal,
  refetchRuntime,
  currentEvmWallet,
  currentIcAccount,
  refetchFinancials,
  validateFinancials,
  createWithdrawal,
  onBroadcast,
}: {
  expectedWallets: ExpectedWallets
  approval: WithdrawalApprovalObservation
  simulateWithdrawal: (quote: Q, blockNumber: bigint) => Promise<unknown>
  refetchRuntime: () => Promise<{ data?: R }>
  currentEvmWallet: () => Promise<{ address: `0x${string}`; chainId: number }>
  currentIcAccount: () => Promise<IcAccount>
  refetchFinancials: (runtime: R & { ready: true }) => Promise<Q>
  validateFinancials: (quote: Q) => void
  createWithdrawal: (quote: Q) => Promise<`0x${string}`>
  onBroadcast: (transactionHash: `0x${string}`) => Promise<void> | void
}): Promise<WithdrawalBroadcastResult> {
  const quote = await waitForWithdrawalApproval({
    ...approval,
    simulate: async (blockNumber) => {
      const runtime = await refetchRuntimeWriteReady(refetchRuntime)
      const [evm, icAccount, quote] = await Promise.all([
        currentEvmWallet(),
        currentIcAccount(),
        refetchFinancials(runtime),
      ])
      requireWalletSnapshot(
        expectedWallets,
        { ...evm, icAccount },
        "after approval or runtime verification",
      )
      validateFinancials(quote)
      await simulateWithdrawal(quote, blockNumber)
      return quote
    },
  })
  const transactionHash = await createWithdrawal(quote)
  try {
    await onBroadcast(transactionHash)
    return { transactionHash, pendingSaved: true }
  } catch {
    return { transactionHash, pendingSaved: false }
  }
}

// Token errors bubble through Bridge.transferFrom without appearing in the Bridge ABI.
export const withdrawalAbi = [
  ...bridgeAbi,
  ...bsnsAbi.filter((item) => item.type === "error"),
] as const

export interface ApprovalReceipt {
  status: "success" | "reverted"
  blockNumber: bigint
  blockHash: `0x${string}`
}

interface WithdrawalApprovalObservation {
  receipt?: ApprovalReceipt
  amount: bigint
  getBlock: (number?: bigint) => Promise<{ number: bigint | null; hash: `0x${string}` | null }>
  readAllowance: (blockNumber: bigint) => Promise<bigint>
}

export async function waitForWithdrawalApproval<T>({
  receipt,
  amount,
  getBlock,
  readAllowance,
  simulate,
}: WithdrawalApprovalObservation & {
  simulate: (blockNumber: bigint) => Promise<T>
}): Promise<T> {
  if (receipt?.status === "reverted") throw new Error("Token approval failed")
  const deadline = Date.now() + 60_000
  const timeout = () =>
    new Error(
      "Token approval could not be confirmed within 60 seconds. The withdrawal was not submitted. Review and try again.",
    )
  // Bound each RPC too: a late read must never resume the submission after timeout.
  const bounded = async <T>(operation: () => Promise<T>): Promise<T> => {
    const remaining = deadline - Date.now()
    if (remaining <= 0) throw timeout()
    let timer: ReturnType<typeof setTimeout> | undefined
    try {
      return await Promise.race([
        operation(),
        new Promise<never>((_, reject) => {
          timer = setTimeout(() => reject(timeout()), remaining)
        }),
      ])
    } finally {
      clearTimeout(timer)
    }
  }
  while (Date.now() < deadline) {
    try {
      const latest = await bounded(() => getBlock())
      if (
        latest.number !== null &&
        latest.hash !== null &&
        (!receipt || latest.number >= receipt.blockNumber)
      ) {
        if (receipt) {
          const included = await bounded(() => getBlock(receipt.blockNumber))
          if (included.hash === null || included.number === null)
            throw new BlockNotFoundError({ blockNumber: receipt.blockNumber })
          if (
            included.number !== receipt.blockNumber ||
            included.hash.toLowerCase() !== receipt.blockHash.toLowerCase()
          )
            throw new Error(
              "Token approval block changed. The withdrawal was not submitted. Review again.",
            )
        }
        const blockNumber = latest.number
        if ((await bounded(() => readAllowance(blockNumber))) >= amount)
          return await bounded(() => simulate(blockNumber))
      }
    } catch (error) {
      const missingBlock =
        error instanceof BaseError &&
        Boolean(
          error.walk(
            (cause) =>
              cause instanceof BlockNotFoundError ||
              // eth_call preserves the RPC error instead of producing BlockNotFoundError.
              (cause instanceof RpcRequestError &&
                cause.data === undefined &&
                [-32000, -32001, -32002].includes(cause.code) &&
                /^(?:header not found|block not found|unknown block|could not find block|block is not yet available)$/i.test(
                  cause.details?.trim() ?? "",
                )),
          ),
        )
      if (!missingBlock) throw error
    }
    await new Promise((resolve) =>
      setTimeout(resolve, Math.min(2_000, Math.max(0, deadline - Date.now()))),
    )
  }
  throw timeout()
}
