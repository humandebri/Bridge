# Notification reservation implementation evidence

The notification reservation adapters use four production-shared kernels in
`canister/bridge-core/src/kernel.rs`. Their Verus specifications expand the same
macros as the executable Rust functions. `verification/verus/manifest.tsv` binds
each expression, proof, rejecting fixture, production call site, and owning claim.
The production binding checker verifies these links; a passing mathematical model
without the executable expression binding is insufficient.

| Kernel | Machine-checked property | Production use |
| --- | --- | --- |
| `notification_reservation_slots` | Each active notification reserves exactly two slots; multiplication overflow returns `None`. | In-flight notification count included in `deposit_reserve_token`. |
| `notification_reservation_excluding_owner` | An unrelated call subtracts zero; an owner subtracts exactly two; subtraction underflow returns `None`. | Paid-call reserve calculation. |
| `nonterminal_deposit_reservation_count` | Idle deposit slots plus separately reserved funding slots equal the owner-index cardinality; inconsistent cardinalities return `None`. | Stable owner-index and funding-attempt counts. |
| `notification_poll_owner` | Entry enables ownership; restoring a saved value returns that exact value, including nested scopes. | Poll-scope entry and restoration through `Drop`. |

The composition theorem `notification_exclusion_preserves_other_owners` proves
that for `L` existing liability slots and `N` active notifications, a polled owner
retains `L + 2 * (N - 1)` slots for everyone else. An unrelated call retains
`L + 2 * N`. This requires `N >= 1` when ownership is asserted and a total that
fits in `u64`. The live `NotificationQuotaGuard` borrowed by the poll wrapper is
the implementation's ownership capability. The existing paid-call kernel then
preserves that reserve after charging, under its registered charging assumptions.

Four deliberate false-theorem fixtures cover wrapped notification counts,
spending another notification's slots, accepting inconsistent index counts, and
leaking ambient ownership after restoring `false`. Each must fail with a proof
rejection; a tool or parser error is not a valid negative result.

The existing abstract reserve and index theorems still apply. These additional
Verus obligations connect their arithmetic premises to the extracted production
expressions. No claim is upgraded solely because a helper name appears in a file.

## Implementation tests and remaining boundaries

The registered core refinement test checks count partitions, other-owner
preservation, overflow/underflow, and nested ownership transitions. The canister
adapter test polls the actual guarded future through `Pending`, `Ready`, and
cancellation, including a nested poll scope. The hold-resolution transaction test
checks index rollback at every write failpoint and consistency after reopen.
These are explicit consumers in the claim-test manifest and links.

Verus does not verify the generic Rust `Future`, `RefCell`, `Drop`, or IC executor
as a whole. The correspondence between a live guard, the active notification
counter, and the currently polled future is supported by the adapter tests and
code review, not a proof of every possible executor schedule. Actual charging,
compiler/runtime behavior, SQL cardinalities, transaction atomicity, and storage
reopen semantics remain registered external boundaries. SQL atomicity is tested
against the implementation rather than represented as an already-proved database.

The full proof gate and release gate remain separate from an impacted proof run;
the latter does not produce a complete production proof receipt.

## Count-to-charging composition and guard transitions

`notification_paid_call_composition_preserves_all_other_reserves` composes the
existing production-shared count partition, owner exclusion, checked reserve,
paid-call, and signing expressions. With `L` existing withdrawal liabilities,
`N` active notifications, indexed deposit count `D`, and funding subset `F`, it
proves that idle deposits `D - F` plus separately reserved funding `F` contribute
exactly `D`. The resulting protected operation count is
`L + 2 * (N - owner) + D`. The theorem covers zero operation count as well as
positive counts, under the successful checked arithmetic premises. With floor
`B`, unit ceiling `U`, attached cycles `A`, and actual charge `C <= A + U`, a
balance admitted at `B + U * count + A + U` retains at least `B + U * count`
after charging. The signing and paid-call requirement expressions agree.

The composition is a supporting theorem in the registered `pass.rs` source; its
kernel constituents retain their individual manifest obligations, negative
fixtures, and claim ownership. It invokes both registered charging preservation
theorems. This adds implementation-linked evidence for `paid_call_cycle_reserve`
and `signing_cycle_reserve`, without weakening their existing contracts.

Two more production-shared expressions implement guard acquisition and counter
release. Acquisition either rejects without producing updated counts or returns
both counters incremented by exactly one within their configured u8 limits.
Release subtracts exactly one from a positive count and retains the pre-existing
defensive saturation at zero. The round-trip theorem proves successful
acquisition followed by one release restores both original counts, and adds then
removes exactly two global reservation slots. The quota acquisition/release
obligations, negative fixtures, and actual guard call sites are registered for
`notification_quota_isolation` and `paid_call_cycle_reserve`.

The registered exhaustive test checks all u8 global/caller counter values for
both runtime lane limits and calls actual `ReservePolicy::required_cycles` for
the count/funding composition. The actual canister adapter test exercises global
and per-caller rejection, the two protected-lane slots, and releases all guards,
checking that the sum of caller counts equals the global count after every
release and that caller entries are removed at zero. It also retains its actual
Future pending/completion/cancellation and nested ownership checks.

These transition proofs establish arithmetic preservation for one acquire and
one release. They do not prove that arbitrary Rust execution invokes `Drop`
exactly once, that every executor schedule satisfies the guard lifetime
invariant, or that a durable owner index faithfully counts every underlying SQL
record. Those remain explicit implementation/runtime boundaries.
