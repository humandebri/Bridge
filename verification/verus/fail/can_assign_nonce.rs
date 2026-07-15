use vstd::prelude::*;
#[path = "../../../canister/bridge-core/src/kernel.rs"] mod kernel;
verus! { proof fn prepared_allows_nonce() ensures kernel::can_assign_nonce_spec(true, true) {} }
fn main() {}
