# Bridge resource replenishment and emergency pause

History index rebuilding persists progress in batches of at most 100 rows. An error stops its timer; there is no automatic retry or administrator restart API. Investigate and correct the cause before a reviewed same-instance upgrade, which rearms rebuilding from the saved cursor. Upgrading alone does not repair corrupt data. Until rebuilding completes, `list_withdrawals` returns `IndexNotReady`; do not publish the new UI before index verification.

## Schema v36 baseline

The initial mainnet deployment installed v35; the reviewed v36 migration moved activation evidence into dedicated stable records. The current production baseline is v36, as recorded in the approved production checkpoint. Only the current SQLite layout is authoritative: capacity reservations, `nonterminal_deposit_owner_index`, role-specific Governance nonce lanes, indexed funding recovery deadlines, Authorization issuance timestamps, and confirmed activation evidence. Other old/unknown schemas or databases missing tables/counters fail closed on reopen. Future layout changes require explicit migrations with incremented schema versions.

## Daily checks

- Check Bridge cycles; ETH for Governance Operator, Runtime Administrator, and Independent Canceller separately; Governance reserve surplus; Finalized observation time; each role's pending nonce lane; and stop reasons. Also check unsettled Authorizations, unsettled Withdrawal counts, total `amountOut`, oldest observation, and Ledger stop reasons. Mint Signer ETH is not required.
- Do not log Fee Recipients, RPC credentials, raw transactions, or secrets in monitoring logs.
- Check `get_bridge_status.counts` fields `reserved_deposit_mint_operations`, `reserved_deposit_mint_amount`, `pending_ledger_operations`, `retained_audit_events`, `pruned_audit_events`, `retained_deposit_index_entries`, and `audit_retention_warning`. Audit details retain the latest 100,000 events, warning at 80,000. Normal Deposit lists retain the latest 100 per owner; all nonterminal Deposits remain paginatable through a separate index.

Status-page and pre-asset-operation runtime checks query reviewed-profile Base RPC directly from the browser without Canister HTTP outcalls. Normal Bridge views do not automatically validate runtime readiness; they read only lightweight fee/pause Quotes. Immediately before Deposit actions, combine Base Finalized/Safe observations, runtime, pause, epoch, IC pause, and Canister cycles floor, failing closed rather than reusing results older than 60 seconds. Mint Signer ETH is not a condition. Base Governance availability checks whether the selected sender—Governance Operator, Runtime Administrator, or Independent Canceller—covers candidate transaction liability. Final asset-state decisions do not trust browser observations; the Canister revalidates Base through provider quorum.

The Canister's Finalized-head block-response cap is fixed at 16 KiB. On overflow, do not automatically raise the cap or retry; fail closed as RPC unavailable before Ledger processing. Do not fetch receipt blocks. Probe the 2-of-3-agreed receipt hash with `bridgeSnapshot()` under EIP-1898, capped at 4 KiB, using `requireCanonical=true` and snapshot block number to verify the canonical receipt.
Production preflight similarly pins known block hashes for receipts, deployments, saved snapshots, and Timelock role events to Bridge `bridgeSnapshot()` or Timelock `getMinDelay()` through EIP-1898. Do not fetch blocks by number; full block responses are used only to discover the Finalized-head hash.
Base Sepolia checks on 2026-07-23 found a maximum `eth_getBlockByNumber` response of 5,542 bytes across the latest 256 Finalized blocks, within the 16 KiB cap.

The single source of truth for Canister Ledger fees is `KINIC_LEDGER_FEE` in `canister/bridge-canister/src/ledger.rs`.
All Canister Ledger operations use this value. The UI queries target Ledger `icrc1_fee()` directly, without the Bridge Canister, for display and preflight.

Production accepts fixed Ledger fee `100000` raw; staging built with `test-deployment` uses `10000` raw. Activation preflight and runtime `BadFee` handling fail closed on disagreement with the build-selected value.
See the “Test Ledger fee” section in `sepolia-staging-e2e.md` for detailed checks.

Never reuse staging Wasm as a production artifact.
For production builds, synchronize the constant with the live KINIC mainnet Ledger fee and approved profile, updating Candid bindings, Rust/UI/integration tests, and production preflight in the same change.

Current formats are stable schema v36 and record wire v30. Only production/test-deployment `post_upgrade` also accepts the one-time migration from deployed version 35/wire v30. Base Sepolia staging accepts only reviewed upgrades preserving Canister and deployment instance; no reinstall.

## Retention and auditing

If a requested `get_audit_events` sequence has been pruned, the response starts at `oldest_available_sequence`. Full deleted events are no longer in the canister; only `pruned_count`, `pruned_through_sequence`, and `pruned_digest` remain as commitments. No external archive or third-party timestamping is provided here; operations needing long-term details must export them before retention limits are reached.

`list_deposit_ids.history_truncated = true` means older owner-list index entries were removed. Even for Deposits older than `oldest_available_cursor`, known-ID `get_deposit` and idempotent retries of the same request remain available.

Normal production reopen rejects stable state other than schema v36/wire v30. Only post-upgrade accepts the one-time atomic migration from deployed schema v35/wire v30 to v36. All other old/unknown schemas or undecodable databases fail closed even if empty.

If `get_bridge_status.withdrawal_fee_guard_active` becomes true, immediately pause Base Bridge Withdrawals. The record's `last_settlement_stop_reason` and audit event retain `LedgerFeeExceedsServiceFee`; no IC release or reserve change occurs. Compare the build-selected fixed `KINIC_LEDGER_FEE`—production `100000 raw`, staging `10000 raw`—and prepared record's charged Service Fee with the reviewed profile. Then any non-anonymous actor may run `continue_withdrawal` from History. The Canister does not query `icrc1_fee()` at runtime; it starts release from the same record and clears the guard only after revalidating fixed Ledger Fee ≤ charged Service Fee.
Current formats are stable schema v36/record wire v30, rejecting everything except the deployed-v35→v36 post-upgrade migration fail closed. Staging upgrade policy fixes the existing test Canister principal, current module/certified Candid, deployment instance, controllers, and new target module/Candid hashes. Update only through reviewed same-instance upgrades; reinstall is prohibited.
Never manually edit SQLite databases or counters.

Only `bridge_metadata.application_schema_version` is authoritative for schema version. Store Deposit record, owner sequence, Base recipient, Authorization, and expiry/mint confirmation evidence in one stable envelope. Corresponding index `table_counts` are authoritative for pending Ledger operations, open reconciliation holds, and nonterminal Withdrawals; update primary rows, liability indexes, and aggregates in one SQLite transaction.

## Controller-only storage maintenance

