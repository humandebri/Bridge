import type { IcAccount } from "@/lib/ic/wallet"
import { refetchRuntimeWriteReady, type RuntimeValidation } from "@/lib/runtime-validation"
import { requireWalletSnapshot } from "@/lib/wallet-snapshot"

interface ExpectedWallets {
  address: `0x${string}`
  chainId: number
  icAccount: IcAccount
}

export async function createWithdrawalAfterRevalidation<T>({
  expectedWallets,
  refetchRuntime,
  currentEvmWallet,
  currentIcAccount,
  createWithdrawal,
}: {
  expectedWallets: ExpectedWallets
  refetchRuntime: () => Promise<{ data?: RuntimeValidation }>
  currentEvmWallet: () => Promise<{ address: `0x${string}`; chainId: number }>
  currentIcAccount: () => Promise<IcAccount>
  createWithdrawal: () => Promise<T>
}): Promise<T> {
  await refetchRuntimeWriteReady(refetchRuntime)
  const [evm, icAccount] = await Promise.all([currentEvmWallet(), currentIcAccount()])
  requireWalletSnapshot(expectedWallets, { ...evm, icAccount }, "after approval or runtime verification")
  return createWithdrawal()
}
