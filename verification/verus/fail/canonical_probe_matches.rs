use vstd::prelude::*;
#[path = "../../../canister/bridge-core/src/kernel.rs"] mod kernel;
verus! { proof fn mismatched_block_is_accepted() ensures kernel::canonical_probe_matches_spec(42, 43) {} }
fn main() {}
