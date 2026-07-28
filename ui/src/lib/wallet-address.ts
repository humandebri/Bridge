export function shortenWalletAddress(wallet: string) {
  return wallet.length > 16 && (wallet.startsWith("0x") || wallet.includes("-"))
    ? `${wallet.slice(0, 6)}…${wallet.slice(-4)}`
    : wallet
}
