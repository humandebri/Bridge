import { test } from 'node:test';
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
