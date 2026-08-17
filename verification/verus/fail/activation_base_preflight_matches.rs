use vstd::prelude::*;
#[path = "../../../canister/bridge-core/src/kernel.rs"]
mod kernel;
verus! { proof fn unpaused_state_satisfies_base_preflight()
    ensures kernel::activation_base_preflight_matches_spec(false, true, true) {} }
fn main() {}