Only Canister controllers may call maintenance APIs; Governance principal, pause principal, and Runtime Administrator are not authorized by those roles. Save target Wasm and stable image before execution; never manually repair metadata/databases after failure.

After fresh install and before controller handover, call `initialize_public_config()` once from the controller identity. It derives Mint Signer, Governance Operator, Runtime Administrator, and Independent Canceller from chain-key and atomically saves all four only if every derivation succeeds. Then call public `get_runtime_binding()`, public `get_control_plane_addresses()`, and controller/Governance-only `get_operational_config()`, matching all four profile role addresses. `get_runtime_binding().operational_config_sha256` binds actual operational values and fixed Ledger fee through domain-separated Candid encoding, letting post-handover `verify-live` detect profile drift without exposing values. Initialization failure, uninitialized queries, saved-address mismatches, or binding-digest mismatch block deployment; never substitute empty/manual values. Upgrades preserve saved values and do not require reinitialization.

1. Call `start_storage_validation()` once, then repeat `continue_storage_validation(100)` until `complete = true`. If normal updates cause `StateChanged`, old progress is discarded; explicitly restart during a quiet period.
2. Verify `storage_integrity_check()` returns `ok`. Upgrades do not automatically run this check.
3. Repeat `refresh_storage_checksum(4194304)` until `complete = true`. Each call covers at most 4 MiB; this is not a raw stable-memory copy or filesystem backup.

Production is now deployed at stable schema v36; v35 is the historical initial-deployment/activation root. Normal current-release Gate B accepts only v36. Historical verification may accept v35 or v36 only when profile, Gate A receipt, upgrade-chain terminal, Wasm, and live RuntimeBinding converge on one version. Post-activation UI publication requires a deployed v36 terminal with valid upgrade evidence rooted in immutable v35 Gate B. Reject v34, v37, disconnected mixed versions, dual reads, shims, and generic fallbacks. The new UI cannot target v35. Replace never-deployed formats directly, updating all callers, tests, fixtures, and documentation together.

## ETH and cycles replenishment

- Replenish ETH separately for Base-sending Governance Operator, Runtime Administrator, and Independent Canceller addresses up to each role's required cap. Before signing, Governance checks the smaller Safe/Finalized sender balance covers maximum transaction cost. Do not fund Mint Signer or require ETH for Deposit admission/Authorization issuance. No automatic SNS-token fee exchange occurs.
- Verify cycles cover both the Bridge's 30-day floor and freezing threshold.
- Permissionless `request_deposit` admits the Ledger pull after checking cycle reserve, the default 60-second quota (30 globally/three per principal), and active funding reservations (16 globally/one per principal). Only funded success or `Duplicate` starts paid Base preflight. Definitive Ledger failure releases the reservation and returns quota in the same window, preventing unfunded callers from occupying shared verification capacity. This quota is not a non-refundable failed-request rate limit; monitor `RateLimited`, `ReserveUnavailable`, and failed-pull cycles consumption. The cost of sustained unfunded failures remains an unmeasured operational risk.
- Permissionless `notify_withdrawal` starts at most 60 paid EVM RPC verifications globally per default 600 seconds, with a separate ingestion cap of 30 persisted canonical confirmed events. Missing, pending, reverted, invalid, and duplicate results do not consume ingestion slots. Verification caps limit unauthorized traffic's spending rate but reserve no slots for legitimate notifications, which may be delayed by Sybil exhaustion. Monitor `RateLimited` and cycles decline; set verification caps below the worst-case acceptable RPC budget. On exhaustion, do not repeat notifications; assess pause as an attack/provider incident.
- Do not resume automatically after replenishment. Governance verifies recovered observations and asset state before resuming.

## EVM RPC provider

Production outcalls use only the official EVM RPC Canister's built-in `BaseMainnet`, with empty `custom_evm_rpc_urls`. Initial-deployment `BASE_RPC_URL` is solely EOA submission transport, never stored in production profiles, release bundles, UI, or evidence. Gate A/B chain binding, Finalized receipts, runtime, roles, and pause use Canister audit records from the official route as authoritative. Base Sepolia staging/fault-rehearsal Custom RPC providers are separate configuration. [ADR 0024](../adr/0024-validate-rpc-chain-binding-before-runtime.md) and [ADR 0027](../adr/0027-use-dedicated-eoa-for-initial-base-deployment.md) are authoritative.

## Emergency pause

From one incident origin, monitoring targets detection within five minutes, acknowledgement within 15, and pause on both Base and IC within 60. One-sided pause is not completion. Evidence includes the single emergency principal's actual request ID/audit event and confirmation times for both transactions/calls. Successful pause/cancel paths, evidence, pending Timelock cancellation, and measured 5/15/60 performance belong to post-unpause Gate C evaluation; they do not authorize Gate B, activation, or controller handover. Gate B verifies production chain binding through the official EVM RPC Canister ID and chain. Underlying provider operators, infrastructure, and availability remain external assumptions.

- Call `pause_new_deposits` from the approved pause identity. Base `pauseDepositMints` increments the epoch and invalidates unexpired Mint Authorizations, but refunds wait for Finalized unprocessed evidence after original deadlines. Monitor every Authorization deadline and stop reason.
- For Base anomalies, the single emergency principal calls Canister `emergency_pause`. Success means IC pause and durable Base action queue, not completed Base submission. Run external CLI `drain-emergency` with the same identity to sign, submit, and confirm Deposit/Withdrawal pauses and recorded Timelock cancellations in order.
- Only conflicting fee/schedule/execute operations that have not issued signed artifacts may be discarded and their unused nonce reused. Never discard `SignedAwaitingRelay`, which may already have been submitted externally. Even if an already-submitted execute succeeds, do not resume IC while emergency actions remain; prioritize Base pause.
- Each reactivation `schedule_activation` derives a fresh Timelock salt from the Canister's monotonically increasing operation ID. Never reuse completed activations; pass only the saved operation-ID/salt pair to `execute_activation` or cancellation.
- The Canister does not automatically recover conflicts in Governance Operator, Runtime Administrator, or Independent Canceller nonce lanes. The CLI checks that role's issued hashes, treating only exact raw-transaction resubmission or known-transaction responses as idempotent success. If another hash may have consumed the nonce, stop and audit; never manually edit any role's stable nonce.

## Governance relayer

The Canister never broadcasts Base governance transactions and has no Governance timer. Signed raw-transaction retrieval/broadcast may be anonymous. `confirm` and `run` use the profile-fixed dedicated confirmation relayer; Governance/Pause principals are recovery callers only during failures. Initial deployment uses an external EOA retaining no role. Service Fee signing requests use Governance. Initial activation separates production-controller preparation/bounded replacement, anonymous relay, and fixed-relayer confirmation. Confirmed initial execute permanently consumes only bootstrap authority; subsequent activation authorizes Governance alone even without external controller changes.

