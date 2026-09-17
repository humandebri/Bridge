# Live Base Sepolia rehearsal through the EVM RPC Canister

This runbook defines test-only live Bridge rehearsals through the official EVM RPC Canister and evidence creation.
Existing `base-sepolia-experiment` is contract-only and cannot provide evidence for this rehearsal.

Normal CI makes no external calls or transactions.
CI checks only rehearsal recorder tests, binding to the official Canister ID, and absence of prohibited local test-backend references.

## Guarantee boundaries

[ADR 0024](../adr/0024-validate-rpc-chain-binding-before-runtime.md) is authoritative for pre-runtime chain-binding validation and runtime quorum as protection against response disagreement/provider failure.

- Fix networks to IC mainnet and Base Sepolia, chain ID `84532`.
- Fix EVM RPC Canister to DFINITY-managed `7hfb6-caaaa-aaaar-qadga-cai`.
- Specify three credential-free HTTPS Custom RPC URLs, rejecting only duplicate URL strings.
- Custom RPC URLs, expected chain ID, and each upstream chain remain immutable during operation. Before deployment/activation, call `eth_chainId` on all three URLs and require complete agreement.
- Do not audit provider operators, upstreams, ASN, cloud, region, failure domains, or availability.
- Runtime 2-of-3 quorum handles disagreement and provider failures; it does not detect runtime upstream-chain switching.
- Retain as an external assumption that the EVM RPC Canister and configured provider quorum correctly return the canonical Finalized Base Sepolia chain.
- Existing PocketIC tests handle deterministic checks for orphan receipts, same-height hash disagreement, and incorrect provider responses. Fault injection into real public RPC is not a production approval condition.

