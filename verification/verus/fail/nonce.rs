use vstd::prelude::*;
#[path = "../../../canister/bridge-core/src/kernel.rs"] mod kernel;
verus! { proof fn prepared_nonce_wraps() ensures kernel::can_assign_nonce_spec(true, true),
    kernel::nonce_next_spec(0xffff_ffff_ffff_ffffint) == Some(0int) {} }
fn main() {}
