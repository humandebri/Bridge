# Deployment profiles

`bridge-profile` validates secret-free JSON profiles and measurement evidence.

```sh
cargo run -p bridge-profile -- derive measurements.json
cargo run -p bridge-profile -- validate profile.json
cargo run -p bridge-profile -- validate-test rehearsal-profile.json
cargo run -p bridge-profile -- validate-bundle --offline evidence/release-id
cargo run -p bridge-profile -- verify-live schedule evidence/release-id
```

For execute authorization after 24 hours, replace the phase in the same position with `execute`.

`derive` and `fee-cycles-measurements.template.json` are for Gate C measurements after unpause. Require at least 10 samples each in schema v3 Governance gas, settlement cycles, and fee series; Base fee samples fail unless first and last observations are at least seven days apart. Never make these seven-day measurements prerequisites for Gate B, initial seal, schedule, or execute. Derive Gate B initial values separately from `initial-operational-parameters.json`. Normal pre-deployment `validate` always rejects `test_assets_only = true`; only Sepolia rehearsals may explicitly use `validate-test`.

Production deployment and asset admission must go through `production-release.sh`. `deploy` Gate A verifies only offline artifacts, profiles, and constructor conditions. Deploy both Bridge contract and Bridge Canister initially paused, admitting no assets. Gate B verifies post-deployment runtime, roles, pause, and chain binding through activation attestation retrieved and saved by the Canister using the official EVM RPC Canister's built-in `BaseMainnet`.

Install the production Canister exactly once from a schema 2 `production-canister-plan.json`, independent of the Base contract release profile. Copy `deployments/production-canister-plan.template.json` outside the repository and fill in the bootstrapped Canister ID, clean source, Wasm, and initial configuration. Only `scripts/production-canister-install.sh` converts the typed plan to Candid binary and installs with `--mode install --args-format bin`. Never use `reinstall`, `auto`, or implicit builds. Subsequent profiles may rely only on a schema 3 receipt satisfying every postcondition: public-config initialization, complete storage checks, checksum, Bootstrap lifecycle, empty state, pause, cycles reserve, RuntimeBinding, and controller/module hash.

```sh
scripts/production-release.sh deploy --bundle evidence/release-id \
  --release-inputs deployments/generated/release-id \
  --canister-install-receipt evidence/production-canister-install.json \
  --receipt evidence/release-id/gate-a-receipt.json -- scripts/production-deploy-driver.sh
```

Require the same Bootstrap operating values in the profile through Gate A and paused Base deployment. Derive initial Governance operation ID 0 from the default counter in the fresh, empty Bootstrap state proved by Gate A. Preparation is unreachable until OperationalConfigSealed; later upgrade continuity is established from certified current state and the current-source upgrade transition, not a stored upgrade receipt. Seal rechecks counter 0 before submission, after await, and within stable commit; disagreement preserves Bootstrap and fails closed. After deployment, record exact `scheduleBatch` / `executeBatch` gas estimates, at least 10 distinct Finalized fee blocks, and idle cycles burn in `initial-operational-parameters.json`. Reconstruct calldata from this ID, deployment-instance-derived salt, Governance Operator sender, Timelock target, value 0, the two Bridge unpause payloads, zero predecessor, and 24-hour delay. In the Gate B profile, replace only eight Governance EVM fee fields, `cycles_floor`, and `settlement_cycle_ceiling` using fixed derivation formulas; reject all other Gate A profile drift.

