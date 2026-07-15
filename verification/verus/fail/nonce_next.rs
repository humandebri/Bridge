use vstd::prelude::*;
#[path = "../../../canister/bridge-core/src/kernel.rs"] mod kernel;
verus! { proof fn nonce_wraps() ensures kernel::nonce_next_spec(0xffff_ffff_ffff_ffffint) == Some(0int) {} }
fn main() {}
