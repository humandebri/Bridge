# IC mainnet × Base Sepolia staging E2E

This runbook targets the same live stack: current staging Canister, deployment instance, Timelock, Bridge, bSNS, and signer. Hash-fix the destructive reinstall and fresh-stack creation completed on 2026-08-27/28 as one-time history; never use it to rerun, resume, or create another stack. Migrate deployed stable schema v35 to v36 exactly once through same-instance `upgrade`, preserving record wire v30. Subsequent Canister updates permit only reviewed v36 upgrades.

Base Sepolia staging uses a 300-second Timelock under `short-delay-test-only`. Preserve the production 86400-second constraint stated here; never use shortened artifacts/evidence for production rehearsals. Production Canister, KINIC Ledger, Base Mainnet, and SNS are excluded.

## Test Ledger fee

Preserve the currently bound shared TICRC1 Ledger/Index. `test-deployment` Wasm uses `KINIC_LEDGER_FEE = 10000`, matching TICRC1 `icrc1_fee()`, separately from production Wasm's `100000`. Never reuse staging artifacts in production.

## Evidence generations and initialization

- The current state machine accepts only evidence schema v8. v7 manifests, local evidence, and artifacts are read-only audit history, never resumed, migrated, dual-read, or used for passing decisions.
- Run `scripts/plan007-local-gate.sh /secure/work/local-e2e.json` from a clean commit, issuing schema v8 local evidence outside the repository. Do not update/reuse checked-in v7 `local-e2e.json`.
- Initialize a new v8 manifest with `BRIDGE_STAGING_LOCAL_EVIDENCE=/secure/work/local-e2e.json scripts/plan007/staging-e2e-driver.sh init`. Never retroactively promote historical evidence to `SHORT_DELAY_LIVE`.
- `bootstrap_attestation` hash-fixes `evidence/reinstall-decision-2026-08-27.json` and `evidence/fresh-stack-2026-08-28.json`, checking only current binding agreement and that history cannot resume. It performs no reinstall, contract deployment, or activation.

`staging-e2e-driver.sh` is a read-only recorder that validates artifacts and updates the manifest; it does not change Canisters, contracts, or frontends. External upgrades, cycles funding, Base Sepolia transactions, and frontend publication each require separate explicit approval and reviewed dedicated tooling. Do not run raw `icp canister install` as a runbook step or add generic paths accepting `install`, `reinstall`, or `auto`. Store no private keys, identity names, or RPC credentials in the repository.

## Evidence state machine

```text
bootstrap_attestation
  -> preflight
  -> current_schema_upgrade
  -> post_upgrade_binding
  -> frontend_publish
  -> smoke_e2e
  -> wallet_e2e
  -> refund_rehearsal
  -> rpc_rehearsal
  -> live_acceptance
  -> SHORT_DELAY_LIVE
```

Each stage saves source commit, artifact SHA-256, target, raw receipts, and observed postconditions. Upgrade, binding, frontend, smoke, wallet, and refund stages require more than a stage receipt: each needs a stage-specific `*-raw-capture` containing tool/argv/exit code/raw JSON stdout and its digest. Validators reparse stdout and check stage details, `details_sha256`, and `capture_sha256`. Bind RPC summaries to `rpc-rehearsal-manifest` digest and dedicated verifier results; bind `live_acceptance` to `reactivation-schedule-receipt`, `reactivation-execute-receipt`, and `staging-monitoring-receipt`. Never turn failed-command output into PASS evidence.

`preflight` checks live Canister ID, deployment instance, module, controllers, cycles, schema/wire, minimum Withdrawal ID, storage integrity, state counts, fixed RPC order, and provider chain binding. `live-canister-status` requires `canister_id`, `module_hash`, `controller_principals`, and `cycles_balance`; match `canister_id` to the profile Bridge Canister. Match profile `rpcProviderUrlsSha256` to the live RuntimeBinding aggregate digest and bind ordered individual URL digests for all three providers to subsequent rehearsal `rpc_endpoints`. Base Deposits/Withdrawals and Canister Deposits must be live and unpaused.

`current_schema_upgrade` accepts only evidence of applying canonical test-deployment Wasm `target/test-deployment/staging/bridge_canister.wasm` in `upgrade` mode. Require all of the following:

- Unchanged Canister ID, deployment instance, stable schema v36, record wire v30, and minimum Withdrawal ID.
- Module/Candid hashes match reviewed bindings.
- Controller set and all state counts match before/after, with storage integrity `ok`.
- Reject `reinstall`, `auto`, instance drift, old/unknown schemas, and unregistered wire formats.

`post_upgrade_binding` retrieves and verifies live identity, fixed RPC-provider aggregate digest, and runtime/contract bindings. `rpc_rehearsal` follows its independent schema through `final_pause`; this is not terminal for the outer staging state. `live_acceptance` verifies reactivation schedule/execute Finalized receipts under a separate operation ID and confirms IC/Base asset flows are unpaused again.

## Finality delay, wallet, and refund E2E

- Demonstrate successful Deposit review with the Finalized head deliberately lagging about 20 minutes; record Authorization `issued_at_timestamp` and `deadline = issued_at + 900`.
- Using the quote snapshot's `blockTimestamp` as authoritative, record that submission is allowed with 300 seconds remaining, but no wallet call, Ledger pull, intent storage, or Base submission starts at 299 seconds or less or exactly at window end.
- Complete a real-wallet TICRC1 Deposit through Base mint, recording the Base ETH payment receipt and exact processed Deposit.
- Propagate `AuthorizationExpired` and `AuthorizationWindowTooShort` to History as stop reasons. Record that refund is prohibited until Finalized Base time exceeds the deadline and then requires exact unprocessed evidence.
- Fix Solidity acceptance of `block.timestamp + 900` and rejection of `+901` through target artifacts and transaction tests.

## RPC failure rehearsal

Fix RPC order to PublicNode, `sepolia.base.org`, and dRPC; verify preflight chain binding and runtime 2-of-3 quorum. Continue through one provider failure; fail closed on quorum loss, chain mismatch, or canonical receipt disagreement. Do not make RPC URLs or chain IDs runtime-mutable or add runtime `eth_chainId` calls.

## `SHORT_DELAY_LIVE` acceptance criteria

Only new v8 manifests may enter `SHORT_DELAY_LIVE`, requiring every condition below:

- Base Deposits/Withdrawals and Canister Deposits are unpaused.
- Canister ID, module, schema v36, wire v30, deployment instance, minimum Withdrawal ID, and contract/runtime/profile hashes match; storage integrity is `ok`.
- Historical/retired stack identities are excluded from active profiles, signers, and automations.
- Pending Governance, Timelock, Deposit, Withdrawal, Ledger operation, reconciliation, mint reservation, and unpaid liability counts are all zero.
- Fixed RPC-provider preflight chain binding and health are valid.
- Wallet E2E, refund rehearsal, and RPC rehearsal pass.
- Reactivation schedule/execute receipts, monitoring receipts, and Finalized block/hash are saved.

Keep staging unpaused with asset flows enabled after acceptance. Actual Canister upgrades, frontend publication, Base transactions, and acceptance evidence finalization are outside this code change and require separate explicit approval. Production deployment is also outside this runbook.
