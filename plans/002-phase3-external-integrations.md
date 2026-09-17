# Plan 002: Phase 3 external integrations and local E2E

> **Historical record:** this document records schema v2 and implementation boundaries at Plan 002 completion. Timers, queues, and client request IDs have been replaced by explicit operations, bounded external calls, and owner sequences.
> See the repository-root `README.md` and `docs/` for current specifications.

## Status

- **Priority**: P1
- **Risk**: HIGH
- **Depends on**: Plan 001
- **State**: DONE

## Implemented boundary

- Public `request_deposit` derives the Deposit ID from caller and client request ID, checks Safe Base fees/limits, and performs an ICRC-2 pull with the same identity.
- Treat ICRC success and `Duplicate` as equivalent success evidence; call rejection or decode failure enters Reconciliation Hold. After deduplication expires, reconcile the full Ledger/dynamic-archive range and never conclude absence without complete coverage.
- Base monitoring uses `WithdrawalCreated` only for discovery; Safe-head `getWithdrawal` authorizes admission. Require agreement from two of three providers.
- Persist EVM operations as EIP-1559 envelopes with a single stable nonce queue, fixed contract, and fixed selector; retain threshold-ECDSA-signed raw transactions for resubmission.
- Timers resume Withdrawal discovery, Hold reconciliation, ICP Release, mint/acknowledgement/refund submission, and Safe-confirmed receipt/contract-state checks from stable records.
- Before production deployment, provide no legacy migrations and reject all schemas except v2 fail closed. Preserve current-schema unfinished records, nonces, cursors, and accounting across upgrades.

## Deferred to Plan 003

- Actual Settlement Reserve costs, task-priority queues, fee bumps, Runtime Administrator, manual Governance resolution, and operational audit logs.
- Finalize mainnet Base addresses, ECDSA keys, gas caps, and monitoring values.

## Verification

- CI gates check Rust formatting, clippy, workspace tests, Wasm builds, Candid drift, and ICP builds.
- Preserve Base ABI, selector/topic snapshots, and existing Foundry/SMT/Verus gates.
- PicJS installs mock Ledger, mock EVM RPC, and management threshold ECDSA in one PocketIC topology, verifying Deposits, Withdrawal release/acknowledgement, Base Refunds, upgrade preservation of Reconciliation Holds, and stuck receipts.
- E2E automatically assigns free ports and verifies that rebroadcasting an operation uses identical raw transactions. `scripts/ci-local.sh checks` requires mock/Bridge Wasm builds and E2E.
