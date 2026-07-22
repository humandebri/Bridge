use vstd::prelude::*;
#[path = "../../../canister/bridge-core/src/kernel.rs"] mod kernel;
verus! { proof fn refresh_generation_wraps() ensures kernel::refresh_generation_next_spec(0xffff_ffff_ffff_ffffint) == Some(0int) {} }
fn main() {}
