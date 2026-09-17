# ADR 0025: Prohibit Canister reinstall and fix the Withdrawal history boundary

## Status

Accepted

## Context

Reinstalling the Bridge Canister loses stable state while the Base Bridge contract's Withdrawal event history remains. Connecting an empty Canister to the same Base contract can admit previously processed Withdrawal IDs as new notifications. Separating IC-side identities by deployment instance alone does not prevent existing Base events from becoming unprocessed again.

## Decision

Update an initialized persistent Canister only through reviewed upgrades preserving the same deployment instance. Permit only the one-time post-upgrade migration from deployed stable schema v35/record wire v30 to current v36, restoring confirmed activation evidence. Deployment gates and storage reopen reject reinstall, instance changes, other old schemas, unknown schemas, and unregistered wire formats.

Treat the explicitly approved, completed one-time destructive reinstall and fresh-stack creation on 2026-08-27/28 as fixed historical evidence for current Base Sepolia staging. Preserve that Canister ID, deployment instance, minimum Withdrawal ID, Base contracts, and signer as the active stack; do not replay or resume this history or use it to authorize another stack. Staging evidence schema v8 `bootstrap_attestation` verifies only historical artifact hashes and active binding matches. Future updates also follow the same-instance, current-schema upgrade rules above.

At initial install, set a nonzero 32-byte inclusive `minimum_withdrawal_id` in immutable configuration. Use 1 for normal new deployments. Current-schema test deployments retain a path to set a staging boundary once with empty liability state; do not use it for old-schema migration or recovery from a reinstall that lost history. Reject all changes except reapplying the same value.

After verifying the canonical Withdrawal event, but before record creation, Ledger calls, or liability changes, the Canister compares the event ID with the boundary as a 256-bit big-endian value. IDs below the boundary fail closed with a typed error.

## Consequences

- Exclude history-destroying reinstall from normal operations.
- Even before production deployment, do not restore reinstall that loses active staging history or replacement with another instance as normal operational options.
- Retain the historical one-time reinstall solely as immutable audit evidence; do not resume v7 evidence or migrate it to v8.
- Once set, accept only the identical boundary idempotently and reject changes to another value.
