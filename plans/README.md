# Implementation plans

This index records implementation plans beginning after Base contract Phase 1E completion and their current progress.
Plans 001–004 are completed historical records. The repository-root `README.md` and `docs/` are authoritative for current specifications.

## Execution order

| Plan | Scope | Priority | Size | Dependencies | Status |
|---|---|---:|---:|---|---|
| [001](001-phase2-deterministic-state-machine.md) | Phase 2 deterministic state machine, stable schema, and read-only Candid boundary | P1 | L | — | DONE |
| [002](002-phase3-external-integrations.md) | ICRC Ledger / EVM RPC / threshold ECDSA integrations and Reconciliation Hold | P1 | L | 001 | DONE |
| [003](003-settlement-reserve-runtime-admin.md) | Settlement Reserve, Runtime Administrator, and operational audit logs | P1 | L | 001, 002 | DONE |
| [004](004-plan003-asset-safety-verus.md) | Verus proofs for canister core and cross-system boundaries | P1 | L | 001, 002, 003 | DONE |
| [005](005-production-parameters-key-operations.md) | Finalize target SNS, numeric parameters, key management, and testnet operations | P1 | M | 001–004 | IN PROGRESS |
| [006](006-sns-handover-upgrade-production-preflight.md) | SNS handover, upgrade compatibility, and production preflight | P0 | L | 001–005 | IN PROGRESS |
| [007](007-local-ic-mainnet-base-sepolia-frontend-e2e.md) | E2E from local to IC mainnet test Canister, Base Sepolia, and test frontend | P0 | L | 001–004 | LOCAL DONE / EXTERNAL PENDING |
| [008](008-proof-strength-production-equivalence.md) | Stronger formal evidence: Verus executable proofs, SMT obligations, vector coverage | P1 | M | 001–004 | COMPLETE |
| [009](009-multiple-assets-evm-deployments.md) | Multiple assets and EVM chains while preserving existing KINIC on Base | P1 | L | 006, 008 | PLANNED |

## Dependencies

- Do not implement external calls before 001 fixes states, IDs, idempotency, and stable schema.
- Build 002 as adapters calling 001's pure core, translating ICRC/EVM failures into core transitions.
- Implement 003 after 002 exposes actual Settlement costs and unfinished states. Authority and audit-log design may be reviewed alongside 001.
- Add 004 proof obligations only after production-shared core exists.
- Resolve 005 TBDs before installing initial production values; unresolved values block 006 preflight.
- 007 is nonblocking staging validation independent of production/SNS. External stages require clean-commit local-gate evidence, but unfinished detailed wallet matrices and additional failure scenarios do not block production activation.

## Recommended next step

Rerun Plan 007's local gate from a clean commit to issue promotion evidence, then proceed to IC mainnet test Canister/Base Sepolia external stages after explicit approval. Complete Plan 005 external measurements in parallel.

## Remaining completion criteria

Passing Base contract validation alone is insufficient for production launch. At minimum, require initial-activation scope from Plans 001–006, initial operating values, target SNS, authenticated Gate A/Gate B, and key/monitoring runbooks. Sepolia's five core scenarios and pause/cancel drills belong to Gate C after unpause and do not authorize initial activation or controller handover. SNS Root-only handover and SNS proposal upgrades are independent of initial activation and occur at a separately approved time. Plan 007 additional wallet compatibility and five additional scenarios remain nonblocking.
