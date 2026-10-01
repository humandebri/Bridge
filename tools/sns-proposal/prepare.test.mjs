import { test } from 'node:test';
import { spawnSync } from 'node:child_process';
import assert from 'node:assert/strict';
import { mkdtempSync, rmSync, symlinkSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { IDL } from '@icp-sdk/core/candid';
import { Principal } from '@icp-sdk/core/principal';
import { prepare, payloadType, decodeRegistry, isMainModule } from './prepare.mjs';
const bridge = 'lb5i5-ziaaa-aaaar-qcgwq-cai';
const registry = () => ({ functions: [], reserved_ids: [1000n, 1002n] });
test('main detection accepts filesystem aliases of the invoked script', t => {
  const directory = mkdtempSync(join(tmpdir(), 'bridge-sns-main-alias.'));
  t.after(() => rmSync(directory, { recursive: true, force: true }));
  const alias = join(directory, 'prepare-alias.mjs');
  symlinkSync(fileURLToPath(new URL('./prepare.mjs', import.meta.url)), alias);
  assert.equal(isMainModule(alias), true);
  assert.equal(isMainModule(fileURLToPath(import.meta.url)), false);
});
test('registration avoids active and reserved IDs and binds typed payload', () => {
  const result = prepare(registry(), bridge, '42');
  assert.deepEqual(result.map(x => x.function_id), ['1001', '1003']);
  assert.deepEqual(IDL.decode([payloadType], Uint8Array.from(Buffer.from(result[0].payload_hex, 'hex')).buffer),
    [{ previous_governance_operation_id: 42n }]);
  assert.match(result[0].registration_proposal, /validate_sns_schedule_activation/);
  assert.match(result[1].registration_proposal, /validate_sns_execute_activation/);
});
test('exact registered function is reused; mismatched validators and duplicates fail', () => {
  const r = registry();
  const g = { target_canister_id: [Principal.fromText(bridge)], target_method_name: ['sns_schedule_activation'],
    validator_canister_id: [Principal.fromText(bridge)], validator_method_name: ['validate_sns_schedule_activation'] };
  r.functions.push({ id: 1234n, function_type: [{ GenericNervousSystemFunction: g }] });
  assert.equal(prepare(r, bridge, '42')[0].registration_proposal, null);
  g.validator_method_name = ['other'];
  assert.throws(() => prepare(r, bridge, '42'), /mismatch/);
  g.validator_method_name = ['validate_sns_schedule_activation'];
  r.functions.push(r.functions[0]);
  assert.throws(() => prepare(r, bridge, '42'), /Duplicate/);
});
test('invalid input and rendered text without raw Candid fail closed', () => {
  for (const previous of ['-1', '1e3', '18446744073709551616'])
    assert.throws(() => prepare(registry(), bridge, previous));
  assert.throws(() => prepare(registry(), 'aaaaa-aa', '42'));
  assert.throws(() => decodeRegistry({ response: '{ functions: [] }' }));
});

test('proposal response requires typed MakeProposal success', async () => {
  const {decodeProposalResponse} = await import('./prepare.mjs');
  const type = IDL.Record({command: IDL.Opt(IDL.Variant({MakeProposal: IDL.Record({proposal_id: IDL.Opt(IDL.Record({id:IDL.Nat64}))}), Error:IDL.Record({error_type:IDL.Int32,error_message:IDL.Text})}))});
  const encode = value => ({response_bytes:Buffer.from(IDL.encode([type],[value])).toString('hex')});
  assert.equal(decodeProposalResponse(encode({command:[{MakeProposal:{proposal_id:[{id:42n}]}}]})),'42');
  assert.throws(()=>decodeProposalResponse(encode({command:[{Error:{error_type:1,error_message:'denied'}}]})));
  assert.throws(()=>decodeProposalResponse({command:{MakeProposal:{proposal_id:{id:42}}}}));
});

test('handover preparation skips registered dapps and fixes exact upgrade bytes', async () => {
  const {prepareHandover,decodeChunkHash,decodeStoredChunks,decodeProposalResponse,decodeRootDapps,KINIC_GOVERNANCE,KINIC_ROOT} = await import('./handover.mjs');
  const {createHash} = await import('node:crypto');
  const wasm = Buffer.from([0,97,115,109,1,0,0,0]);
  const hash = createHash('sha256').update(wasm).digest('hex');
  const envelope = dapps => ({response_bytes:Buffer.from(IDL.encode([IDL.Record({dapps:IDL.Vec(IDL.Principal)})],[{dapps}])).toString('hex')});
  const absent = prepareHandover(envelope([]),bridge,wasm,hash);
  assert.equal(absent.governance_canister_id,KINIC_GOVERNANCE);
  assert.equal(absent.root_canister_id,KINIC_ROOT);
  assert.match(absent.registration_proposal,/RegisterDappCanisters/);
  assert.match(absent.registration_proposal,/co-controllers/);
  assert.match(absent.upgrade_proposal,/mode=opt \(3 : int32\)/);
  assert.equal(prepareHandover(envelope([Principal.fromText(bridge)]),bridge,wasm,hash).registration_proposal,null);
  assert.throws(()=>prepareHandover(envelope([]),bridge,wasm,'0'.repeat(64)));
  assert.throws(()=>prepareHandover(envelope([Principal.fromText(bridge),Principal.fromText(bridge)]),bridge,wasm,hash));
  assert.deepEqual(decodeRootDapps(envelope([Principal.fromText(bridge)])),[bridge]);
  const responseType = IDL.Record({command:IDL.Opt(IDL.Variant({MakeProposal:IDL.Record({proposal_id:IDL.Opt(IDL.Record({id:IDL.Nat64}))}),Error:IDL.Record({error_type:IDL.Int32,error_message:IDL.Text})}))});
  const response = value => ({response_bytes:Buffer.from(IDL.encode([responseType],[value])).toString('hex')});
  assert.equal(decodeProposalResponse(response({command:[{MakeProposal:{proposal_id:[{id:77n}]}}]})),'77');
  assert.throws(()=>decodeProposalResponse(response({command:[{Error:{error_type:1,error_message:'denied'}}]})));
  const chunkType = IDL.Record({hash:IDL.Vec(IDL.Nat8)});
  const chunkEnvelope = value => ({response_bytes:Buffer.from(IDL.encode([chunkType],[value])).toString('hex')});
  assert.equal(decodeChunkHash(chunkEnvelope({hash:[0x12,0x34]})),'1234');
  const storedType = IDL.Vec(chunkType);
  const stored = {response_bytes:Buffer.from(IDL.encode([storedType],[[{hash:[0x34]},{hash:[0x12]}]])).toString('hex')};
  assert.deepEqual(decodeStoredChunks(stored),['12','34']);
  assert.throws(()=>decodeChunkHash({hash:[0x12]}));
});

test('fee proposals pin payload and avoid active and reserved function IDs', async () => {
  const {prepareFeeProposal,proposalType,formatKinic,rawInteger,BRIDGE} = await import('./fees.mjs');
  const {createHash} = await import('node:crypto');
  const payload = {payout_id: 7n, amount: 123456789n, recipient:{owner:Principal.fromText(BRIDGE),subaccount:new Uint8Array(32).fill(5)}};
  const r = {functions:[{id:1001n,function_type:[]}],reserved_ids:[1000n,1002n]};
  const prepared = prepareFeeProposal(r,payload,100000n,8);
  assert.equal(prepared.function_id,'1003');
  assert.match(prepared.registration_proposal,/TreasuryAssetManagement/);
  assert.match(prepared.description,/1.23456789 KINIC \(123456789 raw units\)/);
  const bytes = Buffer.from(prepared.payload_hex,'hex');
  assert.deepEqual(IDL.decode([proposalType],bytes),[payload]);
  assert.equal(prepared.payload_sha256,createHash('sha256').update(bytes).digest('hex'));
  assert.equal(formatKinic((1n<<128n)-1n,8),'3402823669209384634633746074317.68211455');
  for (const value of ['-1','1.2','01','1e3',String(1n<<128n)]) assert.throws(()=>rawInteger(value));
  const g = {target_canister_id:[Principal.fromText(BRIDGE)],target_method_name:['sns_request_fee_payout'],validator_canister_id:[Principal.fromText(BRIDGE)],validator_method_name:['validate_sns_request_fee_payout'],topic:[{TreasuryAssetManagement:null}]};
  r.functions=[{id:1050n,function_type:[{GenericNervousSystemFunction:g}]}];
  assert.equal(prepareFeeProposal(r,payload,100000n,8).registration_proposal,null);
  g.topic=[{DappCanisterManagement:null}];
  assert.throws(()=>prepareFeeProposal(r,payload,100000n,8),/binding mismatch/);
  g.topic=[{TreasuryAssetManagement:null}];
  g.validator_method_name=['wrong'];
  assert.throws(()=>prepareFeeProposal(r,payload,100000n,8),/binding mismatch/);
  g.validator_method_name=['validate_sns_request_fee_payout'];
  r.reserved_ids.push(1050n);
  assert.throws(()=>prepareFeeProposal(r,payload,100000n,8),/binding mismatch/);
});

test('fee preparation checks capacity and continuation state without submitting', async t => {
  const {runFeeCommand,BRIDGE} = await import('./fees.mjs');
  const {readFileSync} = await import('node:fs');
  const directory = mkdtempSync(join(tmpdir(),'bridge-fee-prepare.'));
  t.after(()=>rmSync(directory,{recursive:true,force:true}));
  const recipient={owner:Principal.fromText(BRIDGE),subaccount:[]};
  const status={fee_reserve:110001n,pending_payout_debit:1n,ledger_fee:100000n,max_payout_amount:10000n,fee_recipient:recipient,next_fee_payout_id:9n};
  let payout={id:8n,amount:100n,recipient,ledger_fee:100000n,state:{ReconciliationHold:null}};
  const clients={bridge:{get_fee_status:async()=>({Ok:status}),get_fee_payout:async()=>({Ok:[payout]})},ledger:{icrc1_decimals:async()=>8},governance:{list_nervous_system_functions:async()=>({functions:[],reserved_ids:[1000n]})}};
  assert.equal((await runFeeCommand(['fee-status'],clients)).max_payout_kinic,'0.00010000');
  const output=join(directory,'request.json');
  const result=await runFeeCommand(['prepare-fee-payout','10000',output],clients);
  assert.equal(result.payload.payout_id,9n);
  assert.equal(JSON.parse(readFileSync(output,'utf8')).payload.amount,'10000');
  await assert.rejects(runFeeCommand(['prepare-fee-payout','10001',join(directory,'over.json')],clients));
  await assert.rejects(runFeeCommand(['prepare-fee-payout','0',join(directory,'zero.json')],clients));
  await assert.rejects(runFeeCommand(['prepare-fee-payout','1',output],clients),/EEXIST/);
  const resumed=await runFeeCommand(['prepare-continue-fee-payout','8',join(directory,'resume.json')],clients);
  assert.equal(resumed.target_method_name,'sns_continue_fee_payout');
  assert.equal(resumed.payload.payout_id,8n);
  assert.equal(resumed.payload.amount,100n);
  payout={...payout,state:{Succeeded:{block_index:42n}}};
  assert.equal((await runFeeCommand(['fee-payout-status','8'],clients)).state.Succeeded.block_index,42n);
  await assert.rejects(runFeeCommand(['prepare-continue-fee-payout','8',join(directory,'done.json')],clients));
  clients.bridge.get_fee_status=async()=>({Err:{StorageFailure:null}});
  await assert.rejects(runFeeCommand(['fee-status'],clients),/rejected query/);
});


test('fee CLI starts and rejects invalid arguments without network access', () => {
  const script = fileURLToPath(new URL('./fees.mjs', import.meta.url));
  for (const command of ['fee-status', 'fee-payout-status', 'prepare-fee-payout', 'prepare-continue-fee-payout']) {
    const result = spawnSync(process.execPath, [script, command, ...(command === 'fee-status' ? ['extra'] : [])], {
      encoding: 'utf8', timeout: 10_000,
    });
    assert.ifError(result.error);
    assert.equal(result.status, 1, `${command} must run the CLI argument validation`);
    assert.match(result.stderr, /Usage: fees\.mjs/);
    assert.equal(result.stdout, '');
  }
});