After deployment, keep the production installer as sole controller and seal operating configuration exactly once while the Canister remains paused. Pre-seal Gate B structurally verifies Gate A lineage, proofs, `initial-operational-parameters.json`, and derived values, authorizing only seal. After seal, schedule/execute preparation wrappers refresh Finalized attestation through the fixed confirmation relayer and proceed to production-controller preparation only after fresh live Gate B checks pause, reserves, live module hash, and installer-only control. Relay the fixed artifact anonymously, then confirm through the fixed relayer. After 24 hours, the preparation wrapper again verifies fresh live Gate B and the controller schedule receipt before the same three-step execution of fixed `execute_activation`. Confirmation success alone is insufficient; retain pause until `verify-controller-activation` issues a receipt binding Finalized Base results and Canister state. The stable transaction confirming initial execute permanently consumes only bootstrap activation authority; the installer remains controller until the user separately decides otherwise. Emergency pause never restores bootstrap authority; only the existing Governance principal can prepare subsequent activation. Do not use SNS custom functions for initial activation. Seven-day measurements, keeper drills, and monitoring receipts belong to Gate C after unpause. SNS Root-only handover and same-Wasm SNS upgrades are independent of initial activation/Gate C and occur only at separately chosen times. Accept no arbitrary unpause command.

`governance_operation_id` in `initial-operational-parameters.json` is fixed initially at `0`, checked by the driver before seal submission. `seal_operational_config` accepts only one public `OperationalConfigArgs` argument. The Canister compares internal constant `0` with the next stable Governance operation ID before await, after await, and within stable commit; mismatch prevents seal and preserves Bootstrap.

```sh
export VITE_WALLETCONNECT_PROJECT_ID=<reviewed-project-id-from-ui-assets-receipt>
COMMON=(--phase schedule --bundle evidence/release-id \
  --release-inputs deployments/generated/release-id \
  --receipt evidence/release-id/gate-a-receipt.json \
  --operational-config-seal-receipt evidence/operational-config-seal-receipt.json \
  --confirm-asset-acceptance SCHEDULE_PRODUCTION_ASSET_ACTIVATION)
ARTIFACT=evidence/activation/schedule-artifact.json

scripts/production-release.sh activate "${COMMON[@]}" --step prepare \
  --artifact "$ARTIFACT" --controller-pem /secure/controller.pem \
  --confirmation-relayer-identity confirmation-relayer \
  -- scripts/production-activate-driver.sh
BASE_RPC_URL=https://reviewed-base-rpc.example \
  scripts/production-release.sh activate "${COMMON[@]}" --step relay \
  --artifact "$ARTIFACT" -- scripts/production-activate-driver.sh
scripts/production-release.sh activate "${COMMON[@]}" --step confirm \
  --artifact "$ARTIFACT" --confirmation-relayer-pem /secure/confirmation-relayer.pem \
  --confirmation-receipt evidence/activation/schedule-confirmation.json \
  --activation-receipt evidence/activation/schedule-receipt.json \
  -- scripts/production-activate-driver.sh
```

`VITE_WALLETCONNECT_PROJECT_ID` must match schema 2 `ui-assets.json`. Set the same value in each shell even when schedule and execute 24 hours later run separately.

`execute` after 24 hours requires fresh Gate B and `--prior-schedule-receipt` plus `UNPAUSE_PRODUCTION_ASSET_ACCEPTANCE` at every step. Before preparation, the release wrapper runs `verify-controller-schedule-receipt-live`, stopping unless receipt digests, installer-only control, module hash, and canonical Finalized Base Timelock pending state match. After confirmation, do not consider asset admission complete until `verify-controller-activation execute` issues the activation receipt. For pending fee replacement, run `--step replace` with the production controller before relay, using a new artifact bound to the original artifact, authorization, binding, and profile count/fee caps. If execution stopped before signing and only Canister `Prepared` remains, resume the same preparation idempotently within the five-minute authorization lifetime. After expiry, do not reuse old authorization; persist a new authorization artifact from fresh live Gate B before resuming the same stable operation.

