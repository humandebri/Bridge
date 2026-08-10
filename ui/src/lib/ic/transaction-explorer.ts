import { Principal } from "@dfinity/principal"

const ICP_DASHBOARD_ORIGIN = "https://dashboard.internetcomputer.org"

export function kinicTransactionExplorerUrl(
  snsRootCanisterId: string | null | undefined,
  blockIndex: bigint,
): string | undefined {
  if (!snsRootCanisterId || blockIndex < 0n) return undefined
  try {
    const root = Principal.fromText(snsRootCanisterId).toText()
    return `${ICP_DASHBOARD_ORIGIN}/sns/${root}/transaction/${blockIndex.toString()}`
  } catch {
    return undefined
  }
}
