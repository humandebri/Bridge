use vstd::prelude::*;
#[path = "../../../canister/bridge-core/src/kernel.rs"] mod kernel;
verus! { proof fn unprivileged_rotates() ensures kernel::administrator_authorized_spec(4, false, false, false) {} }
fn main() {}
