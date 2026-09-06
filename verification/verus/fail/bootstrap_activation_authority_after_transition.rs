use vstd::prelude::*;
#[path = "../../../canister/bridge-core/src/kernel.rs"] mod kernel;
verus! {
proof fn confirmed_execute_retains_bootstrap_authority()
    ensures kernel::bootstrap_activation_authority_after_transition_spec(true, true)
{}
}
fn main() {}