Missing bundles, test profiles, source/profile drift, or gate failure must prevent subsequent commands. Reject mixing unpause/resume into Gate A deployment commands.
Fix Gate A profile `deployment_block` to `0` for undeployed state. Before deployment, the wrapper matches the Canister install receipt to the typed profile and clean source. Immediately before Base submission, pass that receipt to the predeploy verifier, rechecking certified `read_state` module hash against both receipt and profile Wasm SHA-256, and requiring the receipt installer principal as sole controller. After deployment, generate `<receipt>.post-deploy-profile.json` with the actual receipt block, embedding its SHA-256 and the full install receipt in a schema 2 Gate A receipt. Gate B is a separate live manifest based only on that post-deploy profile; `parent_gate_a_manifest_sha256` must match the receipt's Gate A hash, together with source/code binding, post-deploy profile hash, actual deployment block, and installed Canister identity/module/runtime/pause. The canonical hash with only Gate B profile `deployment_block` reset to 0 must also match the receipt's Gate A profile hash; reject other field changes. Use direct RPC verification only through `production-live-preflight.sh verify-monitor-drill BUNDLE` for staging monitor drills, never as the authoritative production Base state.
The external `--receipt` must be byte-identical to `gate-a-receipt.json` in the Gate B bundle.

Use a temporary EOA only for initial contract deployment, retaining no deployer roles. Subsequent Base administration uses the Bridge Canister's separately derived Governance Operator, Runtime Administrator, and Independent Canceller. Standardize production IC operations on `BRIDGE_ICP_IDENTITY` and ICP CLI; never use `dfx`. Initial Timelock activation has schedule/execute phases, each with preparation by the production controller fixed at seal, anonymous relay, fixed-relayer confirmation, and fresh live preflight. Confirmed initial execute permanently consumes bootstrap authority even without controller changes; subsequent activation uses only existing Governance authorization. On failure or ambiguity, keep IC/Base paused and track the same signed transaction.

Install the production Canister paused before finalizing Gate A, fixing its unique Mint Signer and Governance Operator in the profile. Gate A deploy drivers do not use externally supplied constructor JSON. A `bridge-profile` built from fixed source regenerates the bundle profile into a temporary directory, checks both live Canister addresses and pause state, then passes it to contract deployment. Do not reinstall the Canister or call deployment-binding APIs.

Profiles include Canister-derived Mint Signer and Governance Operator matched against Finalized Base attestation, current stable schema, official EVM RPC Canister ID, one emergency pause principal, Wasm/bytecode hashes, Timelock, fixed limits, and fee/liveness/reserve relationships. Timelock delay must exactly match profile and live state. `timelock.runtime_code_hash` is a `0x`-prefixed 32-byte Keccak runtime code hash; generated Bridge constructor arguments, actual post-deployment code hash, and Gate B Finalized attestation must agree. Gate A receipts record canonical Bridge/Timelock deployment transactions/blocks; Gate B rechecks runtime and roles through the official EVM RPC Canister. Monitoring fields record notification-routing SHA-256 and exact SLOs: five-minute detection, 15-minute acknowledgement, and pause on Base/IC within 60 minutes.

Gate A binds the pre-deployment profile plus five Bridge/BSNS build artifacts, six total. Do not add the Canister install receipt as a seventh artifact; embed it fully in the schema 2 Gate A receipt and inherit it transitively into Gate B. Gate B contains exactly 11 artifacts: six current-release build artifacts plus initial operating values, provider independence, UI, Gate A receipt, and immutable Gate A profile. Provider independence is not an SNS Motion: its schema 2 receipt binds release source/profile/current Wasm, official EVM RPC Canister, default `BaseMainnet` pool, empty custom URLs, and runtime-fixed three-provider/two-threshold configuration. Default provider registry and upstream chains remain external assumptions. Exclude RPC rehearsals, monitor/keeper drills, monitoring receipts, historical upgrade records, and seven-day measurements from Gate B. Handover and SNS upgrades remain outside Gate B; Gate C does not authorize them or determine their timing. Use no release-approver signatures or key ceremony. Verify Mint Signer by profile, Canister public configuration from signature-verified queries, and fresh Finalized Base attestation agreement. x402 is not a deployment/activation condition.

`validate-bundle --offline` succeeds only with `gate_a=pass authorizing=true` as formal offline Gate A authorization. In addition to Gate B structure, `verify-live` checks activation attestation no older than five minutes, public RuntimeBinding, reserves, installer-only control, and live module hash through authenticated Canister responses. Initial activation schedule/execute receipt verification uses the same controller condition. Adding SNS Root as a co-controller is a separately approved current-state operation; Root-only control, dapp registration, and SNS upgrades remain separate future operations. Match authority principals, rate/cycles policy, Governance fees, and fixed Ledger fee by comparing public RuntimeBinding `operational_config_sha256` with the value reconstructed from the release profile. Use controller/Governance-only `get_operational_config` to inspect actual values. Missing response authentication or postconditions causes nonzero exit.

