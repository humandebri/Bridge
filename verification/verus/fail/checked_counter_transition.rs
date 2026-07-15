use vstd::prelude::*;
#[path = "../../../canister/bridge-core/src/kernel.rs"] mod kernel;
verus! { proof fn zero_counter_decrements() ensures kernel::checked_counter_transition_spec(0, true, false) == Some(-1int) {} }
fn main() {}
