use vstd::prelude::*;
#[path = "../../../canister/bridge-core/src/kernel.rs"] mod kernel;
verus! { proof fn missing_deadline_binding_is_accepted()
    ensures kernel::authorization_commit_allowed_spec(true, true, true, true, true, false) {} }
fn main() {}
