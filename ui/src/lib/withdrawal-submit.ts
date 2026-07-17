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

export async function createWithdrawalAfterRevalidation<Q>({
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
  refetchRuntime: () => Promise<{ data?: RuntimeValidation }>
  currentEvmWallet: () => Promise<{ address: `0x${string}`; chainId: number }>
  currentIcAccount: () => Promise<IcAccount>
  refetchFinancials: () => Promise<Q>
  validateFinancials: (quote: Q) => void
  createWithdrawal: (quote: Q) => Promise<`0x${string}`>
  onBroadcast: (transactionHash: `0x${string}`) => void
}): Promise<WithdrawalBroadcastResult> {
  await refetchRuntimeWriteReady(refetchRuntime)
  const [evm, icAccount, quote] = await Promise.all([currentEvmWallet(), currentIcAccount(), refetchFinancials()])
  requireWalletSnapshot(expectedWallets, { ...evm, icAccount }, "after approval or runtime verification")
  validateFinancials(quote)
  const transactionHash = await createWithdrawal(quote)
  try {
    onBroadcast(transactionHash)
    return { transactionHash, pendingSaved: true }
  } catch {
    return { transactionHash, pendingSaved: false }
  }
}
