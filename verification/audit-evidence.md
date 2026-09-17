# External audit evidence matrix

The reconciliation `NoProgress` classification and the volatile funding recovery execution guard are covered by registered Rust/PocketIC tests for
`automatic_retry_limit`, `hold_resolution`, `fee_payout`,
`funding_reconciliation_freshness`.
Existing arithmetic proofs for attempt counting and confirmed absence do not establish scan-state comparisons
or exclusion between asynchronous callbacks. These retain runtime assumptions about sequential IC message execution,
guard release when the CDK drops a future, and timer initialization after upgrades.

The authoritative audit record is schema 7
`verification/output/claim-report.json`, deterministically generated from `verification/claims.tsv`. Each `claims[]` entry
represents one claim and displays the following separately:

- `assurance_target`: whether the claim is a release target or supporting model evidence.
- `required_strength`: the minimum evidence strength needed for release.
- `evidence_strength`: `abstract-proved`, `production-linked`,
  or `implementation-proved`.
- `typed_implementation_basis`: production symbols, transaction tests, production-bound
  Verus, bounded conformance, and supporting SMT/Halmos evidence, with their types.
- `release_blockers`: missing production links, transaction tests, or required evidence strength.
- `unproved_reasons` and `external`: unbound proof boundaries and external assumptions.

`release-ready` does not mean there are no external assumptions or TCB. It means only that the target and minimum strength
exactly match the manifest and fixed checker policy, and that the declared production bindings are satisfied.
The current fixed policy makes all 43 claims release targets, requiring `implementation-proved` for 25 and
`production-linked` for 18. SMT, Halmos, generated vectors, and model-only
Verus are supporting evidence and cannot independently establish `implementation-proved` status.

The five conditional liveness results are separate in `conditional_liveness[]`, outside `claims[]`. Their
Lean theorems, strong assumptions, and production-unbound boundaries are documented in `conditional-liveness.tsv` and
`conditional-liveness.md`. Fully qualified theorem names, proposition types, and assumption sets must exactly match the fixed policy,
and undergo Lean typechecking and axiom dependency checks.

Before audit submission, run `scripts/ci-local.sh proofs` with the pinned toolchain and check receipt schema 8:
the source fingerprint, `pass` for all 10 stages, `complete: true`, 43 `release-ready` claims,
`release-blocked: 0`, `model-support: 0`, and the 25/18 evidence-strength partition. The receipt is not tracked in Git;
regenerate it from the checkout being audited.
