import { Actor, HttpAgent } from '@icp-sdk/core/agent';
import { IDL } from '@icp-sdk/core/candid';
import { Principal } from '@icp-sdk/core/principal';
import { writeFileSync } from 'node:fs';
import { createHash } from 'node:crypto';
import { isMainModule } from './prepare.mjs';

export const BRIDGE = 'lb5i5-ziaaa-aaaar-qcgwq-cai';
export const GOVERNANCE = '74ncn-fqaaa-aaaaq-aaasa-cai';
export const LEDGER = '73mez-iiaaa-aaaaq-aaasq-cai';
const recipient = IDL.Record({ owner: IDL.Principal, subaccount: IDL.Vec(IDL.Nat8) });
export const proposalType = IDL.Record({ payout_id: IDL.Nat64, amount: IDL.Nat, recipient });
const error = IDL.Variant({ Busy: IDL.Null, Unauthorized: IDL.Null, InvalidArgument: IDL.Text, StorageFailure: IDL.Null, InsufficientFeeReserve: IDL.Null });
const state = IDL.Variant({ Pending: IDL.Null, Succeeded: IDL.Record({ block_index: IDL.Nat }), ReconciliationHold: IDL.Null, Failed: IDL.Null });
const statusType = IDL.Record({ fee_reserve: IDL.Nat, pending_payout_debit: IDL.Nat, ledger_fee: IDL.Nat, max_payout_amount: IDL.Nat, fee_recipient: recipient, next_fee_payout_id: IDL.Nat64 });
const payoutType = IDL.Record({ id: IDL.Nat64, amount: IDL.Nat, recipient, ledger_fee: IDL.Nat, state });
const topic = IDL.Variant(Object.fromEntries(['DaoCommunitySettings', 'SnsFrameworkManagement', 'DappCanisterManagement', 'ApplicationBusinessLogic', 'Governance', 'TreasuryAssetManagement', 'CriticalDappOperations'].map(x => [x, IDL.Null])));
const generic = IDL.Record({ target_canister_id: IDL.Opt(IDL.Principal), target_method_name: IDL.Opt(IDL.Text), validator_canister_id: IDL.Opt(IDL.Principal), validator_method_name: IDL.Opt(IDL.Text), topic: IDL.Opt(topic) });
const registryType = IDL.Record({ reserved_ids: IDL.Vec(IDL.Nat64), functions: IDL.Vec(IDL.Record({ id: IDL.Nat64, function_type: IDL.Opt(IDL.Variant({ NativeNervousSystemFunction: IDL.Reserved, GenericNervousSystemFunction: generic })) })) });
export function rawInteger(text, maximum = (1n << 128n) - 1n) {
  if (!/^(0|[1-9][0-9]*)$/.test(text ?? '')) throw new Error('Use a canonical nonnegative integer in raw units');
  const value = BigInt(text); if (value > maximum) throw new Error('Integer exceeds the supported range'); return value;
}
export function formatKinic(amount, decimals) {
  if (!Number.isInteger(decimals) || decimals < 0 || decimals > 255) throw new Error('Invalid Ledger decimals');
  const raw = BigInt(amount).toString().padStart(decimals + 1, '0');
  return decimals === 0 ? raw : `${raw.slice(0, -decimals)}.${raw.slice(-decimals)}`;
}
const json = value => JSON.stringify(value, (_, x) => typeof x === 'bigint' ? x.toString() : x instanceof Principal ? x.toText() : x instanceof Uint8Array ? [...x] : x, 2);
const blob = bytes => 'blob "' + [...bytes].map(x => '\\' + x.toString(16).padStart(2, '0')).join('') + '"';
const candidString = text => JSON.stringify(text).replace(/\\u([0-9a-f]{4})/gi, (_, n) => String.fromCharCode(parseInt(n, 16)));
const unwrap = value => { if (!('Ok' in value)) throw new Error(`Canister rejected query: ${json(value.Err)}`); return value.Ok; };
export function prepareFeeProposal(registry, payload, ledgerFee, decimals, continuation = false) {
  rawInteger(payload.amount.toString()); rawInteger(payload.payout_id.toString(), (1n << 64n) - 1n);
  if (payload.amount === 0n) throw new Error('Payout amount must be positive');
  if (![0, 32].includes(payload.recipient.subaccount.length) || payload.recipient.owner.toText() === '2vxsx-fae') throw new Error('Invalid recipient');
  const phase = continuation ? 'continue' : 'request';
  const method = `sns_${phase}_fee_payout`, validator = `validate_${method}`;
  const ids = registry.functions.map(f => f.id.toString());
  if (new Set(ids).size !== ids.length) throw new Error('Duplicate registry function IDs');
  const used = new Set([...ids, ...registry.reserved_ids.map(String)]);
  const candidates = registry.functions.filter(f => { const g = f.function_type[0]?.GenericNervousSystemFunction; return g?.target_canister_id[0]?.toText() === BRIDGE && g.target_method_name[0] === method; });
  if (candidates.length > 1) throw new Error('Duplicate payout functions');
  let id, registration = null;
  if (candidates.length) {
    const f = candidates[0], g = f.function_type[0].GenericNervousSystemFunction;
    if (registry.reserved_ids.includes(f.id) || g.validator_canister_id[0]?.toText() !== BRIDGE || g.validator_method_name[0] !== validator || !('TreasuryAssetManagement' in (g.topic[0] ?? {}))) throw new Error('Registered payout function binding mismatch');
    id = f.id;
  } else {
    id = 1000n; while (used.has(id.toString())) id++;
    if (id > (1n << 64n) - 1n) throw new Error('Function ID space exhausted');
    registration = `record { title = "Enable KINIC Bridge fee ${phase} proposals"; url = ""; summary = "Register the dedicated DAO fee payout function. This proposal does not transfer KINIC."; action = opt variant { AddGenericNervousSystemFunction = record { id = ${id} : nat64; name = "KINIC Bridge fee ${phase}"; description = opt "Fixed payout identity, amount and recipient."; function_type = opt variant { GenericNervousSystemFunction = record { topic = opt variant { TreasuryAssetManagement }; target_canister_id = opt principal "${BRIDGE}"; target_method_name = opt "${method}"; validator_canister_id = opt principal "${BRIDGE}"; validator_method_name = opt "${validator}" } } } } }`;
  }
  const description = `${continuation ? 'Continue' : 'Request'} payout ${payload.payout_id}: ${formatKinic(payload.amount, decimals)} KINIC (${payload.amount} raw units) to ${payload.recipient.owner.toText()}, subaccount ${Buffer.from(payload.recipient.subaccount).toString('hex') || 'default'}. Ledger fee ${formatKinic(ledgerFee, decimals)} KINIC (${ledgerFee} raw units). Acceptance or continuation is not confirmation of Ledger settlement.`;
  const bytes = new Uint8Array(IDL.encode([proposalType], [payload]));
  return { governance_canister_id: GOVERNANCE, bridge_canister_id: BRIDGE, function_id: id.toString(), target_method_name: method, validator_method_name: validator, topic: 'TreasuryAssetManagement', payload, ledger_fee: ledgerFee, ledger_decimals: decimals, description, payload_hex: Buffer.from(bytes).toString('hex'), payload_sha256: createHash('sha256').update(bytes).digest('hex'), registration_proposal: registration,
    execution_proposal: `record { title = "${continuation ? 'Continue' : 'Request'} KINIC Bridge fee payout ${payload.payout_id}"; url = ""; summary = ${candidString(description)}; action = opt variant { ExecuteGenericNervousSystemFunction = record { function_id = ${id} : nat64; payload = ${blob(bytes)} } } }` };
}
export async function runFeeCommand(args, clients) {
  const [command, value, output] = args;
  if (!['fee-status', 'fee-payout-status', 'prepare-fee-payout', 'prepare-continue-fee-payout'].includes(command)) throw new Error('Unknown fee command');
  const preparing = command.startsWith('prepare-');
  if (args.length !== (command === 'fee-status' ? 1 : preparing ? 3 : 2)) throw new Error('Usage: fees.mjs fee-status | fee-payout-status ID | prepare-fee-payout RAW_AMOUNT OUTPUT | prepare-continue-fee-payout ID OUTPUT');
  const decimals = await clients.ledger.icrc1_decimals();
  if (command === 'fee-status') { const status = unwrap(await clients.bridge.get_fee_status()); return { ...status, ledger_decimals: decimals, fee_reserve_kinic: formatKinic(status.fee_reserve, decimals), pending_payout_debit_kinic: formatKinic(status.pending_payout_debit, decimals), ledger_fee_kinic: formatKinic(status.ledger_fee, decimals), max_payout_kinic: formatKinic(status.max_payout_amount, decimals) }; }
  if (command === 'fee-payout-status') { const view = unwrap(await clients.bridge.get_fee_payout(rawInteger(value, (1n << 64n) - 1n))); if (!view.length) throw new Error('Payout not found'); return { ...view[0], ledger_decimals: decimals, amount_kinic: formatKinic(view[0].amount, decimals), ledger_fee_kinic: formatKinic(view[0].ledger_fee, decimals) }; }
  let payload, fee;
  if (command === 'prepare-fee-payout') {
    const status = unwrap(await clients.bridge.get_fee_status());
    const amount = rawInteger(value); if (!amount || amount > status.max_payout_amount) throw new Error('Amount exceeds available payout capacity or is zero');
    payload = { payout_id: status.next_fee_payout_id, amount, recipient: status.fee_recipient }; fee = status.ledger_fee;
  } else {
    const view = unwrap(await clients.bridge.get_fee_payout(rawInteger(value, (1n << 64n) - 1n)));
    if (!view.length || 'Failed' in view[0].state || 'Succeeded' in view[0].state) throw new Error('Payout is absent or terminal');
    payload = { payout_id: view[0].id, amount: view[0].amount, recipient: view[0].recipient }; fee = view[0].ledger_fee;
  }
  const registry = await clients.governance.list_nervous_system_functions();
  const prepared = prepareFeeProposal(registry, payload, fee, decimals, command === 'prepare-continue-fee-payout');
  writeFileSync(output, json(prepared) + '\n', { flag: 'wx', mode: 0o600 });
  return prepared;
}
export function feeClients(agent) {
  return {
    bridge: Actor.createActor(() => IDL.Service({ get_fee_status: IDL.Func([], [IDL.Variant({ Ok: statusType, Err: error })], ['query']), get_fee_payout: IDL.Func([IDL.Nat64], [IDL.Variant({ Ok: IDL.Opt(payoutType), Err: error })], ['query']) }), { agent, canisterId: BRIDGE }),
    governance: Actor.createActor(() => IDL.Service({ list_nervous_system_functions: IDL.Func([], [registryType], ['query']) }), { agent, canisterId: GOVERNANCE }),
    ledger: Actor.createActor(() => IDL.Service({ icrc1_decimals: IDL.Func([], [IDL.Nat8], ['query']) }), { agent, canisterId: LEDGER }),
  };
}
if (isMainModule(process.argv[1], import.meta.url)) {
  const agent = await HttpAgent.create({ host: 'https://icp-api.io', verifyQuerySignatures: true });
  console.log(json(await runFeeCommand(process.argv.slice(2), feeClients(agent))));
}
