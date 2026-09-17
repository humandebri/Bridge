# Plan 007: Local → IC Mainnet × Base Sepolia Frontend E2E

## Status

- **Local implementation**: DONE
- **Local promotion evidence**: awaiting rerun after creating a clean commit.
- **IC mainnet / Base Sepolia / Cloudflare Worker**: awaiting explicit approval; not executed.
- **Staging evidence tooling**: DONE (ordered manifest, offline verifier, RPC fault injector)
- **Production / SNS**: out of scope.
- **Production activation dependency**: none. Continue the wallet matrix and five additional RPC scenarios as nonblocking validation.

This plan does not change production assets, production Canister, KINIC Ledger, Base Mainnet, or SNS controllers. Require `local-e2e.json` generated from the same commit before external execution.

## Fixed configuration

Plan 007 local E2E dynamically creates Bridge, Ledger, and Index on PocketIC. Build Bridge with `test-deployment`; directly install checksum-pinned Wasm from `ledger-suite-icrc-2026-03-09` for Ledger and Index. Generate local Canister IDs per run; do not save them in `icp.yaml` environments or mainnet mappings.

IC mainnet `sepolia-staging` preserves the existing `bridge-sepolia` Canister, deployment instance, Base contracts, signer, and shared `testicrc` Ledger/Index as the active stack. Hash-fix the destructive reinstall and fresh-stack creation completed on 2026-08-27/28 as one-time audit history; never use it to rerun, resume, or create another stack. Migrate deployed stable schema v35/record wire v30 exactly once through a same-instance `upgrade` to reviewed v36 Wasm adding confirmed activation evidence, then retain v36.

Do not deploy the test frontend to an IC Asset Canister. Build static assets embedding completed `frontend-profile.json` and publish to Cloudflare Worker `kinic-bridge-ui-test` with the test-only Wrangler command. The Worker serves only static assets, with no server-side state, database, KV, or secrets.

The staging Bridge uses official EVM RPC Canister `7hfb6-caaaa-aaaar-qadga-cai`. The frontend profile requires `testOnly: true`, `environmentMode: short-delay-test-only`, chain ID `84532`, a 300-second activation delay, test-only Canister IDs, contract addresses, and runtime hashes. Always display TEST and five-minute Timelock banners; reject mixtures with Base Mainnet, production Canister IDs, or unofficial EVM RPC IDs.

## Local promotion gate

```sh
scripts/plan007-local-gate.sh /secure/work/local-e2e.json
```

The gate runs Rust, Solidity, Verus, Candid/ABI, UI, and ICP builds plus Playwright E2E connecting PocketIC, real ICRC Ledger/Index, Anvil, and the test frontend. E2E demonstrates:

- Derive Canister Mint Signer and Governance Operator after paused installation.
- Deploy Timelock, Bridge, and bSNS without retaining deployer roles.
- Verify argument-free Canister `schedule_activation()`, early-execute revert, passage of the staging profile's 300 seconds, and `execute_activation()`. Separately verify the default production profile's 24-hour constraint.
- Verify Deposit, EIP-712 Authorization Base mint, post-expiry Finalized reconciliation, Withdrawal, Ledger release, reload, duplicates, and dual-tab leases.
- Preserve unfinished state, nonce queues, pause, and rate limits through a same-Wasm upgrade.
- Match raw EVM transaction hash, RPC-returned hash, and mined receipt.

Do not issue local promotion evidence if any check fails or the working tree is dirty. Even on success, write schema v8 evidence only to an explicit repository-external path; do not update checked-in v7 `local-e2e.json`. Evidence records hashes for source commit, Bridge Wasm, contract runtime, Candid, ABI, and pinned Ledger/Index Wasm.

External stages follow [`sepolia-staging-e2e.md`](../docs/runbooks/sepolia-staging-e2e.md), with order and evidence fixed by `staging-e2e-driver.sh`. The driver only validates and records; it does not upgrade Canisters, publish frontends, or execute Base transactions. The final schema v8 manifest binds local evidence, frontend profile, live artifacts, wallet matrix, refund/RPC rehearsal, same-instance upgrade, reactivation receipts, and monitoring receipts to one source commit.

## External stages after approval

Preserve this external-stage order:

1. `bootstrap_attestation`: hash-fix one-time reinstall/fresh-stack history and verify it cannot be resumed.
2. `preflight`: revalidate repository-external schema v8 local evidence, clean source, active Canister/instance, shared Ledger/Index metadata, unpaused state, storage, and fixed RPC binding.
3. `current_schema_upgrade`: use separately approved tooling to apply only a current-schema `upgrade` to the same Canister; save a receipt showing unchanged identity and all state counts.
4. `post_upgrade_binding`: retrieve live module/Candid, schema/wire, deployment instance, minimum Withdrawal ID, and contract/runtime binding again.
5. `frontend_publish`: after separate approval, publish static assets with the same profile hash to `kinic-bridge-ui-test` and save the receipt.
6. `smoke_e2e`: execute reviewed Deposits/Withdrawals with asset flows unpaused.
7. `wallet_e2e`: record success/failure paths and reload/account/chain changes for OISY, Plug, MetaMask, Rabby, and WalletConnect.
8. `refund_rehearsal`: record refunds at the finalized deadline boundary with exact unprocessed evidence.
9. `rpc_rehearsal`: validate all 10 scenarios under their independent schema, with raw artifacts, through `EXTENDED_COMPLETE`.
10. `live_acceptance`: verify a separate 300-second reactivation schedule/execute operation and monitoring receipts; finish at `SHORT_DELAY_LIVE` with all pending/liability counts zero and asset flows unpaused.

Save new or reused Bridge Canister IDs only in `.icp/data/mappings/sepolia-staging.ids.json`. Do not add existing `testicrc` to mappings for creation targets or modify `production.ids.json`. Use only ICP CLI for ICP operations, never `dfx`.

Obtain explicit approval immediately before each external Canister creation/cycles funding, Base Sepolia transaction, and Cloudflare Worker publication. Create no Gate A/B, key ceremony, or SNS handover.

## Completion criteria

Plan 007 completes when its fixed 10 schema v8 stages, all raw receipts, wallet/refund/RPC rehearsals, same-instance current-schema upgrade, reactivation, and monitoring postconditions bind to one source commit and reach `SHORT_DELAY_LIVE` with zero pending/liability counts and unpaused asset flows. Do not retroactively mark v7 history passing. This is not a production activation blocker. The 10-run/seven-day measurements and SNS proposal upgrades remain in Plans 005/006. x402 is not a Bridge completion condition.
