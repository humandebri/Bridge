use vstd::prelude::*;
#[path = "../../../canister/bridge-core/src/kernel.rs"] mod kernel;
verus! { proof fn signer_can_rotate() ensures kernel::administrator_authorized_spec(4, false, false, false) {} }
fn main() {}
