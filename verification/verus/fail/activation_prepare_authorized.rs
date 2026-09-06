use vstd::prelude::*;
#[path = "../../../canister/bridge-core/src/kernel.rs"] mod kernel;
verus! { proof fn governance_is_allowed_while_bootstrap_controller_is_active() ensures kernel::activation_prepare_authorized_spec(false, true, true, true, 0) {} }
fn main() {}
