use vstd::prelude::*;
#[path = "../../../canister/bridge-core/src/kernel.rs"] mod kernel;
verus! { proof fn active_counter_never_decrements() ensures kernel::counter_delta_spec(true, false) == 0int {} }
fn main() {}
