import { IDL } from '@icp-sdk/core/candid';
import { Principal } from '@icp-sdk/core/principal';
import { createHash } from 'node:crypto';
import { readFileSync, writeFileSync, mkdirSync } from 'node:fs';
import { resolve } from 'node:path';
import { pathToFileURL } from 'node:url';
const sha256 = bytes => createHash('sha256').update(bytes).digest('hex');
const blob = bytes => 'blob "' + [...bytes].map(x => '\\' + x.toString(16).padStart(2, '0')).join('') + '"';
export function prepareHandover(envelope, bridgeText, wasm, expectedHash) {
  const bridge = Principal.fromText(bridgeText).toText();
  if (bridge === 'aaaaa-aa' || bridge === '2vxsx-fae') throw new Error('Invalid Bridge principal');
  if (!/^(?:[0-9a-f]{2})+$/i.test(envelope.response_bytes ?? '')) throw new Error('Missing Root raw Candid');
  const [root] = IDL.decode([IDL.Record({dapps: IDL.Vec(IDL.Principal)})], Uint8Array.from(Buffer.from(envelope.response_bytes, 'hex')).buffer);
  const count = root.dapps.filter(id => id.toText() === bridge).length;
  if (count > 1) throw new Error('Duplicate Root registration');
  if (!/^[0-9a-f]{64}$/.test(expectedHash) || sha256(wasm) !== expectedHash || !Buffer.from(wasm).subarray(0,8).equals(Buffer.from([0,97,115,109,1,0,0,0])))
    throw new Error('Wasm differs from the reviewed uncompressed module');
  const chunks = [];
  for (let offset = 0; offset < wasm.length; offset += 1_000_000) {
    const bytes = wasm.subarray(offset, offset + 1_000_000);
    chunks.push({index:chunks.length,sha256:sha256(bytes),size:bytes.length});
  }
  const chunked = `opt record {store_canister_id=opt principal "${bridge}"; wasm_module_hash=${blob(Buffer.from(expectedHash,'hex'))}; chunk_hashes_list=vec {${chunks.map(c=>blob(Buffer.from(c.sha256,'hex'))).join(';')}}`;
  return { chunks, upload_before_handover:true, store_canister_id:bridge, bridge_canister_id: bridge, wasm_sha256: expectedHash, already_registered: count === 1,
    registration_proposal: count ? null : `record {title="Register KINIC Bridge with SNS"; url=""; summary="Register the Bridge after the separately approved transfer to SNS Root. Production registration removes other controllers."; action=opt variant {RegisterDappCanisters=record {canister_ids=vec {principal "${bridge}"}}}}`,
    upgrade_proposal: `record {title="Verify DAO upgrade of KINIC Bridge"; url=""; summary="Upgrade the registered Bridge with the exact same reviewed Wasm (${expectedHash}). Verify schema, runtime, storage, pending operations and asset flows after execution."; action=opt variant {UpgradeSnsControlledCanister=record {canister_id=opt principal "${bridge}"; new_canister_wasm=blob ""; mode=opt (3 : int32); canister_upgrade_arg=opt ${blob([68,73,68,76,0,0])}; canister_upgrade_options=null; chunked_canister_wasm=${chunked}}}}`,
  };
}
if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
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
