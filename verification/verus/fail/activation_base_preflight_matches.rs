use vstd::prelude::*;
#[path = "../../../canister/bridge-core/src/kernel.rs"]
mod kernel;
verus! { proof fn mismatched_expected_pause_satisfies_base_preflight()
    ensures kernel::activation_base_preflight_matches_spec(true, true, true, false) {} }
fn main() {}
