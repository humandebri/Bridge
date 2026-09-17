# Conditional liveness lemmas

The five entries in `conditional-liveness.tsv` are auxiliary Lean theorems, not release-safety claims.
Each assumes that the target terminal transition remains admissible until selected, weak fairness,
progress of external systems, storage, time, and cycles, and the enumerated user or keeper
actions.

The three Deposit results additionally assume that, after automatic processing stops on three consecutive transient failures including the initial attempt, a non-anonymous principal
explicitly calls `continue_deposit` on the same record as many times as required. A successful cycles
top-up alone does not satisfy this assumption or restart automatic retries.

A passing proof gate establishes only that these implications typecheck without project-local axioms.
It does not prove that production schedulers or external systems satisfy their premises, or that admissibility
can be derived from the production implementation. The release summary therefore reports these results separately
as `conditional-liveness`, counting them as neither `release-ready` nor `implementation-proved`.

`DepositTerminalProgressLemmas` pairs a conditional implication for reaching mint with one for reaching refund.
Each quantifies over its own execution and admissibility; it does not state that the same execution of the same Deposit must reach either mint or refund.
The five registered entries include this pair, which adds no release guarantee.
`CommonOperationalAssumptions` requires continuing availability from readyAt onward; a single success does not satisfy the premise.
