import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { IDL } from "@icp-sdk/core/candid";
import { Principal } from "@icp-sdk/core/principal";
import { PocketIc } from "@dfinity/pic";

// Bindings and Wasm come from the hash-pinned official archive, never a mock SNS.
export async function setupRealSns(pic: PocketIc) {
  const directory = process.env.BRIDGE_SNS_TEST_RUNTIME ?? resolve(__dirname, "../.tools/sns-test-runtime");
  const hashes: Record<string, string> = {
    "sns-governance-canister.wasm": "ad253d7026c7bdc42f42b88dbea489e971120db8ef6163ad5a66d434d7ce5709",
    "sns-root-canister.wasm": "433787a2a35360816e49b541aec59e82d44e9942ebf9278082626deb97006d23",
    "sns-governance-canister.cjs": "453c7c12a964f0517d7657edc64f419d47dcd5e8f064fb8bc1ec1aa67bdd597d",
    "sns-root-canister.cjs": "14565b1c646a6c84d4e59cefd2dcc36773309d6e0a08f80cb6321e48a3fa54e2",
  };
  for (const [name, expected] of Object.entries(hashes)) {
    if (createHash("sha256").update(readFileSync(resolve(directory, name))).digest("hex") !== expected)
      throw new Error(`SNS runtime differs from the pinned official release: ${name}`);
  }
  const governanceBindings = require(resolve(directory, "sns-governance-canister.cjs"));
  const rootBindings = require(resolve(directory, "sns-root-canister.cjs"));
  const governanceId = await pic.createCanister({ cycles: 500_000_000_000_000n });
  const rootId = await pic.createCanister({ cycles: 500_000_000_000_000n });
  const ledgerId = await pic.createCanister();
  const swapId = await pic.createCanister();
  const indexId = await pic.createCanister();
  const proposer = Principal.selfAuthenticating(new Uint8Array(32).fill(201));
  const neuronId = new Uint8Array(32).fill(202);
  const minority = Principal.selfAuthenticating(new Uint8Array(32).fill(203));
  const minorityNeuronId = new Uint8Array(32).fill(204);
  const governanceType: any = governanceBindings.init({ IDL })[0];
  const parametersType = governanceType._fields.find(([key]: any) => key === "parameters")[1]._type;
  const paramsRaw = JSON.parse(readFileSync(resolve(__dirname, "fixtures/sns-parameters.json"), "utf8"));
  const parameters: any = IDL.decode([parametersType], Uint8Array.from(Buffer.from(paramsRaw.response_bytes, "hex")))[0];
  const now = BigInt(Math.floor(await pic.getTime() / 1000));
  const neuron = { id: [{ id: neuronId }], permissions: [{ principal: [proposer], permission_type: [1,2,3,4,5,6,7,8,9,10] }],
    staked_maturity_e8s_equivalent: [], maturity_e8s_equivalent: 0n, cached_neuron_stake_e8s: 1_000_000_000_000n,
    created_timestamp_seconds: now, source_nns_neuron_id: [], auto_stake_maturity: [], aging_since_timestamp_seconds: now,
    dissolve_state: [{ DissolveDelaySeconds: parameters.max_dissolve_delay_seconds[0] }], voting_power_percentage_multiplier: 100n,
    vesting_period_seconds: [], disburse_maturity_in_progress: [], followees: [], topic_followees: [], neuron_fees_e8s: 0n };
  const state = { root_canister_id: [rootId], ledger_canister_id: [ledgerId], swap_canister_id: [swapId], mode: 1,
    parameters: [parameters], sns_initialization_parameters: "Local Bridge DAO integration", genesis_timestamp_seconds: now,
    sns_metadata: [{ url: ["https://kinic.io"], name: ["Bridge test SNS"], description: ["Local SNS integration test for the KINIC Bridge."], logo: [] }],
    neurons: [[Buffer.from(neuronId).toString("hex"), neuron], [Buffer.from(minorityNeuronId).toString("hex"), {...neuron,
      id:[{id:minorityNeuronId}],cached_neuron_stake_e8s:1_000_000_000n,permissions:[{principal:[minority],permission_type:[1,2,3,4,5,6,7,8,9,10]}]}]], id_to_nervous_system_functions: [], proposals: [], in_flight_commands: [],
    metrics: [], maturity_modulation: [], is_finalizing_disburse_maturity: [], deployed_version: [], cached_upgrade_steps: [], latest_reward_event: [],
    pending_version: [], target_version: [], timers: [], upgrade_journal: [] };
  await pic.installCode({ canisterId: governanceId, wasm: readFileSync(resolve(directory,"sns-governance-canister.wasm")),
    arg: IDL.encode([governanceType], [state]) });
  await pic.installCode({ canisterId: rootId, wasm: readFileSync(resolve(directory,"sns-root-canister.wasm")),
    arg: IDL.encode(rootBindings.init({IDL}), [{ governance_canister_id: [governanceId], ledger_canister_id: [ledgerId],
      swap_canister_id: [swapId], index_canister_id: [indexId], archive_canister_ids: [], dapp_canister_ids: [], extensions: [], timers: [], testflight: false }]) });
  const governance: any = pic.createActor(governanceBindings.idlFactory, governanceId);
  governance.setPrincipal(proposer);
  const snsRoot: any = pic.createActor(rootBindings.idlFactory, rootId);
  async function propose(action: any) {
    const result = await governance.manage_neuron({ subaccount: neuronId, command: [{ MakeProposal: {
      title: "Bridge DAO integration operation", url: "", summary: "Local execution verification.", action: [action] } }] });
    if (!result.command[0]?.MakeProposal) throw new Error(`Proposal rejected: ${JSON.stringify(result, (_,v)=>typeof v === 'bigint' ? String(v):v)}`);
    const id = result.command[0].MakeProposal.proposal_id[0];
    for (let attempt = 0; attempt < 40; attempt++) {
      await pic.tick(5);
      const response = await governance.get_proposal({ proposal_id: [id] });
      const proposal = response.result[0].Proposal;
      if (proposal.failed_timestamp_seconds !== 0n) throw new Error(JSON.stringify(proposal.failure_reason));
      if (proposal.executed_timestamp_seconds !== 0n) return proposal;
    }
    throw new Error("SNS proposal did not finish executing");
  }
  async function submitUnadopted(action: any) {
    governance.setPrincipal(minority);
    try {
      const response = await governance.manage_neuron({subaccount:minorityNeuronId,command:[{MakeProposal:{
        title:"Unadopted Bridge operation",url:"",summary:"Verify that minority submission cannot execute.",action:[action]}}]});
      const id = response.command[0]?.MakeProposal?.proposal_id[0];
      if (!id) throw new Error("Minority proposal was not accepted");
      await pic.tick(5);
      return (await governance.get_proposal({proposal_id:[id]})).result[0].Proposal;
    } finally { governance.setPrincipal(proposer); }
  }
  return { governanceId, rootId, governance, snsRoot, propose, submitUnadopted };
}
