use vstd::prelude::*;
#[path = "../../../canister/bridge-core/src/kernel.rs"] mod kernel;
verus! { proof fn mint_precedes_settlement() ensures
    kernel::scheduler_priority_spec(2) < kernel::scheduler_priority_spec(0),
    kernel::candidate_precedes_spec(1, 1, 0, 2) {} }
fn main() {}
