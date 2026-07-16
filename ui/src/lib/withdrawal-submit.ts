import type { IcAccount } from "@/lib/ic/wallet"
import { refetchRuntimeWriteReady, type RuntimeValidation } from "@/lib/runtime-validation"
import { requireWalletSnapshot } from "@/lib/wallet-snapshot"

interface ExpectedWallets {
  address: `0x${string}`
  chainId: number
  icAccount: IcAccount
}

export async function createWithdrawalAfterRevalidation<T, Q>({
  expectedWallets,
  refetchRuntime,
  currentEvmWallet,
  currentIcAccount,
  refetchFinancials,
  validateFinancials,
  createWithdrawal,
}: {
  expectedWallets: ExpectedWallets
  refetchRuntime: () => Promise<{ data?: RuntimeValidation }>
  currentEvmWallet: () => Promise<{ address: `0x${string}`; chainId: number }>
  currentIcAccount: () => Promise<IcAccount>
  refetchFinancials: () => Promise<Q>
  validateFinancials: (quote: Q) => void
  createWithdrawal: (quote: Q) => Promise<T>
}): Promise<T> {
  await refetchRuntimeWriteReady(refetchRuntime)
  const [evm, icAccount, quote] = await Promise.all([currentEvmWallet(), currentIcAccount(), refetchFinancials()])
  requireWalletSnapshot(expectedWallets, { ...evm, icAccount }, "after approval or runtime verification")
  validateFinancials(quote)
  return createWithdrawal(quote)
}
