use vstd::prelude::*;
#[path = "../../../canister/bridge-core/src/kernel.rs"]
mod kernel;
verus! { proof fn paused_state_satisfies_activation_postcondition()
    ensures kernel::activation_postcondition_matches_spec(true, false) {} }
fn main() {}