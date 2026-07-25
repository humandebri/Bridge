use vstd::prelude::*;
#[path = "../../../canister/bridge-core/src/kernel.rs"]
mod kernel;
verus! { proof fn stale_lease_outcome_is_current()
    ensures kernel::lease_outcome_is_current_spec(4, 3, true) {} }
fn main() {}
