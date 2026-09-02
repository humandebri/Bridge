use vstd::prelude::*;
#[path = "../../../canister/bridge-core/src/kernel.rs"] mod kernel;
verus! {
proof fn drifted_generation_is_accepted()
    ensures kernel::confirmed_activation_metadata_matches_spec(3, 41, 2, 41)
{}
}
fn main() {}
