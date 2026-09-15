import { IDL } from '@icp-sdk/core/candid';
import { Principal } from '@icp-sdk/core/principal';
import { readFileSync, writeFileSync } from 'node:fs';
import { createHash } from 'node:crypto';
import { pathToFileURL } from 'node:url';

export const payloadType = IDL.Record({ previous_governance_operation_id: IDL.Nat64 });
const genericType = IDL.Record({
  target_canister_id: IDL.Opt(IDL.Principal), target_method_name: IDL.Opt(IDL.Text),
  validator_canister_id: IDL.Opt(IDL.Principal), validator_method_name: IDL.Opt(IDL.Text),
});
const registryType = IDL.Record({
  reserved_ids: IDL.Vec(IDL.Nat64),
  functions: IDL.Vec(IDL.Record({ id: IDL.Nat64, function_type: IDL.Opt(IDL.Variant({
    NativeNervousSystemFunction: IDL.Reserved, GenericNervousSystemFunction: genericType,
  })) })),
});
export function decodeRegistry(envelope) {
  if (!/^(?:[0-9a-f]{2})+$/i.test(envelope.response_bytes ?? '')) throw new Error('Missing raw registry Candid');
  return IDL.decode([registryType], Uint8Array.from(Buffer.from(envelope.response_bytes, 'hex')).buffer)[0];
}
export function decodeProposalResponse(envelope) {
  if (!/^(?:[0-9a-f]{2})+$/i.test(envelope.response_bytes ?? '')) throw new Error('Missing raw proposal Candid');
  const type = IDL.Record({ command: IDL.Opt(IDL.Variant({
    MakeProposal: IDL.Record({ proposal_id: IDL.Opt(IDL.Record({ id: IDL.Nat64 })) }),
    Error: IDL.Record({ error_type: IDL.Int32, error_message: IDL.Text }),
  })) });
  const result = IDL.decode([type], Uint8Array.from(Buffer.from(envelope.response_bytes, 'hex')).buffer)[0];
  const id = result.command[0]?.MakeProposal?.proposal_id[0]?.id;
  if (!id || id <= 0n) throw new Error('SNS response did not accept a proposal');
  return String(id);
}
const blob = bytes => 'blob "' + [...bytes].map(x => '\\' + x.toString(16).padStart(2, '0')).join('') + '"';
const digest = bytes => createHash('sha256').update(bytes).digest('hex');
export function prepare(registry, bridgeText, previousText) {
  const bridge = Principal.fromText(bridgeText).toText();
  if (bridge === '2vxsx-fae' || bridge === 'aaaaa-aa') throw new Error('Invalid Bridge principal');
  if (!/^(0|[1-9][0-9]*)$/.test(previousText)) throw new Error('Invalid previous operation');
  const previous = BigInt(previousText);
  if (previous > 18446744073709551615n) throw new Error('Previous operation exceeds nat64');
  const payload = new Uint8Array(IDL.encode([payloadType], [{ previous_governance_operation_id: previous }]));
  if (new Set(registry.functions.map(f => String(f.id))).size !== registry.functions.length) throw new Error('Duplicate function IDs');
  const used = new Set([...registry.reserved_ids, ...registry.functions.map(f => f.id)].map(String));
  return ['schedule', 'execute'].map(phase => {
    const method = `sns_${phase}_activation`, validator = `validate_${method}`;
    const candidates = registry.functions.filter(f => {
      const g = f.function_type[0]?.GenericNervousSystemFunction;
      return g?.target_canister_id[0]?.toText() === bridge && g.target_method_name[0] === method;
    });
    if (candidates.length > 1) throw new Error('Duplicate activation functions');
    let id, registration = null;
    if (candidates.length) {
      const g = candidates[0].function_type[0].GenericNervousSystemFunction;
      if (g.validator_canister_id[0]?.toText() !== bridge || g.validator_method_name[0] !== validator
          || registry.reserved_ids.some(id => id === candidates[0].id)) throw new Error('Validator or reserved ID mismatch');
      id = candidates[0].id;
    } else {
      id = 1000n; while (used.has(String(id))) id++;
      used.add(String(id));
      registration = `record { title = "Enable KINIC Bridge ${phase} reactivation proposals"; url = ""; summary = "Register the dedicated validator and execution method for DAO reactivation. This proposal does not restart the Bridge or transfer controller authority."; action = opt variant { AddGenericNervousSystemFunction = record { id = ${id} : nat64; name = "KINIC Bridge ${phase} reactivation"; description = opt "Reactivation after initial activation, bound to the previous confirmed operation."; function_type = opt variant { GenericNervousSystemFunction = record { topic = opt variant { DappCanisterManagement }; target_canister_id = opt principal "${bridge}"; target_method_name = opt "${method}"; validator_canister_id = opt principal "${bridge}"; validator_method_name = opt "${validator}" } } } } }`;
    }
    return { phase, function_id: String(id), target_canister_id: bridge, target_method_name: method,
      validator_canister_id: bridge, validator_method_name: validator, registration_proposal: registration,
      previous_governance_operation_id: previousText, payload_hex: Buffer.from(payload).toString('hex'),
      payload_sha256: digest(payload),
      execution_proposal: `record { title = "${phase === 'schedule' ? 'Schedule' : 'Execute'} KINIC Bridge reactivation"; url = ""; summary = "${phase === 'schedule' ? 'Schedule the 24-hour Timelock' : 'Execute the scheduled Timelock'} after confirmed governance operation ${previous}. Base relay and Finalized confirmation are separate completion requirements."; action = opt variant { ExecuteGenericNervousSystemFunction = record { function_id = ${id} : nat64; payload = ${blob(payload)} } } }`,
    };
  });
}
if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  if (process.argv[2] === 'decode-response') {
    if (process.argv.length !== 4) throw new Error('Usage: prepare.mjs decode-response RESPONSE_JSON');
    console.log(decodeProposalResponse(JSON.parse(readFileSync(process.argv[3]))));
    process.exit(0);
  }
  const [registryPath, bridge, previous, output] = process.argv.slice(2);
  if (!output || process.argv.length !== 6) throw new Error('Usage: prepare.mjs REGISTRY_JSON BRIDGE PREVIOUS_OPERATION OUTPUT_JSON');
  const raw = readFileSync(registryPath);
  const result = { schema_version: 1, governance_canister_id: '74ncn-fqaaa-aaaaq-aaasa-cai',
    registry_sha256: digest(raw), proposals: prepare(decodeRegistry(JSON.parse(raw)), bridge, previous) };
  writeFileSync(output, JSON.stringify(result, null, 2) + '\n', { flag: 'wx', mode: 0o600 });
}
