use vstd::prelude::*;
#[path = "../../../canister/bridge-core/src/kernel.rs"] mod kernel;
verus! { proof fn missing_hash_is_submitted() ensures kernel::nonce_too_low_is_submitted_spec(true, false) {} }
fn main() {}
