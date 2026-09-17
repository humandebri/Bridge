# Plan 004: Prove Plan 003 asset-safety boundaries with Verus

> **Historical record:** this document records proof boundaries at Plan 004 completion. `verification/` is authoritative for the current proof ledger.
> See the repository-root `README.md` and `docs/` for current specifications.

## Status

- **Priority**: P1
- **Risk**: HIGH
- **Depends on**: Plan 001, Plan 002, Plan 003
- **State**: DONE

## Implemented boundary

- Consolidated reserve, nonce, fee payout, administrator, audit sequence, and EVM rank decisions into production-shared kernels without allocation or I/O.
- Verus specifications reference the same expressions, proving boundaries, monotonicity, priority, overflow rejection, and allowed role×action sets.
- The proof manifest maps every asset-safety kernel to passing proofs and domain-specific negative fixtures; CI rejects omissions and proof escapes.
- External responses and stable/async atomicity remain trust boundaries. Rust, storage reopen, and PicJS checks validate coordinator integration.

## Verification

- Exhaustive Rust tests enumerate u128/u64 boundaries and every administrator action×role.
- CI fully checks each negative fixture independently for a postcondition violation.
- `scripts/ci-local.sh checks` runs Rust, Wasm, Candid, PicJS, ICP, Foundry, SMT, and Verus checks together.

## Deferred

- Provider/Ledger/Index/archive response authenticity, production reserve values, key custody, and mainnet deployment belong to Plans 005/006.
