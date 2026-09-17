# Sepolia staging evidence

Current external staging evidence accepts only schema v8. Initialize a new v8 manifest with `scripts/plan007/staging-e2e-driver.sh`, record the fixed 10 stages in order, and reach `SHORT_DELAY_LIVE` only when every acceptance criterion passes. The repository currently stores no completed v8 live-acceptance manifest; do not retroactively mark historical evidence passing.

Checked-in `local-e2e.json` and schema v7 manifests/artifacts under `archive/<source-prefix>/` are read-only audit history. Do not resume, append to, migrate, dual-read, or use v7 for current staging decisions. Generate new schema v8 local evidence outside the repository from a clean commit with `scripts/plan007-local-gate.sh /secure/work/local-e2e.json`, explicitly passing it as the driver's `BRIDGE_STAGING_LOCAL_EVIDENCE`.

`reinstall-decision-2026-08-27.json` and `fresh-stack-2026-08-28.json` fix the history of the one-time destructive reinstall of the existing Canister principal and creation of the current deployment instance/Base contracts. v8 `bootstrap_attestation` only checks those two artifact hashes against current bindings; it neither executes nor authorizes reinstall, contract redeployment, or reactivation. Future updates accept only the same Canister ID/deployment instance and Canister v36 upgrades: schema v36/wire v30 or the one-time migration from deployed version 35.

v8 upgrade, binding, frontend, smoke, wallet, and refund stages require hash-bound stage receipts matching summaries. `rpc_rehearsal` requires a `rpc-rehearsal-manifest` passing its dedicated verifier; `live_acceptance` requires reactivation schedule/execute and monitoring receipts. Self-reported summaries alone cannot reach `SHORT_DELAY_LIVE`.

`archive/dbedb941/`, `archive/f24c09d2/`, `archive/dd0cbdb-failed-rpc-order/`, and `archive/dd0cbdb-failed-activation-salt/` are expired historical chains. Do not use their Deposit ID collisions, activation salt collisions, failed RPC orderings, or other history as current binding or acceptance evidence.

Do not use `short-delay-test-only` evidence for production promotion or a 259200-second Timelock rehearsal. Never store secrets or credential-bearing RPC URLs in artifacts.
