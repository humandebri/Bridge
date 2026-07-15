use vstd::prelude::*;
#[path = "../../../canister/bridge-core/src/kernel.rs"] mod kernel;
verus! { proof fn attempt_wraps() ensures kernel::next_attempt_spec(0xffff_ffff_ffff_ffffint) == Some(0int) {} }
fn main() {}