Query response authentication means signature verification via `call_with_verification()`. It does not certify the returned application state. Module hashes and controller sets are verified separately through certified `read_state` responses.

The production Bridge Canister runs stable schema v36. Published evidence for corrected v36 and the UI uses the local layout below.
Normal current-release Gate B remains v36-only.
Upgrades and UI publication use certified current state plus a Wasm reproduced twice from clean current source.
`verify-production-current-ui-live` verifies the v36 RuntimeBinding from signature-verified queries, module/controllers from certified `read_state`, Activated and unpaused state, complete indexes, fresh attestation, reviewed `BRIDGE_UI_RPC_CONFIG`, and explicit `sole` or `joint` controller mode.
Use `BRIDGE_PRODUCTION_INSTALLER_IDENTITY` for controller-only queries, matching the production controller.
Generate a new UI asset receipt from the clean source being published and WalletConnect project ID.
Do not publish the new UI until the current module and refreshed live publication authorization are available.
See the [operations runbook](../docs/runbooks/operations.md) for procedures and stop points.

Never record credentials, seeds, private keys, hardware-wallet backups, or credential-bearing RPC URLs in profiles or evidence.

## Local production evidence layout

| Location | Contents | Git |
| --- | --- | --- |
| `artifacts/production/80d9ebb/` | Historical published Wasm, UI file ledger/assets, and gate evidence | Untracked |
| `artifacts/production/80d9ebb/execution/` | Historical operation records and UI publication records; not current authorization inputs | Untracked |
| `artifacts/production/80d9ebb/private/` | Reviewed RPC configuration that may contain secrets | Untracked; directory 700 / file 600 |
| `artifacts/audit/` | Historical formal evidence and audit manifests | Untracked; back up separately |

Use the reviewed release bundle, current runtime profile, and reviewed RPC configuration for UI validation.

```sh
BRIDGE_RELEASE_BUNDLE="/absolute/path/to/reviewed-release-bundle"
BRIDGE_UI_RUNTIME_PROFILE_FILE="$PWD/artifacts/production/80d9ebb/execution/ui-runtime-after-upgrade.json"
BRIDGE_UI_RPC_CONFIG="$PWD/artifacts/production/80d9ebb/private/ui-rpc-config.json"
BRIDGE_UI_CONTROLLER_MODE=sole
```

Generate the new Wasm from the same clean source being validated. Production upgrade and handover drivers do not create preflight, request, recovery, or completion receipts. Historical records may be archived for audit but are never accepted as current authorization.

Root `.gitignore` excludes `artifacts/production/` and `artifacts/audit/` from Git and current proof fingerprint traversal. Do not expand exclusions to weaken validation contracts. Verify retained audit artifacts against their source-approved hashes; they do not authorize current operations. After relocation, check unchanged fingerprints, ignore behavior, and a clean tree after commit.

Do not put build/PocketIC TMPDIR under these deep directories; use a short path on an external volume with sufficient space due to macOS Unix socket limits. Fixed tools, active worktrees, and private keys are not deletion targets for evidence organization.

## IC mainnet × Base Sepolia test staging

Plan 007 IC staging preserves `bridge-sepolia` (`rlhjx-iyaaa-aaaaf-qcnyq-cai`), deployment instance, Base contracts, signer, and shared `testicrc` Ledger/Index fixed in current `sepolia-staging` bindings. Reinstall/fresh-stack creation on 2026-08-27/28 is one-time history, never rerun or resumed. Migrate deployed schema v35 once to v36 through same-instance `upgrade`, preserving wire v30; thereafter allow only reviewed v36 upgrades. Serve the test frontend's static assets through Cloudflare Worker `kinic-bridge-ui-test`, not an IC Asset Canister. Do not touch KINIC Ledger, Base Mainnet, or SNS.

