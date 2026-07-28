use vstd::prelude::*;
#[path = "../../../canister/bridge-core/src/kernel.rs"]
mod kernel;
verus! {
fn stale_generation_is_accepted() -> (result: kernel::LeaseOutcomeDecision)
    ensures result == kernel::LeaseOutcomeDecision::Accept
{
    kernel::lease_outcome_decision(2, 1, true)
}
}
fn main() {}