```bash
export BRIDGE_CANISTER_ID='...'
export IC_IDENTITY_PEM='/secure/path/governance.pem'
export BASE_RPC_URL='https://...'

npm run governance-relayer -- status
npm run governance-relayer -- prepare --action pause-deposits
# Switch to the confirmation relayer PEM
export IC_IDENTITY_PEM='/secure/path/confirmation-relayer.pem'
# Refresh Base observations with the dedicated confirmation relayer only for manual diagnostics
npm run governance-relayer -- refresh-attestation
npm run governance-relayer -- run
```

`run` retrieves pending signed artifacts; validates raw transaction hash, chain, sender, nonce, target, calldata, gas, and fees; broadcasts; waits for Finalized status; and notifies the Canister. On reverted receipt, stop Finalized waiting immediately; once confirmed, finalize the Canister through `confirm --artifact-file <artifact.json> --receipt-file <confirmation.json> --transaction-hash <hash>`. If interrupted just after broadcast, inspect the same operation with `status` and rerun `run --operation-id <id>`. Identical raw resubmission and RPC `already known` are idempotent success; treat `nonce too low` as already submitted only if the expected hash's receipt exists.

When transient threshold-signing failure leaves an operation `Prepared`, `status` returns `SigningUnavailable`. Retry normal operations with the same `prepare --action ...`, activation through the same Canister API, and emergencies with `drain-emergency`. Preserve saved nonce, target, calldata, and fees; never re-sign if an artifact already exists.

Never generate replacements automatically for stuck transactions. A human checks the current hash and new fees, then explicitly runs the following. The Canister preserves operation, nonce, target, calldata, and gas; re-signs at least 12.5% above the previous generation within configured ceilings; and allows at most three replacements. The CLI does not sign independently or alter fees.

```bash
npm run governance-relayer -- replace \
  --operation-id <id> \
  --artifact-file <current-artifact.json> \
  --output-artifact-file <replacement-artifact.json> \
  --max-fee <wei> \
  --priority-fee <wei>
npm run governance-relayer -- run --operation-id <id>
```

Initial-activation replacement must not use generic `replace`/`run`. Use `production-activate-driver.sh` with `BRIDGE_ACTIVATION_STEP=replace`, privately freezing current artifact, authorization, and preparation receipt, then durably saving the next-generation artifact/receipt under the same authorization hash. After interruption following the Canister call, recover only live pending state with identical fees and exactly the next generation; reject all other generation drift.

Post-deployment relayer `status` is anonymous. Initial activation runs production-controller `prepare-schedule-activation`/`prepare-execute-activation`, anonymous `relay`, and fixed-relayer `confirm` in separate processes. The driver durably saves authorization immediately after live Gate B passes and before Canister calls, allowing idempotent preparation resume while preserving pending state. New preparation requires authorization no older than five minutes; expired authorization when resuming unsigned `Prepared` requires a new artifact from fresh live Gate B. Relay/replacement separately require paused activation attestation no older than five minutes. Only confirmation of an already-Finalized Base transaction is not blocked by that phase-attestation freshness. Immediately before confirmation, revalidate artifact/authorization/preparation/signed-transaction/prior-schedule bindings and management status for sole production controller and current profile Wasm. After success, immediately refresh activation attestation through the fixed relayer before verifying the final receipt. If refresh/final verification stops, retain raw confirmation receipts and recover by idempotently repeating confirmation without resending Base transactions. After artifact creation, bind Gate B/authorization/artifact hashes into the preparation receipt; relay/confirm reconstruct raw EIP-1559 hash, chain, sender, nonce, target, calldata, gas, and fees before using only verified private-frozen inputs. After initial execute, activation preparation/replacement follows Governance policy. Do not extend controller authority to Service Fees, pause-principal rotation, or normal Governance actions. Initial deployment alone uses `production-deploy-driver.sh` with encrypted Foundry keystore and separate password file. Never record secrets, actual paths, or RPC URLs in release artifacts/evidence.

Before staging upgrades, record pending Deposits/Withdrawals, reserves, pending Governance transactions, and Timelock queue. Preserve Base stack, signer, deployment instance, and Canister principal; compare pre/post state counts, module, Candid, and storage integrity. On mismatch, do not activate, reinstall, or switch instances.

For initial production Canister creation, do not set subnet in `icp.yaml`; run `BRIDGE_ICP_IDENTITY=<identity> scripts/production-canister-bootstrap.sh` with the reviewed identity. The script fixes `pzp6e-ekpqk-3c5x7-2h6so-njoeq-mt45d-h3h6c-q3mxf-vpeq5-fk5o7-yae` in `icp canister create --subnet` and requires equality with the actual NNS Registry subnet after creation or mapping reuse. If `.icp/data/mappings/production.ids.json` already contains an ID, do not create another.

Production install plans use schema 2 with template-fixed Bootstrap Governance EVM fees, cycles floor, and settlement ceiling. Gate A profiles use the same three value groups, allowing Canister installation and paused Base deployment first. Afterward derive `initial-operational-parameters.json` from exact activation calldata gas estimates, Finalized fee blocks, and idle cycles burn. Without an SNS proposal, bind `provider-independence.json` to reviewed source/profile/current Wasm, official EVM RPC Canister, default `BaseMainnet` pool, empty custom URLs, and runtime three-provider/two-threshold settings. Pre-seal Gate B structurally verifies 13 artifacts, proofs, review, exact calldata, and derived values, authorizing only seal. `gate-a-profile.json` preserves the immutable deployment profile; `production-canister-upgrade-receipt.json` fixes old/new modules, sole controller, schema, pause, storage validation/checksum, and public-state continuity for a normal production-controller upgrade. Schema 3 `post-gate-a-policy-transition.json` binds both Wasm hashes and upgrade-receipt hash, separately fixing Gate A source, actual upgrade source, and current release-policy source. Wrappers verify Gate A → upgrade → current ancestry and source trees, rejecting unless current source reproduces the same Wasm. After the controller seals once through `scripts/production-seal-driver.sh`, schedule/execute wrappers automatically refresh attestation through the fixed relayer and recheck config digest from signature-verified queries, sole controller, module, pause, reserves, cycles, and pending state in fresh live Gate B. Manual `refresh-attestation` is diagnostic, not another authorization gate. RPC rehearsals, monitor drills, and seven-day measurements occur in Gate C after unpause; they neither authorize Gate B/handover nor automatically update operating values.

