import { IDL } from '@icp-sdk/core/candid';
import { Principal } from '@icp-sdk/core/principal';
import { createHash } from 'node:crypto';
import { readFileSync, writeFileSync, mkdirSync } from 'node:fs';
import { resolve } from 'node:path';
import { pathToFileURL } from 'node:url';
import { isDeepStrictEqual } from 'node:util';
const sha256 = bytes => createHash('sha256').update(bytes).digest('hex');
const blob = bytes => 'blob "' + [...bytes].map(x => '\\' + x.toString(16).padStart(2, '0')).join('') + '"';
export const KINIC_GOVERNANCE = '74ncn-fqaaa-aaaaq-aaasa-cai';
export const KINIC_ROOT = '7jkta-eyaaa-aaaaq-aaarq-cai';
const proposalResponseType = IDL.Record({command: IDL.Opt(IDL.Variant({
  MakeProposal: IDL.Record({proposal_id: IDL.Opt(IDL.Record({id:IDL.Nat64}))}),
  Error: IDL.Record({error_type:IDL.Int32,error_message:IDL.Text}),
}))});
const rootResponseType = IDL.Record({dapps:IDL.Vec(IDL.Principal)});
const chunkHashType = IDL.Record({hash:IDL.Vec(IDL.Nat8)});
const storedChunksType = IDL.Vec(chunkHashType);
const responseBytes = envelope => {
  if (!/^(?:[0-9a-f]{2})+$/i.test(envelope?.response_bytes ?? '')) throw new Error('Missing raw Candid response');
  return Uint8Array.from(Buffer.from(envelope.response_bytes,'hex')).buffer;
};
export function decodeRootDapps(envelope) {
  if (!/^(?:[0-9a-f]{2})+$/i.test(envelope?.response_bytes ?? '')) throw new Error('Missing Root raw Candid');
  const [root] = IDL.decode([rootResponseType], Uint8Array.from(Buffer.from(envelope.response_bytes,'hex')).buffer);
  return root.dapps.map(id => id.toText());
}
export function decodeProposalResponse(envelope) {
  if (!/^(?:[0-9a-f]{2})+$/i.test(envelope?.response_bytes ?? '')) throw new Error('Missing Governance raw Candid');
  const [response] = IDL.decode([proposalResponseType], Uint8Array.from(Buffer.from(envelope.response_bytes,'hex')).buffer);
  const id = response.command[0]?.MakeProposal?.proposal_id[0]?.id;
  if (typeof id !== 'bigint' || id <= 0n) throw new Error('Governance response has no proposal ID');
  return id.toString();
}
export function decodeChunkHash(envelope) {
  const [response] = IDL.decode([chunkHashType], responseBytes(envelope));
  return Buffer.from(response.hash).toString('hex');
}
export function decodeStoredChunks(envelope) {
  const [response] = IDL.decode([storedChunksType], responseBytes(envelope));
  return response.map(chunk => Buffer.from(chunk.hash).toString('hex')).sort();
}
export function registrationProposal(bridgeText) {
  const bridge = Principal.fromText(bridgeText).toText();
  return `record {title="Register KINIC Bridge with SNS"; url=""; summary="## Overview\\n\\nRegister the production KINIC Bridge as an SNS dapp after verifying that the production identity and SNS Root are the exact co-controllers.\\n\\n## Changes\\n\\nSuccessful execution makes SNS Root the sole controller. The production identity will no longer be able to upgrade the Bridge directly."; action=opt variant {RegisterDappCanisters=record {canister_ids=vec {principal "${bridge}"}}}}`;
}
export function upgradeProposal(bridgeText, storeText, wasm, expectedHash) {
  const bridge = Principal.fromText(bridgeText).toText();
  const store = Principal.fromText(storeText).toText();
  if (!/^[0-9a-f]{64}$/.test(expectedHash) || sha256(wasm) !== expectedHash) throw new Error('Wasm differs from reviewed hash');
  const chunks = [];
  for (let offset = 0; offset < wasm.length; offset += 1_000_000) {
    chunks.push(sha256(wasm.subarray(offset, offset + 1_000_000)));
  }
  const chunked = `opt record {store_canister_id=opt principal "${store}"; wasm_module_hash=${blob(Buffer.from(expectedHash,'hex'))}; chunk_hashes_list=vec {${chunks.map(hash=>blob(Buffer.from(hash,'hex'))).join(';')}}}`;
  return `record {title="Verify DAO upgrade of KINIC Bridge"; url=""; summary="## Overview\\n\\nUpgrade the registered KINIC Bridge with the exact reviewed Wasm (${expectedHash}) through SNS Governance.\\n\\n## Changes\\n\\nReinstall no state; use upgrade mode with empty Candid arguments and verify the Root post-upgrade observation."; action=opt variant {UpgradeSnsControlledCanister=record {canister_id=opt principal "${bridge}"; new_canister_wasm=blob ""; mode=opt (3 : int32); canister_upgrade_arg=opt ${blob([68,73,68,76,0,0])}; canister_upgrade_options=null; chunked_canister_wasm=${chunked}}}}`;
}
export function prepareRootUpgrade(bridgeText, storeText, wasm, currentHash, candidateHash) {
  const bridge = Principal.fromText(bridgeText).toText();
  const store = Principal.fromText(storeText).toText();
  if ([bridge, store].some(value => value === 'aaaaa-aa' || value === '2vxsx-fae') || bridge === store)
    throw new Error('A separate valid chunk store is required');
  if (wasm.length > 128 * 1024 * 1024 || !/^[0-9a-f]{64}$/.test(currentHash) || !/^[0-9a-f]{64}$/.test(candidateHash)
      || currentHash === candidateHash || sha256(wasm) !== candidateHash
      || !Buffer.from(wasm).subarray(0,8).equals(Buffer.from([0,97,115,109,1,0,0,0])))
    throw new Error('Current or candidate Wasm binding is invalid');
  const chunks = [];
  for (let offset = 0; offset < wasm.length; offset += 1_000_000) {
    const bytes = wasm.subarray(offset, offset + 1_000_000);
    chunks.push({index:chunks.length,sha256:sha256(bytes),size:bytes.length});
  }
  return {schema_version:1, bridge_canister_id:bridge, store_canister_id:store,
    current_wasm_sha256:currentHash, candidate_wasm_sha256:candidateHash, chunks,
    upgrade_proposal:upgradeProposal(bridge,store,wasm,candidateHash)};
}
export function verifyRootUpgradeChunks(prepared, bridge, store, wasm, currentHash, candidateHash, storedHashes) {
  const expected = prepareRootUpgrade(bridge,store,wasm,currentHash,candidateHash);
  if (!isDeepStrictEqual(prepared, expected)) throw new Error('Prepared proposal or chunk ordering differs from reviewed candidate');
  if (!Array.isArray(storedHashes) || storedHashes.some(hash => typeof hash !== 'string' || !/^[0-9a-f]{64}$/.test(hash)))
    throw new Error('Stored chunk hashes are invalid');
  const available = new Set(storedHashes);
  if (expected.chunks.some(chunk => !available.has(chunk.sha256))) throw new Error('Required Wasm chunk is missing');
  return true;
}
export function prepareHandover(envelope, bridgeText, wasm, expectedHash) {
  const bridge = Principal.fromText(bridgeText).toText();
  if (bridge === 'aaaaa-aa' || bridge === '2vxsx-fae') throw new Error('Invalid Bridge principal');
  if (!/^(?:[0-9a-f]{2})+$/i.test(envelope.response_bytes ?? '')) throw new Error('Missing Root raw Candid');
  const dapps = decodeRootDapps(envelope);
  const count = dapps.filter(id => id === bridge).length;
  if (count > 1) throw new Error('Duplicate Root registration');
  if (!/^[0-9a-f]{64}$/.test(expectedHash) || sha256(wasm) !== expectedHash || !Buffer.from(wasm).subarray(0,8).equals(Buffer.from([0,97,115,109,1,0,0,0])))
    throw new Error('Wasm differs from the reviewed uncompressed module');
  const chunks = [];
  for (let offset = 0; offset < wasm.length; offset += 1_000_000) {
    const bytes = wasm.subarray(offset, offset + 1_000_000);
    chunks.push({index:chunks.length,sha256:sha256(bytes),size:bytes.length});
  }
  return { chunks, upload_before_handover:true, governance_canister_id:KINIC_GOVERNANCE, root_canister_id:KINIC_ROOT,
    store_canister_id:bridge, bridge_canister_id: bridge, wasm_sha256: expectedHash, already_registered: count === 1,
    registration_proposal: count ? null : registrationProposal(bridge),
    upgrade_proposal: upgradeProposal(bridge, bridge, wasm, expectedHash),
  };
}
if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  if (process.argv[2] === 'decode-response') {
    if (process.argv.length !== 4) throw new Error('Usage: handover.mjs decode-response RESPONSE_JSON');
    console.log(decodeProposalResponse(JSON.parse(readFileSync(process.argv[3]))));
    process.exit(0);
  }
  if (process.argv[2] === 'decode-root') {
    if (process.argv.length !== 4) throw new Error('Usage: handover.mjs decode-root RESPONSE_JSON');
    console.log(JSON.stringify(decodeRootDapps(JSON.parse(readFileSync(process.argv[3])))));
    process.exit(0);
  }
  if (process.argv[2] === 'decode-chunk') {
    if (process.argv.length !== 4) throw new Error('Usage: handover.mjs decode-chunk RESPONSE_JSON');
    console.log(decodeChunkHash(JSON.parse(readFileSync(process.argv[3]))));
    process.exit(0);
  }
  if (process.argv[2] === 'decode-stored-chunks') {
    if (process.argv.length !== 4) throw new Error('Usage: handover.mjs decode-stored-chunks RESPONSE_JSON');
    console.log(JSON.stringify(decodeStoredChunks(JSON.parse(readFileSync(process.argv[3])))));
    process.exit(0);
  }
  if (process.argv[2] === 'registration-payload') {
    if (process.argv.length !== 4) throw new Error('Usage: handover.mjs registration-payload BRIDGE');
    console.log(registrationProposal(process.argv[3]));
    process.exit(0);
  }
  if (process.argv[2] === 'upgrade-payload') {
    if (process.argv.length !== 6) throw new Error('Usage: handover.mjs upgrade-payload BRIDGE WASM SHA256');
    console.log(upgradeProposal(process.argv[3],process.argv[3],readFileSync(process.argv[4]),process.argv[5]));
    process.exit(0);
  }
  if (process.argv[2] === 'root-upgrade-prepare') {
    if (process.argv.length !== 8) throw new Error('Usage: handover.mjs root-upgrade-prepare BRIDGE STORE WASM CURRENT_SHA256 CANDIDATE_SHA256');
    console.log(JSON.stringify(prepareRootUpgrade(process.argv[3],process.argv[4],readFileSync(process.argv[5]),process.argv[6],process.argv[7]),null,2));
    process.exit(0);
  }
  if (process.argv[2] === 'root-upgrade-verify-chunks') {
    if (process.argv.length !== 10) throw new Error('Usage: handover.mjs root-upgrade-verify-chunks PREPARED_JSON BRIDGE STORE WASM CURRENT_SHA256 CANDIDATE_SHA256 STORED_CHUNKS_JSON');
    const prepared = JSON.parse(readFileSync(process.argv[3]));
    const stored = decodeStoredChunks(JSON.parse(readFileSync(process.argv[9])));
    verifyRootUpgradeChunks(prepared,process.argv[4],process.argv[5],readFileSync(process.argv[6]),process.argv[7],process.argv[8],stored);
    console.log('chunk_membership_and_proposal=verified; live authority, subnet, current state and submission remain unverified');
    process.exit(0);
  }
  const [rootPath, bridge, wasmPath, expectedHash, output] = process.argv.slice(2);
  if (!output || process.argv.length !== 7) throw new Error('Usage: handover.mjs ROOT_RESPONSE_JSON BRIDGE WASM EXPECTED_SHA256 OUTPUT_JSON');
  const raw = readFileSync(rootPath);
  const wasm = readFileSync(wasmPath);
  const prepared = prepareHandover(JSON.parse(raw),bridge,wasm,expectedHash);
  const directory = resolve(output + '.chunks');
  mkdirSync(directory, {mode:0o700});
  for (const chunk of prepared.chunks) {
    const name = `chunk-${String(chunk.index).padStart(3,'0')}.bin`;
    writeFileSync(resolve(directory,name),wasm.subarray(chunk.index*1_000_000,chunk.index*1_000_000+chunk.size),{flag:'wx',mode:0o400});
    chunk.file = name;
  }
  writeFileSync(output, JSON.stringify({schema_version:1, root_response_sha256:sha256(raw), chunks_directory:directory, ...prepared},null,2)+'\n',{flag:'wx',mode:0o600});
}
