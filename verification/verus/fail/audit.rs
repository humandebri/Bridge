use vstd::prelude::*;
#[path = "../../../canister/bridge-core/src/kernel.rs"] mod kernel;
verus! { proof fn audit_wraps() ensures kernel::audit_next_spec(0xffff_ffff_ffff_ffffint) == Some(0int) {} }
fn main() {}