Normal production Wasm upgrades require evidence containing an approved checkpoint and additional receipts.
Finalize a read-only snapshot with `scripts/production-canister-upgrade.sh preflight --wasm ABS --checkpoint-evidence ABS --evidence ABS`.
After explicit approval immediately before execution, use the same Wasm/checkpoint evidence plus `--preflight ABS --controller-pem ABS --receipt ABS`, running `execute` with `BRIDGE_ICP_IDENTITY=production` and `BRIDGE_CONFIRM_PRODUCTION_CANISTER_UPGRADE=UPGRADE_PRODUCTION_BRIDGE_CANISTER`.
Accept the confirmation token only for execute.
The driver verifies checkpoint-terminal ancestry to current clean source, source tree, reproducible Wasm, sole controller, v36 RuntimeBinding, Activated state, pause, storage integrity, and reserves.
Execute reruns the common complete proof gate before submission; failure creates no submission.
Read-only preflight and non-resubmitting recover do not duplicate that expensive gate.
Bind frozen evidence hashes into preflight, signed submission, and formal receipts, rejecting replacement during execution.

Exclusively save signed `stored_chunks`, `upload_chunk`, and `install_chunked_code` envelopes and request IDs.
Reject unexpected chunks; after all uploads, recheck pre-execution state before submitting install.
Require persistent public-state continuity across v36 upgrades.
If interrupted after submission, retain sidecars/execution markers and submit no new upgrade until `recover` with identical inputs restores the receipt.
Recover verifies signatures/checkpoint bindings but never resubmits updates.
Chunk-store clearing is a separate explicit operation, not performed by this driver.

