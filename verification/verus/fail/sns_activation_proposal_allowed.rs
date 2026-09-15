use vstd::prelude::*;
#[path = "../../../canister/bridge-core/src/kernel.rs"] mod kernel;
verus! { proof fn stale_proposal_is_allowed() ensures kernel::sns_activation_proposal_allowed_spec(true, true, false, true, false, false) {} }
fn main() {}
