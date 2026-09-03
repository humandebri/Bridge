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
  refetchRuntime,
  currentEvmWallet,
  currentIcAccount,
  refetchFinancials,
  validateFinancials,
  createWithdrawal,
  onBroadcast,
}: {
  expectedWallets: ExpectedWallets
  refetchRuntime: () => Promise<{ data?: R }>
  currentEvmWallet: () => Promise<{ address: `0x${string}`; chainId: number }>
  currentIcAccount: () => Promise<IcAccount>
  refetchFinancials: (runtime: R & { ready: true }) => Promise<Q>
  validateFinancials: (quote: Q) => void
  createWithdrawal: (quote: Q) => Promise<`0x${string}`>
  onBroadcast: (transactionHash: `0x${string}`) => Promise<void> | void
}): Promise<WithdrawalBroadcastResult> {
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
  const transactionHash = await createWithdrawal(quote)
  try {
    await onBroadcast(transactionHash)
    return { transactionHash, pendingSaved: true }
  } catch {
    return { transactionHash, pendingSaved: false }
  }
}