Before external deployment, run repository-root `scripts/plan007-local-gate.sh /secure/work/local-e2e.json` from a clean commit, issuing evidence outside the repository. Dirty trees or hash drift must produce no evidence. External deployment, cycles funding, Base Sepolia transactions, and Cloudflare Worker publication each require separate explicit approval.

Initialize only schema v8 external stages through `scripts/plan007/staging-e2e-driver.sh`, recording `bootstrap_attestation → preflight → current_schema_upgrade → post_upgrade_binding → frontend_publish → smoke_e2e → wallet_e2e → refund_rehearsal → rpc_rehearsal → live_acceptance`. v7 evidence is read-only history, never resumed, migrated, dual-read, or used for current passing decisions. After RPC rehearsal pause, `live_acceptance` verifies separate-operation reactivation and unpaused postconditions; only fully passing v8 reaches `SHORT_DELAY_LIVE`. Different instances, reinstall, old/unknown schemas, or unregistered module/Candid combinations fail closed.

Use existing `bridge-sepolia` in `.icp/data/mappings/sepolia-staging.ids.json` as the authoritative staging Canister ID; create no new mapping. Do not add existing `testicrc` as a creation target. Do not build/publish the frontend before `deployments/sepolia-staging/frontend-profile.json` is complete; afterward publish through the `ui` staging artifact driver to `kinic-bridge-ui-test`. The test frontend rejects Base Mainnet, production Canister IDs, and unofficial EVM RPC Canister IDs, always displaying a TEST banner.

## Base Sepolia staging Bridge target on IC mainnet

Target existing `rlhjx-iyaaa-aaaaf-qcnyq-cai`, reusing `bridge-sepolia` in `.icp/data/mappings/sepolia-staging.ids.json`. Preserve deployment instance, minimum Withdrawal ID, and Base contract binding. Permit only `upgrade` performing the one-time deployed-v35→v36 migration or preserving v36/wire v30. Reject `install`, `reinstall`, and `auto`; provide no future-reinstall init templates or render/validate commands.

Before deployment, recheck target ID/controllers and replenish required cycles. This is test-only staging; do not use it for production assets, production controller handover, or SNS operations.

## Base Sepolia contract-only experiment

[`scripts/base-sepolia-experiment/`](../scripts/base-sepolia-experiment/) performs staged real-transaction validation of the fixed-limit Bridge and 72-hour Timelock.
Resume procedures and secret handling are documented in [`docs/runbooks/base-sepolia-rehearsal.md`](../docs/runbooks/base-sepolia-rehearsal.md).

Scripts create the working public manifest at `base-sepolia-contract-experiment.json`. Do not reuse manifests made with the old Canister-originated mint ABI; recreate them from an EIP-712-compatible Bridge redeployment rehearsal.
Save each public snapshot to `deployments/base-sepolia/YYYY-MM-DD/manifest.json`; never fill unexecuted items with estimates.
See [`base-sepolia/2026-07-13/manifest.json`](base-sepolia/2026-07-13/manifest.json) for the July 13, 2026 record. It is historical evidence for the former Canister-originated mint ABI, not current deployment, preflight, or release evidence.

Because the experimental deployer doubles as Base Admin wallet and Runtime Administrator, it is not evidence of production role separation.
Never save private keys, seeds, keystore passwords, or credential-bearing RPC URLs.

## Live rehearsal through the EVM RPC Canister

[`scripts/evm-rpc-rehearsal/`](../scripts/evm-rpc-rehearsal/) records fail-closed evidence for live Base Sepolia rehearsals from a test Bridge on IC through the official EVM RPC Canister.
Normal CI submits no external transactions; it checks only the recorder and live-only guards.
See [`docs/runbooks/evm-rpc-canister-rehearsal.md`](../docs/runbooks/evm-rpc-canister-rehearsal.md) for execution conditions, scenarios, and secret handling.

Record as an external assumption that the official Canister and provider quorum return the canonical Finalized chain.
