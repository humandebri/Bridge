use vstd::prelude::*;
#[path = "../../../canister/bridge-core/src/kernel.rs"] mod kernel;
verus! { proof fn release_transfer_accepts_mismatched_fee()
    ensures kernel::release_transfer_matches_spec(85, 6, 85, 5) {} }
fn main() {}