For primary sources on official Canister ID/interfaces, see DFINITY [EVM RPC documentation](https://internetcomputer.org/docs/references/evm-rpc-canister) and the [EVM RPC canister repository](https://github.com/dfinity/evm-rpc-canister). Network fetching is not a CI prerequisite.

## Required external inputs

Do not start if any input below is missing.

- A test Bridge Canister on IC and an already-deployed shared `testicrc` Canister.
- Confirmation that no staging-only Ledger/Index Canisters were newly created.
- An initially paused Base Sepolia-only Bridge.
- Sufficient Bridge Canister cycles.
- Test Ledger balance and Base Sepolia ETH.
- A Base Bridge Signer matching the chain-key signer.
- Three credential-free public HTTPS RPC URLs.
- Authentication for the test principal.
- SHA-256 for Bridge Canister Wasm and Bridge runtime bytecode from the same build as the production candidate.
- A recording mechanism that removes secrets before hashing each operation's request/response with SHA-256.

Store no production keys, seeds, private keys, hardware-wallet backups, passwords, credential-bearing URLs, or raw authorization headers in configuration, evidence, or shell arguments.

## Initialization and preflight

Copy the template to a working directory and replace every placeholder with actual test values.

```sh
cp deployments/evidence-templates/evm-rpc-rehearsal-config.template.json \
  /secure/work/rehearsal-config.json
python3 scripts/evm-rpc-rehearsal/rehearsal.py \
  validate-config /secure/work/rehearsal-config.json
python3 scripts/evm-rpc-rehearsal/rehearsal.py \
  init /secure/work/rehearsal-config.json /secure/work/rpc-e2e.json
```

`validate-config` checks IC network, Base Sepolia, official EVM RPC Canister, three secret-free HTTPS URLs, test-only bindings, and Bridge Canister Wasm/runtime bytecode SHA-256.
Output reduces URLs to host and SHA-256; manifests contain no complete URLs.

Before external calls, reread live Bridge Canister chain/canister/contract/RPC configuration, chain-key signer, Bridge Signer at the same Finalized Base block, both-direction pause, cycles, and test ETH.

Preflight evidence `details` must contain exactly the following fields.

```json
{
  "observed_chain_id": 84532,
  "observed_evm_rpc_canister_id": "7hfb6-caaaa-aaaar-qadga-cai",
  "observed_bridge_contract": "0x...",
  "base_bridge_signer": "0x...",
  "canister_chain_key_signer": "0x...",
  "deposits_paused": true,
  "withdrawals_paused": true,
  "cycles_balance": 10000000000,
  "base_sepolia_eth_balance_wei": 1,
  "configured_rpc_url_sha256": ["...", "...", "..."]
}
```

Before manually entering observations, capture ICP CLI and `cast` JSON output as raw artifacts through the fixed driver.
The driver executes commands without a shell, saving argv, exit status, raw stdout, stdout digest, and parsed JSON in one artifact.

```sh
python3 scripts/evm-rpc-rehearsal/rehearsal.py capture-artifact \
  /secure/work/rpc-e2e.json /secure/work/rehearsal-config.json preflight bridge \
  /secure/work/artifacts/preflight-bridge.json none -- \
  icp canister call <bridge-canister-id> get_runtime_binding '()' \
  -n ic --query --json

python3 scripts/evm-rpc-rehearsal/rehearsal.py capture-artifact \
  /secure/work/rpc-e2e.json /secure/work/rehearsal-config.json canonical_receipt base \
  /secure/work/artifacts/canonical-receipt-base.json 0 -- \
  cast receipt <transaction-hash>
```

`bridge` and `audit` artifacts allow only the test Bridge Canister; `ledger` only the configured test Ledger; `base` only Base Sepolia receipts/blocks/calls.
Reject local backends, test doubles, other Canisters/networks, non-JSON output, and failed commands.
Base capture endpoints come only from reviewed configuration `rpc_urls[provider-index]`. Before the actual call, the driver runs `cast chain-id` against that endpoint and verifies `84532`.
Artifacts omit complete URLs and retain provider index, URL SHA-256, chain-id response, method, and parameters. Reject environment overrides such as `ETH_RPC_URL`, command-level `--rpc-url`, `--chain`, `--json`, and duplicate network flags.
Use actual Candid method `get_bridge_status` for Bridge state; reject old `get_status`.

For failure scenarios, never use handwritten JSON. Place the fixed-name `evm-rpc-fault-injector` on PATH; it receives reviewed configuration on stdin, applies faults, runs the scenario, restores all providers, and returns JSON. The recorder deterministically binds URL digests, faulty provider indexes, failure rules, execution interval, and injector output.

```sh
python3 scripts/evm-rpc-rehearsal/rehearsal.py capture-fault \
  /secure/work/rpc-e2e.json /secure/work/rehearsal-config.json single_provider_failure \
  /secure/work/artifacts/single-provider-fault.json fault-one-provider -- \
  evm-rpc-fault-injector
```

The injector returns `schema_version`, `run_reference`, `applied_provider_indices`, `restored_provider_indices`, `result: "completed"`, and fault-interval `decision_sequence`, `decision_timestamp_ns`, and complete `canister_decision`. The recorder saves the decision's canonical digest; validators accept only a matching scenario decision timestamped within the applied-to-restored interval. Reject wrong reviewed indexes, unverified restoration, decisions from another time, failed exits, and injectors with arbitrary arguments.

Scenario `artifacts` list relative artifact paths, whole-file SHA-256, and JSON pointers binding every `details` field to raw stdout.
`verify` fails unless every detail field is rederived from raw artifacts. IDs, Authorization digests, Ledger blocks, wallet transactions, canonical hashes, quorum, and expiry evidence must cross-bind multiple Bridge/Base/Ledger/audit artifacts per scenario; one self-report type is insufficient.
Compute `request_sha256` from compact JSON of artifact-ordered `[tool, argv..., transport]` arrays and `response_sha256` from correspondingly ordered raw stdout arrays. Reject arbitrary hashes.

Fill the template with observations, artifact bindings, and secret-stripped request/response digests, then record as follows.

```sh
python3 scripts/evm-rpc-rehearsal/rehearsal.py \
  record /secure/work/rpc-e2e.json preflight /secure/work/preflight.json
```

`external_calls_performed=true` and `through_evm_rpc_canister=true` alone are not evidence.
For successful quorum scenarios, bind EVM RPC Canister ID, Candid call method, internal request digest, quorum response digest, Finalized block number/hash, and transaction hash from `get_audit_events` `EvmRpcObservation` into `canister_audit`.
Allow only the actual production call method appropriate to the scenario: `multi_request` or `eth_getTransactionReceipt+multi_request`. The Canister never broadcasts mint transactions. Verify receipt canonicality by calling `bridgeSnapshot()` at the 2-of-3-agreed receipt hash with EIP-1898 `requireCanonical=true`, matching snapshot block number to receipt height.
`single_provider_failure` and `quorum_loss` also bind `EvmRpcDecision` into `canister_decision`, rederiving configured provider count, required threshold, stop reason, Ledger-call occurrence, and Bridge continuation. `processed_event_mismatch` binds processed storage, missing exact event, Deposit pause, and no refund initiation to the same Finalized observation. Threshold APIs do not expose every provider response before selection, so fault-injection artifacts distinguish 3/3 from 2/3 agreement; Canister audits prove configured count `3`, required threshold `2`, and actual continuation/stop decisions.
`preflight` additionally binds the module hash from fixed `icp canister status <id> -n ic --public --json` capture to reviewed Wasm SHA-256.
Never record planned values, manually entered digests, or dry runs as evidence.

## Staged execution

The state machine proceeds in this order. Completed scenarios cannot be overwritten with different evidence.

```text
AWAITING_PREFLIGHT
  -> READY_FOR_ASSET_FLOWS
  -> READY_FOR_QUORUM_LOSS
  -> READY_FOR_FINAL_PAUSE
     ├─ final_pause -> LAUNCH_READY
     └─ Five additional scenarios -> final_pause -> EXTENDED_COMPLETE
```

This staging rehearsal verifies chain binding and fault behavior of three reviewed Custom RPC endpoints. Production `provider-independence.json` is separate evidence, binding the official EVM RPC Canister's default `BaseMainnet` pool and Bridge Wasm three-provider/two-threshold configuration to source/profile. Do not treat staging Custom RPC rehearsals as proof of organizational independence of production default providers.

Run four asset-flow scenarios, waiting for every transaction to reach the Finalized head.

1. `authorization_mint`: Deposit ID, Ledger block, Authorization digest, Base wallet mint transaction, exact event, Finalized block/hash.
2. `withdrawal_release`: user `approve`, Finalized block/hash for user `createWithdrawal`, fixed quote, ICRC transfer block, and no additional Base transaction.
3. `ledger_fee_guard`: when fixed `KINIC_LEDGER_FEE = 100000 raw` exceeds charged Service Fee, stop before transfer and pause Base Withdrawals. Do not query `icrc1_fee()` at runtime or create cancellation, refund, or another transfer identity.
4. `canonical_receipt`: receipt block number/hash, EIP-1898 `bridgeSnapshot()` probe at receipt hash, and Finalized head.

Run four failure scenarios with test-only configuration.

1. `single_provider_failure`: real `request_deposit` path with configured providers 3, required threshold 2, raw reference for one injected provider failure, threshold success, and continued Bridge processing.
2. `quorum_loss`: required threshold 2, at least two injected provider failures, failed threshold, `RpcInconsistent` or `RpcUnavailable`, and fail-closed behavior before Ledger calls.
3. `authorization_expiry`: proceed to Ledger refund only after saving canonical evidence that Finalized time exceeds the deadline and the Deposit is unprocessed.
4. `processed_event_mismatch`: if processed is true but the exact Authorization event cannot be proved, pause new Deposits without refunding.

Temporarily replace failure endpoints only on the test Bridge Canister; never mix them with normal evidence using the standard three endpoints.
Record temporary configuration, action times, and restoration separately in operational logs.
The EVM RPC client does not expose all threshold-input provider responses or exact agreement counts, so `agreeing_provider_count` is not evidence.
Instead record configured count, required threshold, fault-injection artifacts, and continuation/fail-closed decisions as the threshold certificate.
Keep fault conditions in dedicated `fault` raw artifacts rather than inventing nonexistent Bridge/Canister audit fields. Include `rehearsal_id`, scenario, run reference, configured provider count 3, required threshold 2, failed provider count, and request/config digests, protected by manifest hash. Canister `EvmRpcDecision` proves only continuation or fail-closed decisions.

Run this rehearsal as Gate C operational evidence after unpause; it does not authorize Gate B, activation, or controller handover. The current template's core sequence is `preflight`, `authorization_mint`, `withdrawal_release`, `quorum_loss`, then `final_pause`. Pause Base Deposits/Withdrawals and new Canister Deposits, rereading the Base pause transaction's Finalized block/hash. Complete any five additional scenarios before `final_pause`; appending afterward is rejected.

`scripts/evm-rpc-rehearsal/rehearsal.py` checks exact scenario `details` field names/types fail closed.
Replace template `details` strings with actual objects, recording one scenario at a time as follows.

```sh
python3 scripts/evm-rpc-rehearsal/rehearsal.py \
  record /secure/work/rpc-e2e.json authorization_mint /secure/work/authorization-mint.json
python3 scripts/evm-rpc-rehearsal/rehearsal.py \
  verify /secure/work/rpc-e2e.json
```

## Completion criteria

- Gate C candidate manifests have `LAUNCH_READY` or `EXTENDED_COMPLETE` and `launch_ready=true`, but are not authorization evidence until known current-schema inconsistencies are corrected and reviewed again.
- Five core scenarios bind the official EVM RPC Canister, Base Sepolia, the same rehearsal ID, and the same Bridge Canister. All five additional scenarios set `extended_complete=true`, but do not block production activation.
- Rehearsal source revision/tree, Bridge Canister Wasm, and Bridge runtime bytecode match the release bundle.
- Successful-quorum `canister_audit` is rederived from raw `get_audit_events`; preflight module hash matches release Bridge Wasm.
- Signer triple, Authorization digest, receipt hash, EIP-1898 canonical probe, and exact event agree.
- Quorum loss stops before Ledger calls.
- Only expired unprocessed Deposits refund; processed/event disagreement pauses Deposits without refunding.
- Return Base Bridge and Canister to paused state at rehearsal end. Starting asset admission requires separate explicit approval.
- Do not register `rpc-e2e.json` or referenced `artifacts/` in Gate B; include them in later Gate C evidence. Save only credential-free raw command stdout, without raw authorization, credential URLs, or secrets.

`LAUNCH_READY` and `EXTENDED_COMPLETE` mean only that live rehearsal evidence is structurally complete; they do not approve production deployment, controller handover, unpause, or asset admission.

The current schema binds production-release and rehearsal Wasm to one hash; monitor schema likewise binds production/rehearsal pause principals to one field. Outer staging v8 requires all 10 scenarios, while the fixed `quorum_loss` injector records `request_deposit` but its validator requires `notify_withdrawal`, and reviewed fixed URLs have no fault-control API. Replace these inconsistencies before Gate C collection, add certified update capture, and obtain separate review. Never bypass optional paths to claim success; continue mandatory PocketIC/proof negative evidence for quorum-loss safety.
