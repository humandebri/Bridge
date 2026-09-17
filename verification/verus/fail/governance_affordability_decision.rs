use vstd::prelude::*;
#[path = "../../../canister/bridge-core/src/kernel.rs"] mod kernel;
verus! {
proof fn larger_balance_cannot_cover_the_smaller_balance()
    ensures kernel::governance_affordability_decision_spec(11, 9, 10).1
{}
}
fn main() {}