Retain formal receipts byte-for-byte for audit. After relocation, use [local production evidence layout](../../deployments/README.md#local-production-evidence-layout) and untracked `relocation-manifest.json` as the authoritative location index. Never rewrite paths embedded in original audit manifests.
Pass only post-checkpoint receipts, chronologically, to `bridge-profile make-production-checkpoint-evidence CHECKPOINT OUTPUT [RECEIPT...]`.
Accept zero additions; retain limits of 16 added receipts, 256 MiB decoded total, and 128 MiB per receipt.
Verify every receipt's hash links, signatures, Wasm, source ancestry, runtime, lifecycle, pause, and storage continuity; reject reuse of pre-checkpoint request IDs or signed install digests.
Normal operations do not reread pre-checkpoint receipts/Git objects or fall back to old chain formats.
Only initial candidate generation and auditing read the old format.
Before reaching limits, create the next unapproved candidate with `rotate-production-checkpoint-candidate EVIDENCE OUTPUT AUDIT`, then pin its hash in a separate commit after review.
Candidate generation/rotation does not change the approval registry.

From clean source with committed mappings, create an external schema 2 plan from `production-canister-plan.template.json`; pass only Wasm rebuilt from that same source to `scripts/production-canister-install.sh --plan ... --wasm ... --receipt ...`. Write receipts to an existing directory outside source checkout on a filesystem enforcing ownership. It must be executor-owned, non-writable by group/others, with ancestors through the mountpoint preventing group/other path replacement. The script fixes install-only mode/raw Candid binary and records post-initialization module/controllers, Bootstrap lifecycle, empty state, pause, storage validation/checksum, cycles reserve, RuntimeBinding, and four role addresses in a typed receipt. After partial failure, investigate live status instead of rerunning normal install. Copy receipt-confirmed role addresses into the release profile and pass the receipt as Gate A `--canister-install-receipt`. The wrapper passes the same frozen receipt to deployment, rechecking certified `read_state` module and installer-only control immediately before Base submission. After Base deployment, record exact schedule/execute gas estimates, at least 10 Finalized fee blocks, and idle cycles burn in `initial-operational-parameters.json`; seal configuration exactly matching the fixed-formula Gate B profile once. Only official EVM RPC Canister `BaseMainnet` observations satisfying runtime/role/pause conditions allow atomic configuration/activation-attestation persistence. Gate B binds source, deployed identity, the normal initial-install-to-current-Wasm upgrade chain, and runtime hashes through Gate A receipt and `post-gate-a-policy-transition.json`; signature-verified queries and certified `read_state` recheck `OperationalConfigSealed`, fresh attestation after both deployment blocks, installer-only control, and reserves. Never schedule/execute under Bootstrap, absent/stale attestation, or profile/module/controller drift. Post-unpause seven-day measurements, keeper drills, and monitoring receipts belong to Gate C, not handover authorization. Controller handover/same-Wasm SNS upgrades are independent and require separately approved timing. Keep production `base_rpc_url` null and `rpc_providers` empty; inject no direct Custom RPC into handover.

If installation/public configuration succeeded but postcondition collection was interrupted, verify the original schema 2 plan, identical Wasm, remaining empty `/Volumes/KINGSTON/KINIC/bridge-production-prep-d85b7ce/production-canister-install-receipt.json.reservation`, sole installer controller, and live module hash. Then, from a reviewed follow-up commit, add `--resume-post-install` to the same command with fixed receipt path `/Volumes/KINGSTON/KINIC/bridge-production-prep-d85b7ce/production-canister-install-receipt.json`. This explicit branch is limited to this approved initial install; it rejects alternate receipt paths and requires exact original plan revision/tree, canonical/file digests, init Candid digest, Wasm digest, Canister ID, and installer principal, without calling install. Reject Git worktree/index/config/pathspec overrides, replacement objects, grafts, split/fsmonitor indexes, and hidden index flags; restrict changes to installer, tests, and this runbook. Reacquire storage validation in at most 100-row batches, checksum in at most 4,194,304-byte batches, and every postcondition. All project-aware ICP CLI calls use a dedicated project root extracted from a verified commit archive into a private working directory, isolating recipe resolution/mutable caches from clean source. Keep the schema 3 receipt bound to original installation plan/source; separately save recovery driver revision/tree, original plan/Wasm, hashes of receipt-writer responses, and receipt hash in `<receipt>.recovery.json`. Publish recovery evidence first and receipt second through verified-inode no-overwrite publication. Delete both reservations only after revalidating both artifacts and source. Evidence existing with a remaining receipt/reservation is terminal partial: preserve artifacts, markers, source, and hashes for manual investigation, never delete to retry. Recovery source requires a normal standalone `.git` directory; use an isolated clean clone, not a linked worktree.

The fixed receipt filesystem must not use macOS `noowners`. To use current `/Volumes/KINGSTON`, obtain separate approval to enable ownership and remove group/other write permissions from ancestors including mount root, then rerun preflight. All ancestor directories must be root- or executor-owned; never traverse owner-writable directories belonging to another UID. Do not bypass checks under a single-user assumption without fixing ownership/permissions.

Before building the validator, recovery verifies current revision descends from original install revision and changes only driver, tests, and this runbook. Freeze the reviewed commit once through sanitized `git archive`, using only snapshot manifest, DID, and mappings. Reject or disable through an empty environment ignored files, Git common directories/alternates/attributes/local includes, Cargo home configuration, and compiler-environment injection. Build the validator with fixed Rust toolchain and offline `--frozen`; run inline Python isolated without `site`. PATH `icp`, Python, shasum, Cargo registry caches, and Rust/Xcode/Homebrew remain externally trusted reviewed host tools. From snapshot creation through reservation cleanup, prohibit other same-UID writers and controller changes from other hosts.

Retrieve management status before artifact publication and immediately before reservation cleanup, requiring the same sole-installer-controller/module-hash tuple. Bind pre-publication status raw digest/tuple into recovery evidence; delete markers only after successful post-publication revalidation. With `R` as original install reservation, `RR` as recovery reservation, `E` as recovery evidence, and `P` as schema 3 receipt, terminal states are:

| State | Meaning | Action |
| --- | --- | --- |
| `R` or `R + RR` | Stopped before artifact publication | Investigate manually; do not rerun |
| `E + R + RR` | Stopped after evidence but before receipt publication | Preserve all evidence as terminal partial |
| `P + R` or `E + P + R + RR` | Stopped after receipt publication around live cleanup checks | Terminal partial; do not proceed to Gate A |
| `E + P + R` | Cleanup interrupted after recovery-marker removal | Manually compare hashes/inodes and live state |
| `P` or `E + P` | All markers removed | Installation or recovery complete |

Do not run Gate A directly from the recovery commit. After recovery, retain `<receipt>.recovery.json` as audit sidecar and return to a clean `d85b7ce8c71e2f85faee0e97cc3cdd7c0eff7dcc` checkout matching install-receipt source. Resume Gate A with an unexpired bundle bound to that source, reviewed release inputs rendered from it, and the same schema 3 receipt. `production-release.sh` rejects unless bundle, checkout, and install-receipt sources match.

For production asset admission, Gate A approves offline artifacts/constructor conditions and a dedicated EOA deploys Timelock/Bridge paused. With production installer as sole controller, pass pre-seal Gate B and seal initial values exactly matching `initial-operational-parameters.json` once. The seal wrapper saves an exclusive reservation before update; ambiguous responses recover only from authenticated live state, without automatic resubmission. Bind finalized seal-receipt hash to subsequent activation authorization. Run `production-release.sh activate --phase schedule --step prepare` using production controller PEM. Before Canister preparation, the wrapper refreshes attestation and verifies post-seal fresh live Gate B, generating the fixed artifact only on success. Pass it to anonymous relay, then fixed-relayer confirmation. Initial activation uses no SNS custom function. Controller activation receipts bind source revision, Wasm, Gate B/seal hashes, certified controllers, Governance operation ID, Timelock operation ID/salt, transaction hash, confirmed signing-attempt generation/timestamp, Finalized block, and live activation status. After 24 hours, execute preparation again verifies fresh Gate B and schedule receipt before the same three-step execution. The stable transaction recording Finalized execute success consumes bootstrap authority and resumes Base Deposits/Withdrawals and IC Deposits. Initial activation does not change controllers; the user separately decides installer removal. Emergency pause never restores bootstrap authority; only existing Governance may prepare later activation. Normal Governance actions always follow existing role policy.

After unpause, record at least seven days and 10-per-type production measurements, keeper drills, monitoring receipts, operational snapshots, and all upgrade history in Gate C. Passing Gate C neither authorizes handover nor automatically triggers/sets its timing. Only after separate explicit operator approval, manually `refresh-attestation` through the fixed relayer, then pass Gate B bundle and seal/schedule/execute receipts to the handover driver. The driver never refreshes attestation and fails closed before controller changes without fresh authenticated evidence. Verify activation lineage from `initial-operational-parameters.json` to live RuntimeBinding and module lineage through post-Gate-A upgrades to current profile Wasm. Validate historical Gate B/initial activation ordering, hashes, and original validity periods, without requiring 90-day freshness at handover. Independently establish current state from live RuntimeBinding, attestation from signature-verified queries, and management status. Immediately before settings update, require installer-only control, Activated state, all Base/IC Deposit flows unpaused, sufficient reserves, and storage integrity `ok`. Save pre/post management status, module, RuntimeBinding, lifecycle, activation status/attestation, storage integrity, and record/audit counts with raw digests, checking equality/nonregression without empty-state requirements. Only on success transition to SNS Root-only while active, then perform a same-Wasm SNS proposal upgrade. Initial Gate B/schedule/execute retain installer-only controller conditions.

Post-activation production UI publication uses `BRIDGE_CHECKPOINT_EVIDENCE` as operational evidence.
Approved checkpoints fix verification results for immutable Gate B and seal/schedule/execute, avoiding rereading all historical files for normal publication.
Additional receipts connect from checkpoint module/RuntimeBinding terminal, requiring final schema v36, Activated state, and unpaused flows.
Reject missing, duplicate, reversed, different-instance, or unapproved-hash evidence and runtime/accounting/storage drift.
Never rewrite original Gate B or formal receipts; archive them for audit.

UI RPC is public configuration separate from Canister RPC. Point `BRIDGE_UI_RPC_CONFIG` to reviewed JSON shaped as `{"schema_version":1,"base_rpc_url":"https://base-mainnet.g.alchemy.com/v2/REVIEWED_APP_KEY"}`; never record production keys in source/logs. This release accepts only Base mainnet Alchemy endpoints. Origin restrictions do not provide secrecy: key-bearing URLs are delivered in the public UI. Preserve fixed Canister RPC configuration.

After Canister upgrade and index completion, generate the publication profile with `bridge-profile render-production-checkpoint-ui-runtime EVIDENCE RPC_CONFIG OUTPUT`.
Deterministically bind the profile to Gate B identity hash, complete checkpoint-evidence hash, verified v36 terminal, and reviewed RPC configuration.
`verify-production-checkpoint-ui-live EVIDENCE RPC_CONFIG UI_RUNTIME_PROFILE` requires byte equality, live RuntimeBinding from signature-verified queries and module hash from certified `read_state`, installer-only control, Activated/unpaused state, no pending Base Governance, reserves, storage, and activation attestation no older than five minutes.
Also require a successful signature-verified `list_withdrawals` query; never treat IndexNotReady or RPC failure as empty history.
Attestation refresh is a production update call requiring separate execution approval.
Stop UI publication until actual upgrade receipts and post-upgrade live publication authorization are available.

For UI-assets-only publication preserving runtime profile, RPC, and checkpoint evidence, run `pnpm --dir ui run deploy:assets-only:check`, then `pnpm --dir ui run deploy:assets-only`. This explicit path verifies live RuntimeBinding from signature-verified queries, module and controllers from certified `read_state`, Activated/unpaused state, no pending Base Governance, reserves, storage, Withdrawal index, and saved attestation content as usual. Saved attestation must be after Gate B and not future-dated, but need not be within five minutes. Instead, fetch production-origin `deployment-profile.js` without redirects and require byte equality with the newly generated public profile twice: before freezing and immediately before Wrangler. Do not publish on mismatch, fetch failure, or unexpected script shape. This path does not prove current Base pause/role freshness and cannot publish changes to runtime profile, RPC, evidence, or Canister.

Generate new UI asset receipts from the clean publication source, binding WalletConnect project ID, every file digest, and aggregate digest.
Controller-only queries use `BRIDGE_PRODUCTION_INSTALLER_IDENTITY`; reject if its resolved principal differs from the approved controller.
After freezing assets, recheck live authorization and unchanged runtime profile immediately before Wrangler.
Do not send to Cloudflare if assets, receipts, profiles, or checkpoint evidence drift.
Proof receipts/current-source fingerprints are independent of checkpoint approval; preserve production-driver complete-proof requirements.

Follow [`token-publication.md`](token-publication.md) for BaseScan source verification, contract-created BSNS ownership verification, and Token Update submissions. These external applications/reviews do not authorize Gate A, Gate B, or activation.

Fixed deployment and activation schedule/execute drivers rerun `scripts/ci-local.sh proofs` from clean source immediately before each action; handover drivers do so immediately before separately approved handover.
Release-candidate operational evidence includes successful manual GitHub Actions `bridge-full-ci` for the target commit SHA. Main pushes run `scripts/ci-local.sh all` once through the same reusable workflow. No scheduled cron full gate exists. GitHub full success does not replace production-driver complete proof receipts; drivers rerun all 10 stages immediately before operations.
Fail closed on proof failure, pre/post source/tree/submodule drift, or bundles containing obsolete `proof-attestation.json`.

Before execute preparation, after proofs/rebuild, attestation refresh, and `verify-live`, run `verify-controller-schedule-receipt-live`, rechecking internal receipt digests, sole production controller, module, and pending Canister Timelock operation. Only after both Base flows are confirmed unpaused does the Canister resume IC. Verify production Base state through saved official EVM RPC Canister `BaseMainnet` attestation and signature-verified Canister queries, never direct Custom RPC URLs. Direct three-provider checks are limited to staging monitor drills.
- Never force-clear Holds, manipulate nonces, or submit arbitrary transactions.
Preflight, execute, and recover all rebuild production Wasm from fixed clean HEAD into an isolated target, rejecting input SHA-256 mismatch before network submission.

For a fresh-install state like the current production template—unsealed, paused, pause principal already production identity, unbound bootstrap marker, separated roles—the upgrade hook is a no-op preserving state/audit. Only actual migration from old SNS Root changes pause principal, marker, and audit.

The handover driver reacquires every live response and reruns the authenticated live verifier, then exclusively/atomically publishes a complete schema 4 pre-send checkpoint alongside the evidence path immediately before settings update. The short verification-to-submission interval assumes no other operator changes pause, activation, runtime, reserves, storage, module, or controllers. If outcome/request ID is unknown, retain the checkpoint and rerun with `BRIDGE_HANDOVER_MODE=recover` at the same evidence path. Preserve any unique request ID even on nonzero response, together with transcript. Recover never resubmits settings updates; it verifies full original recovery source, embedded pre-send checkpoint, Gate B, and activation receipts, creating a separate completion candidate only if live control is SNS Root alone and module, RuntimeBinding, storage integrity, and active state remain continuous. Atomically replace the original checkpoint with a completion receipt only after typed validation; retain it on failure.

## DAO demonstration with individual control and final handover

Use the [DAO reactivation procedure](dao-reactivation.md) for post-launch demonstrations. Preserve initial seal/controller schedule/controller execute evidence unchanged, saving DAO schedule/execute receipts separately. After reactivation, provide both `BRIDGE_DAO_SCHEDULE_RECEIPT` and `BRIDGE_DAO_EXECUTE_RECEIPT` for handover/UI live validation. Reject one-sided input, unexecuted proposals, validator mismatch, or live activation disagreement.

Remove individual control only after separate approval. After Root-only transition, verify SNS registration before the same-Wasm upgrade. Do not register with SNS during the period intended to retain individual control; registration itself removes that controller.

Post-handover UI publication requires both `BRIDGE_SNS_HANDOVER_RECEIPT` and `BRIDGE_SNS_UPGRADE_PROPOSAL_ID` in addition to existing checkpoint evidence/RPC configuration. Dedicated SNS live verification requires Root-only control, dapp registration, upgrade mode, identical uncompressed Wasm, empty Candid upgrade arguments, successful proposal execution, and Root post_upgrade completion after adoption/handover. Do not impersonate Root with an individual identity; retrieve configuration preimages/storage integrity through public read-only release queries and verify unchanged runtime/storage/history-index requirements.

## Mint evidence disagreement

If an owner's refund claim finds `isDepositProcessed(depositId) == true` but `DepositMinted` is missing, duplicated, mismatches Authorization digest/recipient/amount/fee, or cannot bind to a canonical successful receipt, the Canister fails closed without moving funds. Never fall back to refunding or issuing another Authorization.

For audit, save Deposit ID, Authorization digest, originating block, observed Finalized head, runtime hash, signer, epoch, and provider disagreement details. Check contract storage, logs, receipts, and canonical block hash through independent RPC. After resolving the cause, any non-anonymous Principal may retry `request_deposit_refund`. Never manually edit records, counters, or evidence.

## Stable Settlement executor and manual recovery

Mint Authorizations expire 900 seconds after IC-consensus `issued_at_timestamp`; installing a threshold signature requires at least 300 seconds remaining. Using Finalized snapshots from new Deposits/other operations, scan the deadline index in bounded batches and release only reservations with `Finalized timestamp > deadline`, without individual RPC calls. There are no per-Deposit timers, automatic Base reconciliation, or automatic Ledger refunds. When any non-anonymous Principal calls `request_deposit_refund`, issued Authorizations require canonical `isDepositProcessed == false` before refunding the fixed recipient. No funds move before/at the deadline or on RPC disagreement.

For `settlement_scheduler.health = Degraded`, identify stopped jobs, schedules overdue by at least five minutes, and expired leases. While a lease is active, the next wakeup is its expiry; do not immediately rearm a timer for another overdue job. For `Faulted`, record `last_internal_error`/`last_dispatcher_run_at_ns`, pause new Deposits, and upgrade the same Wasm to rearm timers from stable job tables without manually editing SQLite. If unresolved, investigate the failing Wasm.
The base retry interval for transient failures is public `settlement_retry_interval_seconds`, independent of Governance transaction monitoring settings.

After RPC failures, restore provider agreement and canonical Finalized observations before retrying stopped records. For expired leases, verify saved Authorization digests or Ledger transfer identities are unchanged. There are no mint raw transactions or nonces.

For an unnotified Base burn, use History `Check and notify` to run Canister Finalized receipt verification/notification once.
Production uses two keepers with different operators/failure domains, independent of UI actions, monitoring nonterminal Withdrawals and advancing permissionless `continue_withdrawal` one external step at a time. Post-unpause Gate C requires `keeper-drill.json` covering actual burn-to-`Paid`, maximum outstanding time, continued operation after one keeper stops, and manual fallback. `monitoring-receipt.json` binds the same Withdrawal's Finalized Base receipt/`WithdrawalCommitted` event to a signature-verified `get_withdrawal` `Paid` response; `keeper-drill.json` references its artifact digest. Gate B does not require these before unpause. If both keepers stop or maximum outstanding time is exceeded, pause new admission, preserve cycles reserves for existing liabilities, and begin manual fallback.
Save Withdrawal hashes as pending confirmations bound to the active deployment in browser localStorage, without recovery cursors. Recovery uses explicit Withdrawal History `Refresh` and as many `Scan older` actions as needed to retrieve Finalized Base events, then `Check and notify` on the event row for the same hash.
Save Deposit mint hashes bound to the active deployment. History merges Finalized `DepositMinted` logs and Canister Deposits by ID, restoring success only when exact Authorization fields match. This recorded flow requires no post-success IC wallet signature or Canister notification.
Do not save unknown Deposit responses in browser storage. Use `Refresh` to read owner sequence/History, displaying accepted records or explicitly resubmitting with the same next sequence if not accepted.

Any non-anonymous Principal may claim a Deposit refund. Recipient, amount, Ledger transfer identity, Service Fee, and fixed Ledger Fee come only from saved records and cannot be changed by callers. Refunds requiring EVM RPC consume manual Retry quota before external verification, without returning it on RPC failure, `NotClaimable`, or conflict. After resolving a stopped/nonterminal Withdrawal's cause, any non-anonymous identity may call `continue_withdrawal` once. The UI uses the connected IC identity for each operation. Return `Busy` while another record holds an active lease, `RateLimited` on quota exhaustion, or `InsufficientCycles` if external-call cycles would breach the floor. None reschedule a timer.
Continue fee payouts through `continue_fee_payout(payout_id)` with existing payout authority.

- Operations with reserved Governance nonces: retrieve signed artifacts with `governance-relayer status`, submit/confirm the same raw transaction, and request explicit replacement only when needed. Never manually edit nonces or stable counters.
- Mint Authorization: if unsigned, re-sign the same digest. If `AuthorizationAvailable`, submit from a Base wallet before expiry; the UI tracks successful receipts/events. Never reissue with a changed deadline.
- Withdrawal Ledger Hold: `continue_withdrawal` preserves Withdrawal ID, IC Account, and fixed amountOut. Each call advances at most one external transfer/reconciliation step. During deduplication, resubmit the same transfer identity once; after complete absence evidence, save a new identity and stop. Submit it only on the next explicit call. No recipient changes, arbitrary payouts, or Base refunds.
- Deposit funding Hold: no compensation until pull success or complete absence is proved. Success enters `EscrowedUnquoted`; absence enters `Cancelled`.
- Deposit refund Hold: compare original account, saved attempt amount, and fixed Ledger fee. Refund `gross - ledger_fee` before Authorization issuance or `gross - charged_service_fee - ledger_fee` afterward; never return initial pull, finalized Service Fee, or refund fees. Success evidence enters `Refunded`; any non-anonymous caller may renew a claim to reconcile ambiguity. Change attempt number, created-at time, and memo only after complete absence proof. Definitive `BadFee` stops on fixed-fee mismatch without changing refund payload.
- Stop reasons: record History or `get_deposit`/`get_withdrawal` `last_settlement_stop_reason`; resolve external failures before Continue.

Default manual Retry quota per 10-minute window is 60 globally, six per caller, and three per record. Profile changes must preserve `1 <= per_record <= per_principal <= global` and a 60–3600-second window.

## Gate C RPC and monitor evidence

`rpc-e2e.json` and `monitor-drill.json` are post-unpause Gate C evidence, not authorization inputs for 13-artifact Gate B, seal, schedule, execute, or controller handover. Seven-day/10-per-type measurements likewise do not authorize handover. Current templates still conflate production/rehearsal Wasm hashes and pause principals, require all 10 staging-v8 scenarios, mismatch `quorum_loss` injector/validator operations, and lack fault control for fixed URLs. Replace schemas/capture paths and obtain separate review before Gate C collection; reject bypassed evidence. Until then, PocketIC/proof gates remain authoritative mandatory quorum-loss negative evidence.

### Mint epoch observations across upgrades

The observation that may change between upgrade-history terminal and live runtime is the mint Authorization epoch saved by Deposit admission. Only when Activated/unpaused, all other runtime fields match, TTL is unchanged, and epochs are positive/monotonic may old/current digests be recomputed from the same typed controller-authenticated `get_operational_config` preimage. Reject arbitrary digests or configuration changes. Drivers save raw Candid/SHA-256 in preflight/receipt `before_operational_config`, checking snapshot status/runtime agreement. Receipts reaching v36, including 36→36, require this evidence; only already-published 35→35 receipts may omit it. Preserve pre/post-runtime equality for the upgrade itself.

`bridge-profile operational-epoch-digests OPERATIONAL_HEX_FILE LEDGER_FEE OLD_EPOCH NEW_EPOCH` is a diagnostic command displaying only two digests from canonical Rust Candid encoding. UI live validation retrieves the preimage using Gate B controller-bound `BRIDGE_PRODUCTION_INSTALLER_IDENTITY`, comparing history terminal under identical conditions. This is implementation verification depending on authentic external observations and IC response/controller authentication, not abstract proof of external facts.

## Launcher execution-cycles replenishment

The Bridge checks execution cycles immediately after init and successful upgrades, and every 24 hours.
At balances at or below 2,000,000,000,000 cycles, it calls fixed launcher `xfug4-5qaaa-aaaak-afowa-cai`
using `request_cycles : () -> (variant { Ok; Err : RequestCyclesError })`.
No amount is specified; launcher authorization, funds, and request intervals govern replenishment. This is not a transfer to a cycles-ledger account.
The timer is independent of asset-operation pause/configuration seal and is rearmed after upgrade.

Controllers may manually call `check_cycles_top_up : () -> (variant { Ok; Err : text })`.
Example: `icp canister call <Bridge-canister-id> check_cycles_top_up '()' --network ic --identity <controller-identity>`.
Balances above threshold and already-running requests also return `Ok`; `Ok` alone does not prove replenishment.
Check execution balance with `icp canister status <Bridge-canister-id> --network ic --identity <controller-identity>`.

Find request failures in canister logs under `cycles top-up failed:`. Controllers retrieve them through
IC Dashboard canister logs or management canister `fetch_canister_logs`.
For `Unauthorized`, check launcher registration; `TooSoon`, request interval; `LauncherBalanceTooLow`, launcher funds;
for `TopUpFailed`, investigate execution failure. A timeout may occur after successful funding, so check execution balance first.
Automatic retry occurs at the next 24-hour check, with no additional short-interval retries.
Daily checks do not guarantee protection from rapid spending or depletion during stops/freezing.

Before production use, verify Bridge authorization registration, replenishment amount/request interval,
and actual execution-balance funding through launcher administrator procedures or implementation. Do not assume public Candid `register_shared_memory`
also grants replenishment authorization. This change alone performs no registration or production upgrade.

Bind local `cycles_top_up_request_policy` conditions to shared kernels and Verus.
Unit/PocketIC tests verify in-flight flags, controller inputs, timers, and Candid responses,
but whole-operation guarantees remain `partial`, depending on launcher authorization/funds/success and IC runtime.
Stable schema v36 is unchanged. Existing `attempts` counts consecutive automatic failures. Transient RPC, signing, Ledger, and insufficient-cycles failures stop automatically on the third attempt including the first; successful replenishment alone does not resume. Explicitly call `continue_deposit` from the same History record for one recovery attempt. Only actual progress resets failures to zero and resumes automatic processing. Fee payouts use administrator-only `continue_fee_payout`.

`settlement_cycle_ceiling` is a per-liability reservation and one-operation margin, not cycles attached to an external call. Immediately before paid EVM RPC/threshold signing, require the checked sum of existing-liability reserves, actual call cycles, and this margin. Even zero-attached-cycles calls such as Ledger calls are prohibited if reserves plus margin cannot be maintained.

Funding recovery sets a one-shot timer for the stable deadline index's oldest `recovery_due_ns`. Replace it only when an earlier attempt is added; each callback handles at most one step and rearms for the next deadline. Do not poll fruitlessly every 30 seconds until a 24-hour deadline. Register no extra timer while the callback runs; on completion/cancellation, release execution authority and reread the earliest deadline. Do not rearm recovery while Deposit admission is paused. Resume from the deadline index after confirmed activation restores admission or upgrade initialization while unpaused.

Successful reconciliation communication alone is not progress. If Ledger/Index phase, page/archive position, and confirmed watermark are unchanged, count `LedgerUnavailable` as a consecutive failure. Formal Deposits/fee payouts stop after three; pre-formal-Deposit funding recovery has no attempt cap and reschedules after 30 seconds. Complete absence determination/reservation release still requires a fresh scan after the deduplication period.


### Mainnet mint recovery Worker

`ui/recovery-worker` is a production-only candidate-hash discovery API; it performs no mint, refund, or IC notification. Store secrets in Cloudflare Secrets `ALCHEMY_API_KEY`, `CURSOR_KEY`, and `BRIDGE_PROFILE_JSON`. Use a dedicated server-capable Transfers API key, not the UI's Origin-restricted public key. `CURSOR_KEY` must contain at least 32 cryptographically random characters.

Publish with pinned Node.js using `node ui/recovery-worker/release.mjs dry-run`, then `deploy`. Required variables: `BRIDGE_UI_RUNTIME_PROFILE_FILE`, `BRIDGE_CHECKPOINT_EVIDENCE`, `BRIDGE_UI_RPC_CONFIG`, `BRIDGE_PROOF_RECEIPT`, `RECOVERY_ALCHEMY_API_KEY`, `RECOVERY_CURSOR_KEY`, `BRIDGE_RECOVERY_SMOKE_DEPOSIT_ID`, and `BRIDGE_RECOVERY_SMOKE_TRANSACTION_HASH`. Smoke targets must be existing successful production mints. Verify clean source, complete current proof receipt, and v36 live approval; check Alchemy discovery before publication and Worker discovery afterward. Proceed to normal production UI publication only on success. Smoke tests submit no new transactions.

On failure, set that Worker's `RECOVERY_ENABLED` to `false`, stopping only discovery while known-hash receipt tracking continues. Log no secrets, RPC URLs, or addresses; monitor HTTP status/duration, 429s, and upstream failures. IP/Deposit limits are soft per-Cloudflare-location limits, not a global spending cap. Failed recovery never implies “not submitted” or permits automatic resubmission. History does not support manual recovery by pasting hashes.

### v36 Authorization TTL migration

The v36 upgrade from 600 to 900 seconds computes the post-migration digest by replacing only TTL in controller-retrieved pre-migration operational configuration. Preserve epoch, other configuration, deployment, lifecycle, pause, and public asset state; verify pre/post snapshots and respective public-state digests. Normal upgrade/epoch-progress conditions remain unchanged. History chains and checkpoint suffixes use the same limited check. Reject reverse migration, other TTLs, mixed schemas, or missing evidence.

This release assumes operationally that no active 600-second Authorizations remain. Do not extend, reissue, or compatibly submit old Authorizations. Existing checkpoint drivers pass saved pre-migration configuration unchanged to validation. When validating with old CLI `verify-production-upgrade-state-preserved`, append raw Candid hex from pre-migration `get_operational_config` to existing arguments. Omitting it preserves prior validation conditions.

### Mint recovery candidates and failure notifications

Separate page fetching from receipt-validation queue consumption and Deposit rotation. Prioritize the active page sequence each turn; after enumeration, advance to the next Deposit. Search failure delays that Deposit 30 seconds while others continue. Move unavailable/unfinalized candidates to a 30-second retry queue and continue others. Process at most four candidates per step and retain at most 10,000 unverified candidates; near the cap, pause search instead of dropping candidates. Do not advance search position if candidates cannot be durably saved.

Worker-signed cursors bind the fixed search upper bound, pageKey, and enumerated-range resume block. pageKey expires nine minutes after each issuance; afterward resume including the last block without pageKey. Handle blocks spanning pages without omissions, deduplicating saved hashes. After all pages, wait 60 seconds and rescan from the original Authorization start block to catch indexing delay. Neither candidates nor unsuccessful searches are settlement evidence.

The 30-second failure cooldown for `notify_deposit_mint` applies only to a caller/Deposit pair. Changing hashes does not let the same caller spam; another party's failures do not restrict other callers or the confirmation relayer. Preserve global RPC budgets, in-flight exclusion, and strict Finalized mint evidence validation.
