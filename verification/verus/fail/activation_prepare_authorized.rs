use vstd::prelude::*;
#[path = "../../../canister/bridge-core/src/kernel.rs"] mod kernel;
verus! { proof fn governance_uses_initial_schedule_slot() ensures kernel::activation_prepare_authorized_spec(false, true, true, 0, 0) {} }
fn main() {}
