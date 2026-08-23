import { IcrcLedgerCanister } from "@icp-sdk/canisters/ledger/icrc"
import { Principal } from "@icp-sdk/core/principal"
import { createIcAgent } from "@/lib/ic/agent"

export interface LedgerAccount { owner: Principal; subaccount: [] | [Uint8Array] }
export interface LedgerAllowance { allowance: bigint; expires_at: [] | [bigint] }
export interface LedgerActor {
  icrc1_balance_of(account: LedgerAccount): Promise<bigint>
  icrc1_fee(): Promise<bigint>
  icrc1_name(): Promise<string>
  icrc1_decimals(): Promise<number>
  icrc1_symbol(): Promise<string>
  icrc2_allowance(args: { account: LedgerAccount; spender: LedgerAccount }): Promise<LedgerAllowance>
}

function balanceAccount(value: LedgerAccount) {
  return { owner: value.owner, subaccount: value.subaccount[0] }
}

function candidAccount(value: LedgerAccount) {
  return { owner: value.owner, subaccount: value.subaccount }
}

function metadataText(metadata: Awaited<ReturnType<IcrcLedgerCanister["metadata"]>>, key: string): string {
  const value = metadata.find(([name]) => name === key)?.[1]
  if (!value || !("Text" in value)) throw new Error(`Ledger metadata ${key} is unavailable`)
  return value.Text
}

function metadataNat(metadata: Awaited<ReturnType<IcrcLedgerCanister["metadata"]>>, key: string): number {
  const value = metadata.find(([name]) => name === key)?.[1]
  if (!value || !("Nat" in value)) throw new Error(`Ledger metadata ${key} is unavailable`)
  return Number(value.Nat)
}

export async function createLedgerActor(host: string, canisterId: string): Promise<LedgerActor> {
  const ledger = IcrcLedgerCanister.create({
    agent: await createIcAgent(host),
    canisterId: Principal.fromText(canisterId),
  })
  let metadata: Awaited<ReturnType<typeof ledger.metadata>> | undefined
  const readMetadata = async () => metadata ??= await ledger.metadata({ certified: false })
  return {
    icrc1_balance_of: (value) => ledger.balance({ ...balanceAccount(value), certified: false }),
    icrc1_fee: () => ledger.transactionFee({ certified: false }),
    icrc1_name: async () => metadataText(await readMetadata(), "icrc1:name"),
    icrc1_decimals: async () => metadataNat(await readMetadata(), "icrc1:decimals"),
    icrc1_symbol: async () => metadataText(await readMetadata(), "icrc1:symbol"),
    icrc2_allowance: async ({ account: owner, spender }) => {
      const result = await ledger.allowance({
        account: candidAccount(owner),
        spender: candidAccount(spender),
        certified: false,
      })
      return {
        allowance: result.allowance,
        expires_at: result.expires_at,
      }
    },
  }
}

export function ledgerAccount(owner: string, subaccount?: Uint8Array): LedgerAccount {
  return { owner: Principal.fromText(owner), subaccount: subaccount ? [subaccount] : [] }
}
